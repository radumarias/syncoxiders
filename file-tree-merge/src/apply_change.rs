use crate::change_tree::Change;
use std::fs;
use std::fs::File;
use std::path::Path;

use crate::change_tree_merge::{Changes, Items};
use crate::crc_eq;
use anyhow::Result;
use colored::*;

pub fn apply(
    changes: &Changes,
    items_src: &Items,
    items_dst: &Items,
    src_mnt: &Path,
    dst_mnt: &Path,
    dry_run: bool,
) -> Result<()> {
    for (change, path) in changes {
        let dst = dst_mnt.join(&path);
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
                let src_item = items_src.get(path).unwrap();
                if let Some(dst_item) = items_dst.get(path) {
                    if src_item.eq(dst_item) {
                        add = false;
                    }
                }
                if add {
                    fs::create_dir_all(dst.parent().unwrap())?;
                    fs::copy(src_mnt.join(&path), dst.clone())?;
                    File::set_times(&File::open(dst.clone())?, src_item.times)?;
                    File::open(dst.clone())?.sync_all()?;
                    File::open(dst.parent().unwrap())?.sync_all()?;
                    if !crc_eq(&src_mnt.join(&path), &dst.clone())? {
                        println!("{}", "   checksum failed after copy, aborting".red().bold());
                        anyhow::bail!("Checksum failed for `{path}` after copy");
                    }
                } else {
                    println!(
                        "{}",
                        "   skip, already present in dst with same hash".yellow()
                    );
                }
            }
            Change::Delete => {
                println!("{} '{}'", change.to_string().red(), path.red().bold());
                if dry_run {
                    continue;
                }
                if dst.exists() {
                    fs::remove_file(dst.clone())?;
                    File::open(dst.parent().unwrap())?.sync_all()?;
                } else {
                    println!("{}", "  skip, not present in dst".yellow());
                }
            }
            Change::Rename(old_path) => {
                println!("{} '{}'", change.to_string().magenta(), path.magenta());
                if dry_run {
                    continue;
                }
                let src_item = items_src.get(path).unwrap();
                // todo: compare if old file hash in src is same as old file hash in dst
                if dst_mnt.join(&old_path).exists() {
                    fs::create_dir_all(dst.parent().unwrap())?;
                    fs::rename(dst_mnt.join(&old_path), dst.clone())?;
                    File::set_times(&File::open(dst.clone())?, src_item.times)?;
                    File::open(dst.clone())?.sync_all()?;
                    File::open(dst.parent().unwrap())?.sync_all()?;
                } else {
                    println!("{}", format!("  cannot R '{old_path}' -> '{path}', old file not present in dst. Will copy instead from src to new destination").yellow());
                    fs::create_dir_all(dst_mnt.join(path).parent().unwrap())?;
                    fs::copy(src_mnt.join(&path), dst.clone())?;
                    File::set_times(&File::open(dst.clone())?, src_item.times)?;
                    File::open(dst.clone())?.sync_all()?;
                    File::open(dst.parent().unwrap())?.sync_all()?;
                }
                if !crc_eq(&src_mnt.join(&path), &dst.clone())? {
                    println!("{}", "   checksum failed after copy, aborting".red().bold());
                    anyhow::bail!("Checksum failed for `{path}` after copy");
                }
            }
            Change::Copy(old_path) => {
                println!("{} '{}'", change.to_string().blue(), path.blue());
                if dry_run {
                    continue;
                }
                let src_item = items_src.get(path).unwrap();
                // todo: compare if old file hash in src is same as old file hash in dst
                if dst_mnt.join(&old_path).exists() {
                    fs::create_dir_all(dst.clone().parent().unwrap())?;
                    fs::copy(dst_mnt.join(&old_path), dst.clone())?;
                    File::set_times(&File::open(dst.clone())?, src_item.times)?;
                    File::open(dst.clone())?.sync_all()?;
                    File::open(dst.parent().unwrap())?.sync_all()?;
                } else {
                    println!("{}", format!("  cannot C '{old_path}' -> '{path}', old file not present in dst. Will copy instead from src to new destination").yellow());
                    fs::create_dir_all(dst.parent().unwrap())?;
                    fs::copy(src_mnt.join(&path), dst.clone())?;
                }
                if !crc_eq(&src_mnt.join(&path), &dst.clone())? {
                    println!("{}", "   checksum failed after copy, aborting".red().bold());
                    anyhow::bail!("Checksum failed for `{path}` after copy");
                }
            }
        }
    }

    Ok(())
}
