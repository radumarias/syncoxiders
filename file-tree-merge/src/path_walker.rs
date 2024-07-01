use crate::tree_creator;
use crate::tree_creator::HashKind;
use std::fs::{File, FileTimes};
use std::io;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

pub struct PathWalker {
    it: Box<dyn Iterator<Item = walkdir::Result<DirEntry>>>,
    src: PathBuf,
}

impl PathWalker {
    pub fn new(src: &Path) -> Self {
        Self {
            it: Box::new(WalkDir::new(src).follow_links(true).into_iter()),
            src: src.to_path_buf(),
        }
    }
}

impl Iterator for PathWalker {
    type Item = io::Result<(String, tree_creator::Item)>;

    fn next(&mut self) -> Option<Self::Item> {
        self.it.next().map(|entry| {
            entry
                .map(|entry| {
                    let path = entry.path();
                    let path_rel = path.strip_prefix(&self.src).unwrap();
                    let name = path.file_name().unwrap().to_str().unwrap().to_string();
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
                    (
                        path_rel.to_str().unwrap().to_string(),
                        tree_creator::Item {
                            name,
                            times,
                            size,
                            is_dir,
                            hash,
                        },
                    )
                })
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        })
    }
}
