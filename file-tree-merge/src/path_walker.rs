use std::fs::{File, FileTimes};
use std::io;
use std::path::{Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

use crate::tree_creator::HashKind;
use crate::{tree_creator, IterRef};

pub struct PathWalker {
    src: PathBuf,
}

impl PathWalker {
    pub fn new(src: &Path) -> Self {
        Self {
            src: src.to_path_buf(),
        }
    }
}

pub struct Iter {
    it: Box<dyn Iterator<Item = walkdir::Result<DirEntry>>>,
    src: PathBuf,
}

impl IterRef for PathWalker {
    type Item = io::Result<tree_creator::Item>;
    type Iter = Iter;

    fn iter(&self) -> Self::Iter {
        Iter {
            it: Box::new(
                WalkDir::new(self.src.clone())
                    .follow_links(true)
                    .into_iter(),
            ),
            src: self.src.clone(),
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
                    let path_rel = path.strip_prefix(&self.src).unwrap();
                    let times = FileTimes::new()
                        .set_accessed(entry.metadata().unwrap().accessed().unwrap())
                        .set_modified(entry.metadata().unwrap().modified().unwrap());
                    let size = entry.metadata().unwrap().len();
                    let is_dir = entry.metadata().unwrap().is_dir();
                    let hash = if !path.is_dir() {
                        Some((
                            HashKind::Md5,
                            chksum_md5::chksum(File::open(path).unwrap())
                                .unwrap()
                                .to_hex_lowercase(),
                        ))
                    } else {
                        None
                    };
                    tree_creator::Item {
                        path: path_rel.to_str().unwrap().to_string(),
                        times,
                        size,
                        is_dir,
                        hash,
                    }
                })
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        })
    }
}
