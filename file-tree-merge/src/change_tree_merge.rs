use std::collections::BTreeMap;

use crate::change_tree::{Change, ChangeTree};
use crate::tree_creator::Item;
use anyhow::Result;

pub enum MergeStrategy {
    Copy,
    Move,
    OneWay,
    TwoWay,
}

pub type Changes = Vec<(Change, String)>;
pub type Items = BTreeMap<String, Item>;
pub type SrcChanges = (Changes, Items);
pub type DstChanges = (Changes, Items);

pub fn merge(
    changes_src: (ChangeTree, BTreeMap<String, Item>),
    changes_dst: (ChangeTree, BTreeMap<String, Item>),
    strategy: MergeStrategy,
) -> Result<(SrcChanges, DstChanges)> {
    match strategy {
        MergeStrategy::Copy => unimplemented!(),
        MergeStrategy::Move => unimplemented!(),
        MergeStrategy::OneWay => Ok((do_one_way(changes_src)?, (vec![], changes_dst.1))),
        MergeStrategy::TwoWay => unimplemented!(),
    }
}

fn do_one_way(changes_src: (ChangeTree, BTreeMap<String, Item>)) -> Result<SrcChanges> {
    let mut changes = vec![];
    let (changes_src, items_src) = changes_src;
    if changes_src.tree.root().unwrap().first_child().is_some() {
        let root = changes_src.tree.root().unwrap();
        root.traverse_pre_order().for_each(|node| {
            if node.node_id() == changes_src.tree.root_id().unwrap() {
                return;
            }
            // println!(
            //     "{:?} {:?}",
            //     node.data().change.as_ref().unwrap(),
            //     node.data().path
            // );

            let change = node.data().change.as_ref().unwrap();
            let path = node.data().path.clone();
            changes.push((change.clone(), path));
        });
    }

    Ok((changes, items_src))
}
