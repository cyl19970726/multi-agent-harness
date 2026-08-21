use super::*;

#[test]
fn parallel_data_driven_comprehension_runs_every_spec() {
    // A DATA-DRIVEN fan-out: the script builds the spec list from `args` via
    // a list comprehension, so N items → exactly N concurrent steps, all
    // collected by the barrier.
    let seen = Mutex::new(Vec::new());
    let script = r#"results = parallel([{"prompt": "fix " + x} for x in args["items"]])"#;
    let args = serde_json::json!({ "items": ["a", "b", "c", "d"] });
    let outcome = {
        let driver = recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", Some(&args), &driver)
            .expect("run ok")
            .outcome
    };
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 4, "one step per comprehension item");
    assert_eq!(outcome.steps.len(), 4, "barrier collected every slot");
    // Every prompt flowed through and the run completed.
    let prompts: Vec<String> = seen.iter().map(|(_, prompt)| prompt.clone()).collect();
    for item in ["fix a", "fix b", "fix c", "fix d"] {
        assert!(prompts.contains(&item.to_string()), "missing prompt {item}");
    }
    assert_eq!(outcome.status, WorkflowRunStatus::Completed);
}
