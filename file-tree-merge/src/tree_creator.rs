use std::fs::{File, FileTimes};
use std::io::Write;
use std::path::Path;
use std::{fs, io};

use crate::IterRef;

pub(crate) const PATH_SEPARATOR: &str = "／";

pub enum HashKind {
    Md5,
    Sha1,
    Sha256,
}

pub struct Item {
    pub path: String,
    pub times: FileTimes,
    pub size: u64,
    pub is_dir: bool,
    pub hash: Option<(HashKind, String)>,
}

pub struct TreeCreator<I: Iterator<Item = io::Result<Item>>, Iter: IterRef<Iter = I>> {
    iter: Iter,
}

impl<I: Iterator<Item = io::Result<Item>>, Iter: IterRef<Iter = I>> TreeCreator<I, Iter> {
    pub fn new(iter: Iter) -> Self {
        Self { iter }
    }

    pub fn create(&self, repo: &Path) -> io::Result<Vec<Item>> {
        let dst = repo.to_path_buf();
        if dst.exists() {
            if !dst.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Destination must be a directory",
                ));
            }
            remove_all_from_dir(&dst)?;
        } else {
            fs::create_dir_all(&dst)?;
        }
        println!("Creating tree in: {:?}", dst);
        let mut first = true;
        let mut items = vec![];
        for item in self.iter.iter() {
            if first {
                // skip root
                first = false;
                continue;
            }
            let item = item?;
            let path_rel = item.path.replace('/', PATH_SEPARATOR);
            let path = dst.join(path_rel);
            println!("Creating file: {:?}", path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&path)?;
            if !item.is_dir {
                file.write_all(&item.size.to_le_bytes())?;
                if let Some((_, ref hash)) = item.hash {
                    file.write_all(hash.as_bytes())?;
                }
                file.flush()?;
            }
            file.sync_all()?;
            File::set_times(&file, item.times)?;
            File::open(&path)?.sync_all()?;
            items.push(item);
        }
        Ok(items)
    }
}

fn remove_all_from_dir(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}
