use super::*;

#[test]
fn pipeline_accepts_bare_positional_stages_not_just_a_list() {
    // issue #139 item 4: `pipeline(items, stage1, stage2)` (bare positional
    // stages, the generic-tool convention) must work, not only the canonical
    // `pipeline(items, [stage1, stage2])` list form. Both normalize identically.
    let seen = Mutex::new(Vec::new());
    let script = r#"
results = pipeline(
["alpha"],
{"prompt": "scan {input}", "label": "s1"},
{"prompt": "fix per {input}", "label": "s2"},
)
log("alpha last: " + results[0])
"#;
    let outcome = {
        let driver = recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    let pairs: std::collections::HashSet<(String, String)> =
        seen.into_inner().unwrap().into_iter().collect();
    assert!(pairs.contains(&("s1".to_string(), "scan alpha".to_string())));
    assert!(pairs.contains(&("s2".to_string(), "fix per ok: scan alpha".to_string())));
    assert_eq!(outcome.steps.len(), 2);
    assert_eq!(outcome.status, WorkflowRunStatus::Completed);
}
