//! The Files pane's tree.
//!
//! rtorrent reports a flat list of paths; the pane shows them as the folders the
//! paths describe, with each folder's size and progress rolled up from its
//! children. Pure: the view decides how a node looks, this decides what the
//! nodes are.
//!
//! A port of `src/utils/filetree.ts`.

use std::collections::HashSet;

use rtorrent_core::types::{FileNode, Status};

/// One row of the tree: a folder, or a file from the daemon's list.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    pub name: String,
    pub is_dir: bool,
    /// Bytes a leaf occupies; a folder's total.
    pub size: i64,
    /// 0..=100. A folder's is size-weighted, so a 3 GB file at 60% and a 1 kB
    /// file at 0% do not average to 30%.
    pub progress: f64,
    /// A leaf's priority; `-1` for a folder whose children disagree.
    pub priority: i64,
    /// The index into the daemon's flat file list. Leaves only.
    pub file_index: Option<usize>,
    pub children: Vec<TreeNode>,
}

/// The priorities a file can hold, in rtorrent's own numbering.
pub const PRIORITY_OFF: i64 = 0;
pub const PRIORITY_NORMAL: i64 = 1;
pub const PRIORITY_HIGH: i64 = 2;

/// Build the tree from the daemon's flat file list.
#[must_use]
pub fn build_tree(files: &[FileNode]) -> Vec<TreeNode> {
    let mut roots: Vec<TreeNode> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let parts: Vec<&str> = file
            .path
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        let Some((name, dirs)) = parts.split_last() else {
            continue;
        };
        insert(&mut roots, dirs, name, index, file);
    }
    for node in &mut roots {
        roll_up(node);
    }
    roots
}

/// Walk the folder names, creating the nodes they imply, and hang the file off
/// the last one.
fn insert(siblings: &mut Vec<TreeNode>, dirs: &[&str], name: &str, index: usize, file: &FileNode) {
    let Some((next, rest)) = dirs.split_first() else {
        siblings.push(TreeNode {
            name: name.to_owned(),
            is_dir: false,
            size: file.size,
            progress: file.progress,
            priority: file.priority,
            file_index: Some(index),
            children: Vec::new(),
        });
        return;
    };

    let position = siblings
        .iter()
        .position(|node| node.is_dir && node.name == *next)
        .unwrap_or_else(|| {
            siblings.push(TreeNode {
                name: (*next).to_owned(),
                is_dir: true,
                size: 0,
                progress: 0.0,
                priority: PRIORITY_NORMAL,
                file_index: None,
                children: Vec::new(),
            });
            siblings.len() - 1
        });
    insert(&mut siblings[position].children, rest, name, index, file);
}

/// Compute a folder's size, progress and priority from its children.
fn roll_up(node: &mut TreeNode) {
    for child in &mut node.children {
        roll_up(child);
    }
    if !node.is_dir {
        return;
    }

    let size: i64 = node.children.iter().map(|child| child.size).sum();
    node.progress = if size > 0 {
        let weighted: f64 = node
            .children
            .iter()
            .map(|child| child.size as f64 * child.progress)
            .sum();
        weighted / size as f64
    } else {
        // Every child empty: nothing to weight by, so average them.
        let total: f64 = node.children.iter().map(|child| child.progress).sum();
        let count = node.children.len();
        if count == 0 {
            0.0
        } else {
            total / count as f64
        }
    };
    node.size = size;

    // Mixed priorities have no single value; the chip says so rather than
    // showing one child's answer for all of them.
    let mut priorities = node.children.iter().map(|child| child.priority);
    node.priority = match priorities.next() {
        Some(first) if priorities.all(|priority| priority == first) => first,
        _ => -1,
    };
}

/// Every file index under a node — the node itself when it is a leaf.
#[must_use]
pub fn leaf_indexes(node: &TreeNode) -> Vec<usize> {
    match node.file_index {
        Some(index) => vec![index],
        None => node.children.iter().flat_map(leaf_indexes).collect(),
    }
}

/// The chip's word. A folder whose children disagree has no single value.
#[must_use]
pub fn priority_label(priority: i64) -> &'static str {
    match priority {
        PRIORITY_OFF => "Skip",
        PRIORITY_NORMAL => "Normal",
        PRIORITY_HIGH => "High",
        _ => "Mixed",
    }
}

