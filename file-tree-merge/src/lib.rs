use anyhow::Result;
use change_tree_merge::HashKind;
use crc32fast::Hasher;
use std::fs::File;
use std::io;
use std::io::{BufReader, Read};
use std::path::Path;
use std::process::Command;

pub mod apply_change;
pub mod change_tree;
pub mod change_tree_merge;
pub mod path_walker;
pub mod tree_creator;

pub const MNT_DIR: &str = "mnt";
pub const REPO_DIR: &str = "repo";
pub const TREE_DIR: &str = "tree";

pub trait IterRef {
    /// The type of the elements being iterated over.
    type Item;

    /// Which kind of iterator are we turning this into?
    type Iter: Iterator<Item = Self::Item>;

    /// Creates an iterator from a value.
    fn iter(&self) -> Self::Iter;
}

pub fn crc_eq(path1: &Path, path2: &Path) -> Result<bool> {
    let src_crc = crc(path1)?;
    let dst_crc = crc(path2)?;
    Ok(src_crc == dst_crc)
}

pub fn crc(path: &Path) -> Result<u32> {
    // Open the file
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    // Create a CRC32 hasher
    let mut hasher = Hasher::new();
    let mut buffer = [0u8; 4096];

    // Read the file in chunks and update the hasher
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    // Finalize the hasher to get the checksum
    let checksum = hasher.finalize();

    Ok(checksum)
}

pub fn file_hash(path: &Path, kind: HashKind) -> io::Result<String> {
    match kind {
        HashKind::Md5 => Ok(chksum_md5::chksum(File::open(path)?)
            .unwrap()
            .to_hex_lowercase()),
        HashKind::Sha1 => {
            unimplemented!("sha1")
        }
        HashKind::Sha256 => {
            unimplemented!("sha256")
        }
    }
}

pub fn git_status(repo: &Path) -> Result<String> {
    command("git", vec!["status", "-s"], repo)
}

pub fn git_add(repo: &Path, path_specs: &str) -> Result<()> {
    command("git", vec!["add", path_specs], repo)?;
    Ok(())
}

pub fn command(command: &str, args: Vec<&str>, dir: &Path) -> Result<String> {
    let mut c = Command::new(command);
    let c = c.current_dir(dir);
    let c = args.iter().fold(c, |c, arg| c.arg(arg));
    let output = c.output().expect("Failed to execute command");
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        anyhow::bail!(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

pub fn git_restore_staged(repo: &Path, path_specs: &str) -> Result<()> {
    command(
        "git",
        vec!["rm", "--cached", "--ignore-unmatch", "-r", path_specs],
        repo,
    )?;
    let _ = command("git", vec!["restore", "--staged", path_specs], repo);
    Ok(())
}

pub fn git_commit(repo: &Path) -> Result<()> {
    // repo.commit(
    //     Some("HEAD"),
    //     &repo.signature().unwrap(),
    //     &repo.signature().unwrap(),
    //     if new_repo { "Initial commit" } else { "Update" },
    //     &repo
    //         .find_tree(repo.index().unwrap().write_tree().unwrap())
    //         .unwrap(),
    //     &[&repo.head().unwrap().peel_to_commit().unwrap()],
    // )
    // .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    command(
        "git",
        vec!["commit", "--allow-empty", "-m", "\"changes\""],
        repo,
    )?;
    Ok(())
}

pub fn git_delete_history(repo: &Path) -> Result<()> {
    command("git", vec!["checkout", "--orphan", "latest_branch"], repo)?;
    git_add(repo, "-A")?;
    git_commit(repo)?;
    command("git", vec!["branch", "-D", "master"], repo)?;
    command("git", vec!["branch", "-m", "master"], repo)?;
    Ok(())
}
