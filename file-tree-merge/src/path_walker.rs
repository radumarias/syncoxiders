use anyhow::Result;
use colored::Colorize;
use std::fs::FileTimes;
use std::io;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

use crate::{tree_creator, IterRef};

pub struct PathWalker {
    path: PathBuf,
}

impl PathWalker {
    pub fn new(path: &Path) -> Result<Self> {
        if !path.exists() {
            println!(
                "{}",
                format!("Path '{}' does not exist", path.display()).red()
            );
            anyhow::bail!("Path '{}' does not exist", path.display())
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

pub struct Iter {
    it: Box<dyn Iterator<Item = walkdir::Result<DirEntry>>>,
    path: PathBuf,
    ctr: u64,
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
            ctr: 0,
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
                    if self.ctr % 100 == 0 {
                        println!(
                            "{}",
                            format!("Checking '{}'", path.to_string_lossy()).cyan()
                        );
                    }
                    self.ctr += 1;
                    let path_rel = path.strip_prefix(&self.path).unwrap();
                    let atime = entry.metadata().unwrap().accessed().unwrap();
                    let mtime = entry.metadata().unwrap().modified().unwrap();
                    let times = FileTimes::new().set_accessed(atime).set_modified(mtime);
                    let size = entry.metadata().unwrap().len();
                    let is_dir = entry.metadata().unwrap().is_dir();
                    tree_creator::Item {
                        path: path_rel.to_string_lossy().to_string(),
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
