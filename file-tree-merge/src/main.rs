use std::env;
use std::path::PathBuf;

use anyhow::Result;

use file_tree_merge::path_walker::PathWalker;
use file_tree_merge::tree_creator::TreeCreator;
use file_tree_merge::{change_tree, change_tree_merge, MNT_DIR, REPO_DIR, TREE_DIR};

fn main() -> Result<()> {
    let src = PathBuf::from(env::args().nth(1).expect("No source directory provided"));
    let dst = PathBuf::from(
        env::args()
            .nth(2)
            .expect("No destination directory provided"),
    );
    let path_tree_iterator1 = PathWalker::new(&src.join(MNT_DIR));
    let tree_creator1 = TreeCreator::new(path_tree_iterator1);
    let items1 = tree_creator1.create(&src.join(format!("{}/{}", REPO_DIR, TREE_DIR)))?;
    let mut change_tree1 = change_tree::build(items1, &src.join(REPO_DIR))?;

    let path_tree_iterator2 = PathWalker::new(&dst.join(MNT_DIR));
    let tree_creator2 = TreeCreator::new(path_tree_iterator2);
    let items2 = tree_creator2.create(&dst.join(format!("{}/{}", REPO_DIR, TREE_DIR)))?;
    let mut change_tree2 = change_tree::build(items2, &dst.join(REPO_DIR))?;

    change_tree_merge::merge(&mut change_tree1, &mut change_tree2, true)?;

    Ok(())
}
