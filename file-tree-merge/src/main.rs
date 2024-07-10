use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{io, thread};
use tap::Pipe;

use file_tree_merge::change_tree::ChangeTree;
use file_tree_merge::change_tree_merge::MergeStrategy;
use file_tree_merge::path_walker::PathWalker;
use file_tree_merge::tree_creator::{Item, TreeCreator};
use file_tree_merge::{
    apply_change, change_tree, change_tree_merge, git_add, git_commit, git_delete_history, TREE_DIR,
};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(
        short,
        long,
        short = 'a',
        help = "First directory where actual files that needs to be synced are located."
    )]
    path1: PathBuf,

    #[arg(
        short,
        long,
        short = 'b',
        help = "Second directory where actual files that needs to be synced are located."
    )]
    path2: PathBuf,

    #[arg(
        short,
        long,
        short = 'r',
        help = "A directory where we'll keep a git repo to detect changes. Should persist between runs. MUST NOT BE INSIDE ANY OF path1 or path2 DIRECTORIES"
    )]
    repo: PathBuf,

    #[arg(
        short,
        long,
        default_value_t = false,
        help = "This simulates the sync. Will not actually create or change any of the files in path1 or path2, will just print the operations that would have normally be applied to both ends"
    )]
    dry_run: bool,

    #[arg(
        short,
        long,
        short = 't',
        default_value_t = false,
        help = "If specified it will calculate MD5 hash for files when comparing file in path1 with the file in path2 when applying Add and Modify operations. It will be considerably slower when activated"
    )]
    checksum: bool,

    #[arg(
        short,
        long,
        short = 'x',
        default_value_t = false,
        help = "If specified it will skip CRC check after file was transferred. Without this it compares the CRC of the file in path1 before transfer with the CRC of the file in path2 after transferred. This ensures the transfer was successful. Checking CRC is highly recommend if any of path1 or path1 are accessed over the network"
    )]
    no_crc: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.no_crc {
        println!("{}", "CRC is disabled for checking file after transfer. Make sure you want it like that, if not, remove --no-crc from args !".red().bold());
        thread::sleep(std::time::Duration::from_secs(10));
    }

    if args.checksum {
        println!(
            "{}",
            "Checksum mode enabled, it could be quite slow!"
                .yellow()
                .bold()
        );
    }

    if args.dry_run {
        println!("{}", "Dry-run mode enabled, it will not touch create, modify ot delete any files, will just print the changes!".yellow().bold());
    }

    println!("{}", "Building changes trees...".cyan());
    let (changes_tree1, errors1) =
        changes_tree(PathWalker::new(&args.path1), &args.repo.join("1"))?;
    let (changes_tree2, errors2) =
        changes_tree(PathWalker::new(&args.path2), &args.repo.join("2"))?;

    println!("{}", "Merging changes trees...".cyan());
    change_tree_merge::merge(changes_tree1, changes_tree2, MergeStrategy::OneWay)?.pipe(|x| {
        if x.0 .0.is_empty() && x.1 .0.is_empty() {
            println!("{}", "No changes".green());
            return Ok::<(), anyhow::Error>(());
        }
        if !args.dry_run {
            println!("{}", "Applying changes...".cyan());
        }
        println!("{}", "src -> dst...".cyan());
        let (changes_src, items_path1) = x.0;
        let (_changes_dst, items_path2) = x.1;
        apply_change::apply(
            changes_src,
            items_path1,
            items_path2,
            &args.path1,
            &args.path2,
            &args.repo.join("1"),
            &args.repo.join("2"),
            args.dry_run,
            args.checksum,
            !args.no_crc,
        )?;
        println!("{}", "Cleaning up repo...".cyan());
        git_delete_history(&args.repo.join("1"))?;
        // todo: dst -> src
        git_add(&args.repo.join("1"), ".")?;
        git_commit(&args.repo.join("2"))?;
        git_delete_history(&args.repo.join("2"))?;
        Ok(())
    })?;

    if !errors1.is_empty() {
        println!("{}", "Errors reading from path1:".red());
        for e in errors1 {
            println!("{}", e.to_string().red().bold());
        }
    }

    if !errors2.is_empty() {
        println!("{}", "Errors reading from path2:".red());
        for e in errors2 {
            println!("{}", e.to_string().red().bold());
        }
    }

    Ok(())
}

fn changes_tree(
    iter: PathWalker,
    repo: &Path,
) -> Result<((ChangeTree, BTreeMap<String, Item>), Vec<io::Error>)> {
    iter.pipe(TreeCreator::new)
        .pipe(|x| x.create(&repo.join(TREE_DIR)))?
        .pipe(|x| Ok((change_tree::build(x.0, repo)?, x.1)))
}
