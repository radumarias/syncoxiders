use colored::Colorize;
use std::fs::{File, FileTimes};
use std::io;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

use crate::tree_creator::HashKind;
use crate::{tree_creator, IterRef};

pub struct PathWalker {
    src: PathBuf,
    checksum: bool,
}

impl PathWalker {
    pub fn new(src: &Path, checksum: bool) -> Self {
        Self {
            src: src.to_path_buf(),
            checksum,
        }
    }
}

pub struct Iter {
    it: Box<dyn Iterator<Item = walkdir::Result<DirEntry>>>,
    src: PathBuf,
    checksum: bool,
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
            checksum: self.checksum,
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
                        format!("Prepare file '{}'", path.to_str().unwrap()).cyan()
                    );
                    let path_rel = path.strip_prefix(&self.src).unwrap();
                    // println!("prepare {:?}", path_rel);
                    let atime = entry.metadata().unwrap().accessed().unwrap();
                    let mtime = entry.metadata().unwrap().modified().unwrap();
                    let times = FileTimes::new().set_accessed(atime).set_modified(mtime);
                    let size = entry.metadata().unwrap().len();
                    let is_dir = entry.metadata().unwrap().is_dir();
                    let hash = if !path.is_dir() {
                        if self.checksum {
                            Some((
                                HashKind::Md5,
                                chksum_md5::chksum(File::open(path).unwrap())
                                    .unwrap()
                                    .to_hex_lowercase(),
                            ))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    tree_creator::Item {
                        path: path_rel.to_str().unwrap().to_string(),
                        times,
                        atime,
                        mtime,
                        size,
                        is_dir,
                        hash,
                    }
                })
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        })
    }
}
