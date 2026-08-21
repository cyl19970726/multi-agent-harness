use super::*;

#[test]
fn discarded_worktree_diff_warning_names_run_step_and_recovery() {
    let diff = "\
diff --git a/src/new.rs b/src/new.rs
new file mode 100644
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1 @@
+pub fn new() {}
";
    let step = workflow::StepResult {
        phase: "impl".into(),
        label: "writer".into(),
        provider: "codex".into(),
        isolation: Some("worktree".into()),
        ok: true,
        output_summary: "done".into(),
        step_id: None,
        started_at: None,
        details: Some(serde_json::json!({ "worktree_diff": diff })),
        structured: None,
        ordinal: Some(0),
    };
    let warning = discarded_worktree_diff_warning("wfrun-test", &step).expect("warning emitted");
    assert!(warning.contains("workflow run wfrun-test"));
    assert!(warning.contains("step 'writer'"));
    assert!(warning.contains("1 changed file(s)"));
    assert!(warning.contains("harness workflow get-output wfrun-test --step writer"));
    assert!(warning.contains("harness workflow patch apply"));
}
