use std::path::{Path, PathBuf};
use std::{io, thread};

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use tap::Pipe;

use file_tree_merge::change_tree::PathChanges;
use file_tree_merge::change_tree_merge::MergeStrategy;
use file_tree_merge::path_walker::PathWalker;
use file_tree_merge::tree_creator::TreeCreator;
use file_tree_merge::{
    apply_change, change_tree, change_tree_merge, git_add, git_commit, git_delete_history, TREE_DIR,
};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(
        short,
        long,
        short = 'r',
        help = "A directory where we'll keep a git repo to detect changes. Should persist between runs. MUST NOT BE ON ANY OF THE ENDPOINTS"
    )]
    repo: PathBuf,

    #[arg(
        short,
        long,
        default_value_t = false,
        help = "This simulates the sync. Will not apply any operations to the files, will just print the operations that would have normally be applied to endpoints"
    )]
    dry_run: bool,

    #[arg(
        short,
        long,
        short = 't',
        default_value_t = false,
        help = "If specified it will calculate MD5 hash for files when comparing file in path1 with the file in path2 when applying Add and Modify operations between endpoints. It will be considerably slower when activated"
    )]
    checksum: bool,

    #[arg(
        short,
        long,
        short = 'x',
        default_value_t = false,
        help = "If specified it will skip CRC check after file was transferred. Without this it compares the CRC of the file in path1 before transfer with the CRC of the file in path2 after transferred. This ensures the transfer was successful. Checking CRC is highly recommend if any of the endpoints are accessed over the network"
    )]
    no_crc: bool,

    #[arg(
        short,
        long,
        short = 'p',
        default_value_t = false,
        help = "Will print logs for each sync operation it applying"
    )]
    print_all_changes: bool,

    #[arg(help = "Endpoints where data to be synced resides")]
    inputs: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.no_crc {
        println!("{}", "CRC check after transfer is disabled. Make sure this is what you want because like that file integrity cannot be ensured. If you didn't intended this, remove --no-crc from args!".red().bold());
        thread::sleep(std::time::Duration::from_secs(10));
    }

    if args.checksum {
        println!(
            "{}",
            "Checksum mode enabled, it will be quite slow!"
                .yellow()
                .bold()
        );
    }

    if args.dry_run {
        println!("{}", "Dry-run mode enabled, it will not touch create, modify ot delete any files, will just print the changes!".yellow().bold());
    }

    if args.inputs.len() < 2 {
        println!(
            "{}",
            "You need to specify at least two paths to sync!"
                .red()
                .bold()
        );
        anyhow::bail!("You need to specify at least two paths to sync!");
    } else {
        std::fs::create_dir_all(&args.repo)?;
    }

    for path in &args.inputs {
        if !path.exists() {
            println!(
                "{}",
                format!("Path '{}' does not exist", path.display()).red()
            );
            anyhow::bail!("Path '{}' does not exist", path.display());
        }
    }

    println!("{}", "Building changes trees ...".cyan());
    let mut changes = vec![];
    let mut errors = vec![];
    for (idx, path) in args.inputs.iter().enumerate() {
        let (changes_tree, err) =
            changes_tree(PathWalker::new(path)?, &args.repo.join(idx.to_string()))?;
        changes.push(changes_tree);
        errors.push(err);
    }

    println!("{}", "Merging changes trees ...".cyan());
    change_tree_merge::merge(changes, MergeStrategy::OneWay)?.pipe(|x| {
        if x.iter().map(|x| x.0.len()).sum::<usize>() == 0 {
            println!("{}", "No changes".green());
            return Ok::<(), anyhow::Error>(());
        }
        if !args.dry_run {
            println!("{}", "Applying changes ...".cyan());
        }
        for (idx, path_change_to_apply) in x.into_iter().enumerate() {
            let src_repo_idx = path_change_to_apply.1 .0;
            println!(
                "{}",
                format!("-> '{}' ...", args.inputs[idx].to_string_lossy()).cyan()
            );
            if path_change_to_apply.0.is_empty() {
                println!("{}", "    no changes".green());
                continue;
            }
            apply_change::apply(
                path_change_to_apply,
                &args.inputs[src_repo_idx],
                &args.inputs[idx],
                &args.repo.join(src_repo_idx.to_string()),
                &args.repo.join(idx.to_string()),
                args.dry_run,
                args.checksum,
                !args.no_crc,
                args.print_all_changes,
            )?;
            println!("{}", "Cleaning up repo ...".cyan());
            git_add(&args.repo.join(idx.to_string()), ".")?;
            git_commit(&args.repo.join(idx.to_string()))?;
            git_delete_history(&args.repo.join(idx.to_string()))?;
        }
        // todo: remove when we group changes per src
        git_add(&args.repo.join("0"), ".")?;
        git_commit(&args.repo.join("0"))?;
        git_delete_history(&args.repo.join("0"))?;
        Ok(())
    })?;

    for (idx, err) in errors.iter().enumerate() {
        if !err.is_empty() {
            println!(
                "{}",
                format!("Errors reading from path{}:", idx).red().bold()
            );
            for e in err {
                println!("{}", e.to_string().red().bold());
            }
        }
    }

    Ok(())
}

fn changes_tree(iter: PathWalker, repo: &Path) -> Result<(PathChanges, Vec<io::Error>)> {
    iter.pipe(TreeCreator::new)
        .pipe(|x| x.create(&repo.join(TREE_DIR)))?
        .pipe(|x| Ok((change_tree::build(x.0, repo)?, x.1)))
}
