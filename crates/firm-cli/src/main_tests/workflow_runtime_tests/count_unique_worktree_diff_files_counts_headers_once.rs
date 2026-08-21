use super::*;

#[test]
fn count_unique_worktree_diff_files_counts_headers_once() {
    let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-old
+new
diff --git a/docs/workflow-runtime.md b/docs/workflow-runtime.md
index 3333333..4444444 100644
--- a/docs/workflow-runtime.md
+++ b/docs/workflow-runtime.md
@@ -1 +1 @@
-old
+new
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
";
    assert_eq!(count_unique_worktree_diff_files(diff), 2);
    assert_eq!(count_unique_worktree_diff_files("no diff headers\n"), 0);
}
