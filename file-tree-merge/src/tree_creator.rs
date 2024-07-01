use std::fs::{File, FileTimes};
use std::io::Write;
use std::path::Path;
use std::{fs, io};

pub(crate) const PATH_SEPARATOR: &str = "／";

pub enum HashKind {
    Md5,
    Sha1,
    Sha256,
}

pub struct Item {
    pub name: String,
    pub times: FileTimes,
    pub size: u64,
    pub is_dir: bool,
    pub hash: Option<(HashKind, String)>,
}

pub fn create<Src: Iterator<Item = io::Result<(String, Item)>>>(
    src: &mut Src,
    repo: &Path,
) -> io::Result<()> {
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
    for item in src {
        if first {
            // skip root
            first = false;
            continue;
        }
        let (mut path_rel, item) = item?;
        path_rel = path_rel.replace('/', PATH_SEPARATOR);
        let path = dst.join(path_rel);
        println!("Creating file: {:?}", path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&path)?;
        if !item.is_dir {
            file.write_all(&item.size.to_le_bytes())?;
            if let Some((_, hash)) = item.hash {
                file.write_all(hash.as_bytes())?;
            }
            file.flush()?;
        }
        file.sync_all()?;
        File::set_times(&file, item.times)?;
        File::open(&path)?.sync_all()?;
    }
    Ok(())
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
