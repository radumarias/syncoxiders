use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use tap::Pipe;

use file_tree_merge::change_tree::ChangeTree;
use file_tree_merge::change_tree_merge::MergeStrategy;
use file_tree_merge::path_walker::PathWalker;
use file_tree_merge::tree_creator::{Item, TreeCreator};
use file_tree_merge::{apply_change, change_tree, change_tree_merge, TREE_DIR};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, short = 'a')]
    src_mnt: PathBuf,

    #[arg(short, long, short = 'x')]
    src_repo: PathBuf,

    #[arg(short, long, short = 'b')]
    dst_mnt: PathBuf,

    #[arg(short, long, short = 'y')]
    dst_repo: PathBuf,

    #[arg(short, long, default_value_t = false)]
    dry_run: bool,

    #[arg(short, long, default_value_t = false)]
    checksum: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.checksum {
        println!(
            "{}",
            "Checksum mode enabled, it could be very slow!"
                .yellow()
                .bold()
        );
    }

    if args.dry_run {
        println!("{}", "Dry-run mode enabled, it will not touch any files on dst, will just print the changes!".yellow().bold());
    }

    println!("{}", "Build changes tree...".cyan());
    let (changes_tree1, errors1) = changes_tree(
        PathWalker::new(&args.src_mnt, args.checksum),
        &args.src_repo,
    )?;
    let (changes_tree2, errors2) = changes_tree(
        PathWalker::new(&args.dst_mnt, args.checksum),
        &args.dst_repo,
    )?;

    println!("{}", "Merge changes trees...".cyan());
    change_tree_merge::merge(changes_tree1, changes_tree2, MergeStrategy::OneWay)?.pipe(|x| {
        if x.0 .0.is_empty() && x.1 .0.is_empty() {
            println!("{}", "No changes to apply!".green());
            return Ok(());
        }
        if !args.dry_run {
            println!("{}", "Apply changes...".cyan());
        }
        println!("{}", "src -> dst...".cyan());
        let (changes_src, items_src) = x.0;
        let (_changes_dst, items_dst) = x.1;
        apply_change::apply(
            &changes_src,
            &items_src,
            &items_dst,
            &args.src_mnt,
            &args.dst_mnt,
            args.dry_run,
        )
        // todo: dst -> src
    })?;

    if !errors1.is_empty() {
        println!("{}", "Errors reading from src:".red());
        for e in errors1 {
            println!("{}", e.to_string().red().bold());
        }
    }

    if !errors2.is_empty() {
        println!("{}", "Errors reading from dst:".red());
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
