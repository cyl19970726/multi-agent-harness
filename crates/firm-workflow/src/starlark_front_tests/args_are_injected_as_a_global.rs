use super::*;

#[test]
fn args_are_injected_as_a_global() {
    let seen = Mutex::new(Vec::new());
    let script = r#"agent("audit " + args["area"])"#;
    let args = serde_json::json!({ "area": "checkout flow" });
    let outcome = {
        let driver = recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", Some(&args), &driver)
            .expect("run ok")
            .outcome
    };
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].1.contains("audit checkout flow"));
    assert_eq!(outcome.steps.len(), 1);
}