/// A folder's checkbox in the add dialog: on when every file under it is, off
/// when none is, and neither in between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriState {
    Checked,
    Unchecked,
    Indeterminate,
}

/// Which of a node's leaves are selected.
#[must_use]
pub fn folder_state(node: &TreeNode, selected: &HashSet<usize>) -> TriState {
    match node.file_index {
        Some(index) => {
            if selected.contains(&index) {
                TriState::Checked
            } else {
                TriState::Unchecked
            }
        }
        None => {
            let leaves = leaf_indexes(node);
            let chosen = leaves
                .iter()
                .filter(|index| selected.contains(index))
                .count();
            if chosen == 0 {
                TriState::Unchecked
            } else if chosen == leaves.len() {
                TriState::Checked
            } else {
                TriState::Indeterminate
            }
        }
    }
}

/// The bytes a selection of files adds up to, for the dialog's "N selected".
#[must_use]
pub fn selected_size(files: &[FileNode], selected: &HashSet<usize>) -> i64 {
    selected
        .iter()
        .filter_map(|index| files.get(*index))
        .map(|file| file.size)
        .sum()
}

/// The next priority a click cycles to: off → normal → high → off.
///
/// A folder whose children disagree counts as normal to start with, so one click
/// gives the whole subtree a single answer — the rule the Tauri tree uses.
#[must_use]
pub const fn next_priority(current: i64) -> i64 {
    match current {
        PRIORITY_OFF => PRIORITY_NORMAL,
        PRIORITY_HIGH => PRIORITY_OFF,
        // Normal, and a folder's mixed `-1`.
        _ => PRIORITY_HIGH,
    }
}

