use std::io;

use crate::change_tree::{Change, ChangeTree};

pub fn merge(src: &mut ChangeTree, dst: &mut ChangeTree, delete: bool) -> io::Result<Vec<Change>> {
    let changes = vec![];
    if let Some(root) = src.tree.root() {
        println!("Changes in src:");
        root.traverse_pre_order().for_each(|node| {
            println!("{:?} {}", node.data().change, node.data().path);
        });
    } else {
        println!("No changes in src");
    }
    if let Some(root) = dst.tree.root() {
        println!("Changes in dst:");
        root.traverse_pre_order().for_each(|node| {
            println!("{:?} {}", node.data().change, node.data().path);
        });
    } else {
        println!("No changes in dst");
    }
    Ok(changes)
}
