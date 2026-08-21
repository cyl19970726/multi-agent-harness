use super::*;

#[test]
fn patch_artifact_kwargs_flow_through_parallel_and_pipeline_specs() {
    let seen = Mutex::new(Vec::<AgentStepSpec>::new());
    let driver = |spec: &AgentStepSpec| {
        seen.lock().unwrap().push(spec.clone());
        StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            output_summary: "ok".to_string(),
            step_id: None,
            started_at: None,
            details: None,
            structured: None,
            ordinal: None,
        }
    };
    let script = r#"
parallel([
{
    "prompt": "fan",
    "label": "fan",
    "writable": True,
    "persist_changes": "patch",
    "owned_paths": ["src"],
    "artifact_root": "out",
    "write_roots": ["out"],
    "auto_apply_on_verdict": True,
},
])
pipeline(
["item"],
[{
    "prompt": "pipe {input}",
    "label": "pipe",
    "writable": True,
    "persist_changes": "discard",
    "owned_paths": ["docs"],
    "artifact_root": "reports",
    "write_roots": ["reports"],
    "auto_apply_on_verdict": True,
}],
)
"#;
    run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
    let seen = seen.into_inner().unwrap();
    let fan = seen.iter().find(|spec| spec.label == "fan").unwrap();
    assert_eq!(fan.persist_changes.as_deref(), Some("patch"));
    assert_eq!(fan.owned_paths, vec!["src"]);
    assert_eq!(fan.artifact_root.as_deref(), Some("out"));
    assert_eq!(fan.write_roots, vec!["out"]);
    assert!(fan.auto_apply_on_verdict);
    let pipe = seen.iter().find(|spec| spec.label == "pipe").unwrap();
    assert_eq!(pipe.persist_changes.as_deref(), Some("discard"));
    assert_eq!(pipe.owned_paths, vec!["docs"]);
    assert_eq!(pipe.artifact_root.as_deref(), Some("reports"));
    assert_eq!(pipe.write_roots, vec!["reports"]);
    assert!(pipe.auto_apply_on_verdict);
}