/// The status a file row's progress bar is drawn with: a skipped file is inert,
/// a complete one is finished, anything else is in flight.
#[must_use]
pub fn file_status(priority: i64, progress: f64) -> Status {
    if priority == PRIORITY_OFF {
        Status::Paused
    } else if progress >= 100.0 {
        Status::Completed
    } else {
        Status::Downloading
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: i64, progress: f64, priority: i64) -> FileNode {
        FileNode {
            path: path.to_owned(),
            size,
            priority,
            progress,
            is_dir: false,
        }
    }

    #[test]
    fn paths_become_folders() {
        let tree = build_tree(&[
            file("iso/install/initrd.gz", 100, 100.0, PRIORITY_HIGH),
            file("iso/install/vmlinuz", 200, 50.0, PRIORITY_HIGH),
            file("iso/README.txt", 10, 0.0, PRIORITY_NORMAL),
            file("checksums.txt", 5, 100.0, PRIORITY_NORMAL),
        ]);

        // A single path element is a top-level file, not a folder.
        assert_eq!(tree.len(), 2);
        let iso = &tree[0];
        assert_eq!((iso.name.as_str(), iso.is_dir), ("iso", true));
        assert_eq!(iso.children.len(), 2);
        let install = &iso.children[0];
        assert_eq!((install.name.as_str(), install.is_dir), ("install", true));
        assert_eq!(install.children.len(), 2);
        // Leaves carry the index into the daemon's flat list.
        assert_eq!(install.children[0].file_index, Some(0));
    }

    #[test]
    fn a_leaf_is_a_node_of_its_own() {
        let tree = build_tree(&[file("only.txt", 42, 12.0, PRIORITY_OFF)]);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "only.txt");
        assert!(!tree[0].is_dir);
        assert_eq!(tree[0].priority, PRIORITY_OFF);
        assert_eq!(tree[0].size, 42);
    }

    #[test]
    fn a_folders_size_is_its_childrens() {
        let tree = build_tree(&[
            file("a/one", 100, 0.0, PRIORITY_NORMAL),
            file("a/two", 300, 0.0, PRIORITY_NORMAL),
        ]);
        assert_eq!(tree[0].size, 400);
    }

    #[test]
    fn a_folders_progress_is_weighted_by_size() {
        // 3 GB at 60% and 1 kB at 0% is ~60%, not 30%.
        let tree = build_tree(&[
            file("a/big", 3_000_000_000, 60.0, PRIORITY_NORMAL),
            file("a/small", 1_000, 0.0, PRIORITY_NORMAL),
        ]);
        let progress = tree[0].progress;
        assert!(progress > 59.9 && progress < 60.0, "{progress}");
    }

    #[test]
    fn empty_files_average_instead_of_dividing_by_zero() {
        let tree = build_tree(&[
            file("a/one", 0, 40.0, PRIORITY_NORMAL),
            file("a/two", 0, 60.0, PRIORITY_NORMAL),
        ]);
        assert_eq!(tree[0].size, 0);
        assert_eq!(tree[0].progress, 50.0);
    }

    #[test]
    fn a_folder_keeps_a_priority_its_children_agree_on() {
        let tree = build_tree(&[
            file("a/one", 1, 0.0, PRIORITY_HIGH),
            file("a/two", 1, 0.0, PRIORITY_HIGH),
        ]);
        assert_eq!(tree[0].priority, PRIORITY_HIGH);
        assert_eq!(priority_label(tree[0].priority), "High");
    }

    #[test]
    fn a_folder_whose_children_disagree_is_mixed() {
        let tree = build_tree(&[
            file("a/one", 1, 0.0, PRIORITY_HIGH),
            file("a/two", 1, 0.0, PRIORITY_OFF),
        ]);
        assert_eq!(tree[0].priority, -1);
        assert_eq!(priority_label(-1), "Mixed");
    }

    #[test]
    fn leaf_indexes_reach_every_file_under_a_node() {
        let tree = build_tree(&[
            file("a/one", 1, 0.0, PRIORITY_NORMAL),
            file("a/deep/two", 1, 0.0, PRIORITY_NORMAL),
            file("b", 1, 0.0, PRIORITY_NORMAL),
        ]);
        assert_eq!(leaf_indexes(&tree[0]), vec![0, 1]);
        assert_eq!(leaf_indexes(&tree[1]), vec![2]);
    }

    #[test]
    fn a_folder_checkbox_follows_its_leaves() {
        let tree = build_tree(&[
            file("a/one", 100, 0.0, PRIORITY_NORMAL),
            file("a/deep/two", 200, 0.0, PRIORITY_NORMAL),
        ]);
        let folder = &tree[0];
        let none = HashSet::new();
        let some = HashSet::from([0]);
        let all = HashSet::from([0, 1]);

        assert_eq!(folder_state(folder, &none), TriState::Unchecked);
        assert_eq!(folder_state(folder, &some), TriState::Indeterminate);
        assert_eq!(folder_state(folder, &all), TriState::Checked);
        // A leaf is simply in or out.
        assert_eq!(folder_state(&tree[0].children[0], &some), TriState::Checked);
        assert_eq!(
            folder_state(&tree[0].children[0], &none),
            TriState::Unchecked
        );
    }

    #[test]
    fn a_selection_sums_only_what_is_there() {
        let files = [
            file("a", 100, 0.0, PRIORITY_NORMAL),
            file("b", 250, 0.0, PRIORITY_NORMAL),
        ];
        assert_eq!(selected_size(&files, &HashSet::from([0, 1])), 350);
        assert_eq!(selected_size(&files, &HashSet::from([1])), 250);
        assert_eq!(selected_size(&files, &HashSet::new()), 0);
        // An index past the list cannot add anything.
        assert_eq!(selected_size(&files, &HashSet::from([9])), 0);
    }

    #[test]
    fn a_click_cycles_off_normal_high() {
        assert_eq!(next_priority(PRIORITY_OFF), PRIORITY_NORMAL);
        assert_eq!(next_priority(PRIORITY_NORMAL), PRIORITY_HIGH);
        assert_eq!(next_priority(PRIORITY_HIGH), PRIORITY_OFF);
        // A folder whose children disagree starts its cycle at normal, so one
        // click gives the subtree one answer rather than landing on "off".
        assert_eq!(next_priority(-1), PRIORITY_HIGH);
    }

    #[test]
    fn a_row_bar_is_inert_when_skipped_and_finished_when_complete() {
        assert_eq!(file_status(PRIORITY_OFF, 40.0), Status::Paused);
        assert_eq!(file_status(PRIORITY_NORMAL, 100.0), Status::Completed);
        assert_eq!(file_status(PRIORITY_NORMAL, 40.0), Status::Downloading);
        assert_eq!(file_status(PRIORITY_HIGH, 0.0), Status::Downloading);
    }

    #[test]
    fn an_empty_path_is_skipped_rather_than_becoming_a_blank_row() {
        assert!(build_tree(&[file("", 1, 0.0, PRIORITY_NORMAL)]).is_empty());
        assert!(build_tree(&[file("/", 1, 0.0, PRIORITY_NORMAL)]).is_empty());
    }
}
