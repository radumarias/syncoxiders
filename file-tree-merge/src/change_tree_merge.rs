use std::collections::BTreeMap;

use crate::change_tree::{Change, PathChanges};
use crate::tree_creator::Item;
use anyhow::{anyhow, Result};

pub enum MergeStrategy {
    Copy,
    Move,
    OneWay,
    TwoWay,
}

pub type Changes = Vec<(Change, String)>;
pub type SrcItems = (usize, BTreeMap<String, Item>);
pub type DstItems = BTreeMap<String, Item>;
pub type MergedChanges = (Changes, SrcItems, DstItems);

pub fn merge(
    path_changes: Vec<PathChanges>,
    strategy: MergeStrategy,
) -> Result<Vec<MergedChanges>> {
    if path_changes.len() < 2 {
        return Err(anyhow!("Min 2 path_changes required for merge"));
    }
    match strategy {
        MergeStrategy::Copy => unimplemented!(),
        MergeStrategy::Move => unimplemented!(),
        MergeStrategy::OneWay => do_one_way(path_changes),
        MergeStrategy::TwoWay => unimplemented!(),
    }
}

fn do_one_way(mut path_changes: Vec<PathChanges>) -> Result<Vec<MergedChanges>> {
    let mut merged_changes = vec![];
    let (changes_path1, items_path1) = path_changes.remove(0);
    merged_changes.push((
        Default::default(),
        (0, Default::default()),
        Default::default(),
    ));
    let mut changes = vec![];
    if changes_path1.tree.root().unwrap().first_child().is_some() {
        let root = changes_path1.tree.root().unwrap();
        root.traverse_pre_order().for_each(|node| {
            if node.node_id() == changes_path1.tree.root_id().unwrap() {
                return;
            }
            let change = node.data().change.as_ref().unwrap();
            let path = node.data().path.clone();
            // todo: merge changes similar to what we do on apply
            changes.push((change.clone(), path));
        });
    }
    while !path_changes.is_empty() {
        merged_changes.push((
            changes.clone(),
            // todo: use Arc
            (0, items_path1.clone()),
            path_changes.remove(0).1,
        ));
    }

    Ok(merged_changes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashKind {
    Md5,
    Sha1,
    Sha256,
}
