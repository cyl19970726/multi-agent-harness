use super::*;

#[test]
fn workflow_run_script_rejects_missing_design_intent() {
    // A program with no `workflow(...)` header is rejected fail-fast, and the
    // error mentions design_intent so the author knows what to add.
    let store = temp_store("run-script-no-intent");
    let dir = std::env::temp_dir().join(format!("harness-wf-script-{}", generated_id("noi")));
    fs::create_dir_all(&dir).expect("mkdir script dir");
    let path = dir.join("noheader.star");
    fs::write(&path, r#"agent("x")"#).expect("write script");

    let args = vec![path.display().to_string(), "--dry-run".to_string()];
    let err = workflow_run_script_value(&store, None, &args).expect_err("rejected");
    match err {
        CliError::Usage(message) => assert!(
            message.contains("design_intent"),
            "error should mention design_intent: {message}"
        ),
        other => panic!("expected Usage error, got {other:?}"),
    }
}
