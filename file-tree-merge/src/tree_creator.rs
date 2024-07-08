use crate::IterRef;
use anyhow::Result;
use colored::Colorize;
use std::fmt::{Debug, Formatter};
use std::fs::{File, FileTimes};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

// pub(crate) const PATH_SEPARATOR: &str = "／";
pub(crate) const PATH_SEPARATOR: &str = "|";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashKind {
    Md5,
    Sha1,
    Sha256,
}

pub struct Item {
    pub path: String,
    pub times: FileTimes,
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub size: u64,
    pub is_dir: bool,
    pub hash: Option<(HashKind, String)>,
}

impl Clone for Item {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            times: self.times.clone(),
            mtime: self.atime.clone(),
            atime: self.mtime.clone(),
            size: self.size,
            is_dir: self.is_dir,
            hash: self.hash.as_ref().map(|(k, v)| (k.clone(), v.clone())),
        }
    }
}

impl Debug for Item {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Item")
            .field("path", &self.path)
            .field("times", &self.times)
            .field("atime", &self.atime)
            .field("mtime", &self.mtime)
            .field("size", &self.size)
            .field("is_dir", &self.is_dir)
            .field("hash", &self.hash)
            .finish()
    }
}

pub struct TreeCreator<I: Iterator<Item = io::Result<Item>>, Iter: IterRef<Iter = I>> {
    iter: Iter,
}

impl<I: Iterator<Item = io::Result<Item>>, Iter: IterRef<Iter = I>> TreeCreator<I, Iter> {
    pub fn new(iter: Iter) -> Self {
        Self { iter }
    }

    pub fn create(&self, tree_dir: &Path) -> Result<(Vec<Item>, Vec<io::Error>)> {
        let dst = tree_dir.to_path_buf();
        if dst.exists() {
            if !dst.is_dir() {
                anyhow::bail!("Destination is not a directory");
            }
            remove_all_from_dir(&dst)?;
        } else {
            fs::create_dir_all(&dst)?;
        }
        // println!("Creating tree in: {:?}", dst);
        let mut first = true;
        let mut items = vec![];
        let mut errors = vec![];
        for item in self.iter.iter() {
            if first {
                // skip root
                first = false;
                continue;
            }
            if let Err(err) = item {
                println!("{}", format!("Error reading file: {:?}", err).red().bold());
                errors.push(err);
                continue;
            }
            let item = item?;
            // let path_rel = item.path.replace('/', PATH_SEPARATOR);
            // let path = dst.join(path_rel);
            let path = dst.join(item.path.clone());
            if item.is_dir {
                // println!("Creating dir: {:?}", path);
                fs::create_dir_all(&path)?;
            } else {
                // println!("Creating file: {:?}", path);
                let mut file = File::create(&path)?;
                Path::new(&path)
                    .parent()
                    .map_or(Ok(()), fs::create_dir_all)?;
                file.write_all(&get_time_bytes(&item.mtime))?;
                file.write_all(&item.size.to_le_bytes())?;
                if let Some((_, ref hash)) = item.hash {
                    file.write_all(hash.as_bytes())?;
                }
                file.flush()?;
                file.sync_all()?;
                File::set_times(&file, item.times)?;
            }
            File::open(&path)?.sync_all()?;
            items.push(item);
        }
        Ok((items, errors))
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

fn get_time_bytes(time: &SystemTime) -> Vec<u8> {
    // Convert to a duration since the Unix epoch
    let duration_since_epoch = time.duration_since(UNIX_EPOCH).unwrap();

    // Convert the duration to seconds and nanoseconds
    let secs = duration_since_epoch.as_secs();
    let nanos = duration_since_epoch.subsec_nanos();

    // Convert seconds and nanoseconds to byte arrays
    let secs_bytes = secs.to_be_bytes();
    let nanos_bytes = nanos.to_be_bytes();

    // Combine the byte arrays
    let mut bytes = [0u8; 12];
    bytes[..8].copy_from_slice(&secs_bytes);
    bytes[8..].copy_from_slice(&nanos_bytes);

    bytes.to_vec()
}
