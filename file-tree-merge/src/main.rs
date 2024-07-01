use std::env;
use std::path::PathBuf;

use anyhow::Result;
use file_tree_merge::path_walker::PathWalker;
use file_tree_merge::{change_tree, change_tree_merge, tree_creator, MNT_DIR, REPO_DIR, TREE_DIR};

fn main() -> Result<()> {
    let src = PathBuf::from(env::args().nth(1).expect("No source directory provided"));
    let dst = PathBuf::from(
        env::args()
            .nth(2)
            .expect("No destination directory provided"),
    );
    let mut path_tree_iterator1 = PathWalker::new(&src.join(MNT_DIR));
    tree_creator::create(
        &mut path_tree_iterator1,
        &src.join(format!("{}/{}", REPO_DIR, TREE_DIR)),
    )?;
    let mut change_tree1 = change_tree::build(&src.join(REPO_DIR))?;

    let mut path_tree_iterator2 = PathWalker::new(&dst.join(MNT_DIR));
    tree_creator::create(
        &mut path_tree_iterator2,
        &dst.join(format!("{}/{}", REPO_DIR, TREE_DIR)),
    )?;
    let mut change_tree2 = change_tree::build(&dst.join(REPO_DIR))?;

    change_tree_merge::merge(&mut change_tree1, &mut change_tree2, true)?;

    Ok(())
}
