use super::*;

#[test]
fn workflow_run_script_rejects_bad_args_json() {
    let store = temp_store("run-script-badargs");
    let dir = std::env::temp_dir().join(format!("harness-wf-script-{}", generated_id("bad")));
    fs::create_dir_all(&dir).expect("mkdir script dir");
    let path = dir.join("noop.star");
    fs::write(&path, r#"agent("x")"#).expect("write script");

    let args = vec![
        path.display().to_string(),
        "--args".to_string(),
        "{not json".to_string(),
        "--dry-run".to_string(),
    ];
    let err = workflow_run_script_value(&store, None, &args).expect_err("bad json");
    assert!(matches!(err, CliError::Usage(_)));
}
