use super::*;

#[test]
fn pipeline_flows_every_item_through_all_stages_in_order() {
    // 2 items x 2 stages: each item must visit BOTH stages, and stage 2's prompt
    // must carry the forward-injected output of stage 1 (proving the no-barrier
    // streaming engine threads the prior value into the next stage's template).
    let seen = Mutex::new(Vec::new());
    let script = r#"
results = pipeline(
["alpha", "beta"],
[
    {"prompt": "scan {input}", "label": "s1"},
    {"prompt": "fix per {input}", "label": "s2"},
],
)
log("alpha last: " + results[0])
log("beta last: " + results[1])
"#;
    let outcome = {
        let driver = recording_driver(&seen);
        run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver)
            .expect("run ok")
            .outcome
    };
    let seen = seen.into_inner().unwrap();
    // 2 items x 2 stages = 4 driver dispatches.
    assert_eq!(seen.len(), 4);
    // Every (label, prompt) pair that must have run, regardless of interleaving.
    // recording_driver returns "ok: <prompt>", so stage 2 sees stage 1's output.
    let pairs: std::collections::HashSet<(String, String)> = seen.into_iter().collect();
    assert!(pairs.contains(&("s1".to_string(), "scan alpha".to_string())));
    assert!(pairs.contains(&("s1".to_string(), "scan beta".to_string())));
    assert!(pairs.contains(&("s2".to_string(), "fix per ok: scan alpha".to_string())));
    assert!(pairs.contains(&("s2".to_string(), "fix per ok: scan beta".to_string())));

    // Every produced step (item x stage) is journaled.
    assert_eq!(outcome.steps.len(), 4);
    assert!(outcome.steps.iter().all(|s| s.ok));
    // The script-visible return is the LAST stage's summary, in input order.
    assert_eq!(outcome.status, WorkflowRunStatus::Completed);
}
