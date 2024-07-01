use crate::TREE_DIR;
use std::collections::HashMap;
use std::path::Path;
use std::{fs, io};

use git2::{Repository, Status};
use slab_tree::{NodeId, Tree, TreeBuilder};

use crate::tree_creator;
use crate::tree_creator::HashKind;

#[derive(Debug, Clone)]
pub enum Change {
    New,
    Modify,
    Delete,
    Rename(String),
}

impl From<Status> for Change {
    fn from(status: Status) -> Self {
        match status {
            Status::WT_NEW => Change::New,
            Status::WT_MODIFIED => Change::Modify,
            Status::WT_DELETED => Change::Delete,
            Status::WT_RENAMED => Change::Rename("".to_string()),
            _ => unreachable!(),
        }
    }
}

pub struct Node {
    pub path: String,
    pub change: Change,
    pub hash: Option<(HashKind, String)>,
}

#[derive(Default)]
pub struct ChangeTree {
    pub new_repo: bool,
    pub tree: Tree<Node>,
    pub idx: HashMap<String, NodeId>,
}

pub fn build(repo: &Path) -> io::Result<ChangeTree> {
    if repo.exists() && !repo.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Repository must be a directory",
        ));
    }
    if !repo.exists() {
        fs::create_dir_all(repo)?;
    }
    let mut new_repo = false;
    let init_repo = |_| {
        new_repo = true;
        Repository::init(repo)
    };
    let repo = match Repository::discover(repo).or_else(init_repo) {
        Ok(repo) => repo,
        Err(e) => Err(io::Error::new(io::ErrorKind::Other, e))?,
    };
    repo.index()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let iter = repo
        .statuses(Some(
            git2::StatusOptions::new()
                .include_ignored(false)
                .recurse_untracked_dirs(true)
                .sort_case_insensitively(true)
                .include_untracked(true)
                .update_index(true),
        ))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut statuses: Vec<_> = iter.iter().collect();
    statuses.sort_unstable_by_key(|status| status.path().unwrap().to_string());
    let mut nodes_idx = HashMap::new();
    let tree_builder = TreeBuilder::new();
    if statuses.is_empty() {
        return Ok(ChangeTree {
            new_repo,
            tree: tree_builder.build(),
            idx: nodes_idx,
        });
    }
    let mut tree = tree_builder
        .with_root(Node {
            path: "".to_string(),
            change: Change::New,
            hash: None,
        })
        .build();

    let root_id = tree.root_id().unwrap();
    let root = tree.get_mut(root_id).unwrap();
    nodes_idx.insert(root.as_ref().data().path.clone(), root_id);
    for status in statuses {
        let mut path = status.path().unwrap().to_string();
        path = path
            .strip_prefix(&format!("{TREE_DIR}/"))
            .unwrap()
            .to_string();
        let status = status.status();
        let parent_path = get_parent(&path);
        let mut parent_node = tree.get_mut(*nodes_idx.get(parent_path).unwrap()).unwrap();
        let mut change: Change = status.into();
        match change {
            Change::Rename(_) => {}
            _ => {}
        }
        let child_id = parent_node
            .append(Node {
                path: path.clone(),
                change,
                hash: None,
            })
            .node_id();
        nodes_idx.insert(path, child_id);
    }

    repo.commit(
        Some("HEAD"),
        &repo.signature().unwrap(),
        &repo.signature().unwrap(),
        if new_repo { "Initial commit" } else { "Update" },
        &repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap(),
        &[&repo.head().unwrap().peel_to_commit().unwrap()],
    )
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    Ok(ChangeTree {
        new_repo,
        tree,
        idx: nodes_idx,
    })
}

fn get_parent(path: &str) -> &str {
    path.find(tree_creator::PATH_SEPARATOR)
        .map_or("", |i| &path[..i])
}
