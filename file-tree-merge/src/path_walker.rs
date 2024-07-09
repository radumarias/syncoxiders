use std::fs::FileTimes;
use std::io;
use std::path::{Path, PathBuf};

use colored::Colorize;
use walkdir::{DirEntry, WalkDir};

use crate::{tree_creator, IterRef};

pub struct PathWalker {
    path: PathBuf,
}

impl PathWalker {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

pub struct Iter {
    it: Box<dyn Iterator<Item = walkdir::Result<DirEntry>>>,
    path: PathBuf,
}

impl IterRef for PathWalker {
    type Item = io::Result<tree_creator::Item>;
    type Iter = Iter;

    fn iter(&self) -> Self::Iter {
        Iter {
            it: Box::new(
                WalkDir::new(self.path.clone())
                    .follow_links(true)
                    .into_iter(),
            ),
            path: self.path.clone(),
        }
    }
}

impl Iterator for Iter {
    type Item = io::Result<tree_creator::Item>;

    fn next(&mut self) -> Option<Self::Item> {
        self.it.next().map(|entry| {
            entry
                .map(|entry| {
                    let path = entry.path();
                    println!(
                        "{}",
                        format!("Checking '{}'", path.to_str().unwrap()).cyan()
                    );
                    let path_rel = path.strip_prefix(&self.path).unwrap();
                    let atime = entry.metadata().unwrap().accessed().unwrap();
                    let mtime = entry.metadata().unwrap().modified().unwrap();
                    let times = FileTimes::new().set_accessed(atime).set_modified(mtime);
                    let size = entry.metadata().unwrap().len();
                    let is_dir = entry.metadata().unwrap().is_dir();
                    tree_creator::Item {
                        path: path_rel.to_str().unwrap().to_string(),
                        times,
                        atime,
                        mtime,
                        size,
                        is_dir,
                    }
                })
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        })
    }
}
