use super::*;

#[test]
fn worktree_paths_are_unique_per_leaf_even_with_duplicate_labels() {
    // issue #139 item 7: two SAME-LABEL writable nodes in one run must NOT
    // share a worktree path/branch (the collision that failed the 2nd node).
    // The per-leaf session_id disambiguates them.
    let (rel_a, br_a) = worktree_paths("wfrun-1", "dup", "session-1-0");
    let (rel_b, br_b) = worktree_paths("wfrun-1", "dup", "session-1-1");
    assert_ne!(rel_a, rel_b, "same-label leaves must get distinct paths");
    assert_ne!(br_a, br_b, "same-label leaves must get distinct branches");
    // Stable for the same leaf, and the label + run are still in the name.
    assert_eq!(
        worktree_paths("wfrun-1", "dup", "session-1-0"),
        (rel_a.clone(), br_a.clone())
    );
    assert!(rel_a.contains("wfrun-1") && rel_a.contains("dup"));
    assert!(br_a.starts_with("harness/wt/"));
}
