use super::*;

#[test]
fn patch_and_artifact_authoring_intents_round_trip() {
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
agent(
"edit",
label = "writer",
writable = True,
persist_changes = "patch",
owned_paths = ["src"],
artifact_root = "out",
write_roots = ["out"],
auto_apply_on_verdict = True,
)
artifact_manifest(["summary.md"], label = "writer", artifact_root = "out", write_roots = ["out"])
apply_patch("writer", "review passed")
verdict(True, "patch reviewed inside workflow")
"#;
    let run = run_starlark(&format!("{HEADER}{script}"), "demo", None, &driver).expect("run ok");
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 1);
    let spec = &seen[0];
    assert_eq!(spec.persist_changes.as_deref(), Some("patch"));
    assert_eq!(spec.owned_paths, vec!["src"]);
    assert_eq!(spec.artifact_root.as_deref(), Some("out"));
    assert_eq!(spec.write_roots, vec!["out"]);
    assert!(spec.auto_apply_on_verdict);

    let final_output = run.outcome.final_output.expect("final_output");
    assert_eq!(
        final_output["patch_actions"][0]["action"],
        serde_json::json!("apply")
    );
    assert_eq!(
        final_output["patch_actions"][0]["label"],
        serde_json::json!("writer")
    );
    assert_eq!(
        final_output["artifact_manifests"][0]["paths"],
        serde_json::json!(["summary.md"])
    );
}
