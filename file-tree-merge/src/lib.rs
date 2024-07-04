pub mod change_tree;
pub mod change_tree_merge;
pub mod path_walker;
pub mod tree_creator;

pub const MNT_DIR: &str = "mnt";
pub const REPO_DIR: &str = "repo";
pub const TREE_DIR: &str = "tree";

pub trait IterRef {
    /// The type of the elements being iterated over.
    type Item;

    /// Which kind of iterator are we turning this into?
    type Iter: Iterator<Item = Self::Item>;

    /// Creates an iterator from a value.
    fn iter(&self) -> Self::Iter;
}
