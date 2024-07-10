use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::{fmt, fs, io};

use anyhow::Result;
use git2::{Repository, Status};
use slab_tree::{NodeId, Tree, TreeBuilder};

use crate::git_status;
use crate::tree_creator::Item;
use crate::{git_restore_staged, tree_creator};

pub type PathChanges = (ChangeTree, BTreeMap<String, Item>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Add,
    Modify,
    Delete,
    Rename(String),
    Copy(String),
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Change::Add => write!(f, "A"),
            Change::Modify => write!(f, "M"),
            Change::Delete => write!(f, "D"),
            Change::Rename(name) => write!(f, "R '{}' ->", name),
            Change::Copy(name) => write!(f, "C '{}' ->", name),
        }
    }
}

impl From<Status> for Change {
    fn from(status: Status) -> Self {
        match status {
            Status::INDEX_NEW => Change::Add,
            Status::INDEX_MODIFIED => Change::Modify,
            Status::INDEX_DELETED => Change::Delete,
            Status::INDEX_RENAMED => Change::Rename("".to_string()),
            _ => {
                println!("{status:?}");
                unreachable!()
            }
        }
    }
}

#[derive(Debug)]
pub struct Node {
    pub path: String,
    pub item: Option<Item>,
    pub change: Option<Change>,
}

#[derive(Default)]
pub struct ChangeTree {
    pub new_repo: bool,
    pub tree: Tree<Node>,
    pub idx: HashMap<String, NodeId>,
}

pub fn build(items: Vec<Item>, repo: &Path) -> Result<PathChanges> {
    use crate::TREE_DIR;

    if repo.exists() && !repo.is_dir() {
        anyhow::bail!("Destination is not a directory");
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

    crate::git_add(repo.workdir().unwrap(), ".")?;
    let items_map: BTreeMap<_, _> = items
        .into_iter()
        .map(|data| (data.path.clone(), data))
        .collect();
    // let iter = repo
    //     .statuses(Some(
    //         git2::StatusOptions::new()
    //             .include_ignored(false)
    //             .recurse_untracked_dirs(true)
    //             .sort_case_insensitively(true)
    //             .include_untracked(true)
    //             .update_index(true),
    //     ))
    //     .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    // let mut statuses: Vec<_> = iter.iter().collect();
    // statuses.sort_unstable_by_key(|status| status.path().unwrap().to_string());
    let mut nodes_idx = HashMap::new();
    let tree_builder = TreeBuilder::new();
    // if statuses.is_empty() {
    //     return Ok((
    //         ChangeTree {
    //             new_repo,
    //             tree: tree_builder.build(),
    //             idx: nodes_idx,
    //         },
    //         items_map,
    //     ));
    // }
    let mut tree = tree_builder
        .with_root(Node {
            path: "".to_string(),
            item: None,
            change: None,
        })
        .build();
    let root_id = tree.root_id().unwrap();
    let root = tree.get_mut(root_id).unwrap();
    nodes_idx.insert(
        root.as_ref()
            .data()
            .item
            .as_ref()
            .map_or("".to_string(), |x| x.path.clone()),
        root_id,
    );
    for line in git_status(&repo.workdir().unwrap().join(TREE_DIR))?.lines() {
        let change = line.chars().take(1).collect::<String>();
        let mut path = line.chars().skip(3).collect::<String>();
        let change = match change.as_str() {
            "M" => Change::Modify,
            "A" => Change::Add,
            "D" => Change::Delete,
            "R" | "C" => {
                let capture = path
                    .split(" -> ")
                    .map(|x| x.to_string())
                    .collect::<Vec<String>>();
                // let re = Regex::new(r"^(\S+)\s+->\s+(\S+)").expect("Failed to create regex");
                // let capture = re.captures(path.as_str()).expect("Failed to match");
                let mut old_path = capture[0].clone();
                if old_path.starts_with("\\\"") {
                    old_path = old_path
                        .strip_prefix("\\\"")
                        .unwrap()
                        .strip_suffix("\\\"")
                        .unwrap()
                        .to_string();
                } else if old_path.starts_with('\"') {
                    old_path = old_path
                        .strip_prefix('"')
                        .unwrap()
                        .strip_suffix('\"')
                        .unwrap()
                        .to_string();
                }
                path = capture[1].to_string();
                match change.as_str() {
                    "R" => Change::Rename(old_path),
                    "C" => Change::Copy(old_path),
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        };
        // println!("{} {}", change, path);
        if path.starts_with("\\\"") {
            path = path
                .strip_prefix("\\\"")
                .unwrap()
                .strip_suffix("\\\"")
                .unwrap()
                .to_string();
        } else if path.starts_with('\"') {
            path = path
                .strip_prefix('"')
                .unwrap()
                .strip_suffix('\"')
                .unwrap()
                .to_string();
        }
        let parent_node_id = get_parent(&path, &mut tree, &mut nodes_idx);
        let mut parent_node = tree.get_mut(parent_node_id).unwrap();
        if let Some(child_node_id) = nodes_idx.get(&path) {
            let mut child_node = tree.get_mut(*child_node_id).unwrap();
            child_node.data().item = items_map.get(&path).cloned();
            child_node.data().change = Some(change);
            continue;
        }
        let child_id = parent_node
            .append(Node {
                path: path.clone(),
                item: items_map.get(&path).cloned(),
                change: Some(change),
            })
            .node_id();
        nodes_idx.insert(path, child_id);
    }

    git_restore_staged(repo.workdir().unwrap(), ".")?;

    Ok((
        ChangeTree {
            new_repo,
            tree,
            idx: nodes_idx,
        },
        items_map,
    ))
}

fn get_parent(path: &str, tree: &mut Tree<Node>, idx: &mut HashMap<String, NodeId>) -> NodeId {
    let parent = path
        .find(tree_creator::PATH_SEPARATOR)
        .map_or("", |i| &path[..i]);
    if parent.is_empty() {
        return tree.root_id().unwrap();
    }
    if let Some(parent_node) = idx.get(parent) {
        *parent_node
    } else {
        let parent_node_id = get_parent(parent, tree, idx);
        let mut parent_node = tree.get_mut(parent_node_id).unwrap();
        let child_id = parent_node
            .append(Node {
                path: parent.to_string(),
                item: None,
                change: None,
            })
            .node_id();
        idx.insert(path.to_string(), child_id);

        parent_node_id
    }
}
