use crate::change_tree::Change;
use std::fs::File;
use std::path::Path;
use std::{fs, io};

use crate::change_tree_merge::{Changes, HashKind, Items};
use crate::tree_creator::Item;
use crate::{crc_eq, file_hash};
use anyhow::Result;
use colored::*;

pub fn apply(
    changes: &Changes,
    items_path1: &Items,
    items_path2: &Items,
    path1_mnt: &Path,
    path2_mnt: &Path,
    dry_run: bool,
    checksum: bool,
    crc: bool,
) -> Result<()> {
    for (change, path) in changes {
        let path2 = path2_mnt.join(&path);
        match change.clone() {
            Change::Add | Change::Modify => {
                if matches!(change, Change::Add) {
                    println!("{} '{}'", change.to_string().green(), path.green());
                } else {
                    println!("{} '{}'", change.to_string().blue(), path.blue());
                }
                if dry_run {
                    continue;
                }
                // check if it's the same as in dst
                let mut add = true;
                let path1_item = items_path1.get(path).unwrap();
                if let Some(dst_item) = items_path2.get(path) {
                    if items_content_eq(&path1_mnt, path1_item, &path2_mnt, dst_item, checksum)? {
                        add = false;
                    }
                }
                if add {
                    fs::create_dir_all(path2.parent().unwrap())?;
                    fs::copy(path1_mnt.join(&path), path2.clone())?;
                    File::set_times(&File::open(path2.clone())?, path1_item.times)?;
                    File::open(path2.clone())?.sync_all()?;
                    File::open(path2.parent().unwrap())?.sync_all()?;
                    if crc && !crc_eq(&path1_mnt.join(&path), &path2.clone())? {
                        println!(
                            "{}",
                            "   CRC check failed after transfer, aborting".red().bold()
                        );
                        anyhow::bail!("CRC check failed for `{path}` after transfer");
                    }
                } else {
                    println!(
                        "{}",
                        "   skip, already present in path2 with the same content".yellow()
                    );
                }
            }
            Change::Delete => {
                println!("{} '{}'", change.to_string().red(), path.red().bold());
                if dry_run {
                    continue;
                }
                if path2.exists() {
                    fs::remove_file(path2.clone())?;
                    File::open(path2.parent().unwrap())?.sync_all()?;
                } else {
                    println!("{}", "  skip, not present in path2".yellow());
                }
            }
            Change::Rename(old_path) => {
                println!("{} '{}'", change.to_string().magenta(), path.magenta());
                if dry_run {
                    continue;
                }
                let path1_item = items_path1.get(path).unwrap();
                // todo: compare if old file hash in src is same as old file hash in dst
                if path2_mnt.join(&old_path).exists() {
                    fs::create_dir_all(path2.parent().unwrap())?;
                    fs::rename(path2_mnt.join(&old_path), path2.clone())?;
                    File::set_times(&File::open(path2.clone())?, path1_item.times)?;
                    File::open(path2.clone())?.sync_all()?;
                    File::open(path2.parent().unwrap())?.sync_all()?;
                } else {
                    println!("{}", format!("  cannot R '{old_path}' -> '{path}', old file not present in path2. Will copy instead from path1 to the new destination").yellow());
                    fs::create_dir_all(path2_mnt.join(path).parent().unwrap())?;
                    fs::copy(path1_mnt.join(&path), path2.clone())?;
                    File::set_times(&File::open(path2.clone())?, path1_item.times)?;
                    File::open(path2.clone())?.sync_all()?;
                    File::open(path2.parent().unwrap())?.sync_all()?;
                }
                if crc && !crc_eq(&path1_mnt.join(&path), &path2.clone())? {
                    println!(
                        "{}",
                        "   CRC check failed after transfer, aborting".red().bold()
                    );
                    anyhow::bail!("CRC check failed for `{path}` after transfer");
                }
            }
            Change::Copy(old_path) => {
                println!("{} '{}'", change.to_string().blue(), path.blue());
                if dry_run {
                    continue;
                }
                let path1_item = items_path1.get(path).unwrap();
                // todo: compare if old file hash in src is same as old file hash in dst
                if path2_mnt.join(&old_path).exists() {
                    fs::create_dir_all(path2.clone().parent().unwrap())?;
                    fs::copy(path2_mnt.join(&old_path), path2.clone())?;
                    File::set_times(&File::open(path2.clone())?, path1_item.times)?;
                    File::open(path2.clone())?.sync_all()?;
                    File::open(path2.parent().unwrap())?.sync_all()?;
                } else {
                    println!("{}", format!("  cannot C '{old_path}' -> '{path}', old file not present in path2. Will copy instead from path1 to the new destination").yellow());
                    fs::create_dir_all(path2.parent().unwrap())?;
                    fs::copy(path1_mnt.join(&path), path2.clone())?;
                }
                if crc && !crc_eq(&path1_mnt.join(&path), &path2.clone())? {
                    println!(
                        "{}",
                        "   CRC check failed after transfer, aborting".red().bold()
                    );
                    anyhow::bail!("CRC check failed for `{path}` after transfer");
                }
            }
        }
    }

    Ok(())
}

fn items_content_eq(
    path1_mnt: &&Path,
    a: &Item,
    path2_mnt: &&Path,
    b: &Item,
    checksum: bool,
) -> io::Result<bool> {
    if a.size == b.size && a.mtime == b.mtime {
        if checksum {
            let hash1 = file_hash(&path1_mnt.join(&a.path), HashKind::Md5)?;
            let hash2 = file_hash(&path2_mnt.join(&b.path), HashKind::Md5)?;
            Ok(hash1.eq(&hash2))
        } else {
            Ok(true)
        }
    } else {
        Ok(false)
    }
}
