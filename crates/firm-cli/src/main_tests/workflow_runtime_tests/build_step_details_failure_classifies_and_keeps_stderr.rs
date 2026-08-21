use super::*;

#[test]
fn build_step_details_failure_classifies_and_keeps_stderr() {
    let spec = workflow::AgentStepSpec {
        phase: "p".into(),
        label: "l".into(),
        provider: "claude".into(),
        model: None,
        effort: None,
        service_tier: None,
        fallback_model: None,
        timeout_s: None,
        image: Vec::new(),
        add_dir: Vec::new(),
        expected_artifacts: Vec::new(),
        persist_changes: None,
        write_mode: None,
        owned_paths: Vec::new(),
        artifact_root: None,
        write_roots: Vec::new(),
        auto_apply_on_verdict: false,
        isolation: None,
        prompt: "hi".into(),
        schema: None,
        schema_strict: false,
        writable: false,
        ordinal: None,
    };
    let spawn = EphemeralSpawn {
        ok: false,
        reply: None,
        native_session: None,
        stderr: "boom: provider exploded".into(),
        exit_code: Some(3),
        timed_out: false,
        wall_timed_out: false,
        tokens: None,
        // The node requested no model, so the worker-reported one is used.
        model: Some("claude-opus-4-8".into()),
        structured: None,
        cost_usd: None,
        warnings: Vec::new(),
    };
    let details = build_step_details(&spec, &spawn, spec.model.as_deref(), 50, None, None);
    assert_eq!(details["model"], serde_json::json!("claude-opus-4-8"));
    assert_eq!(details["failure"]["failed"], serde_json::json!(true));
    assert_eq!(details["failure"]["reason"], serde_json::json!("exit"));
    assert_eq!(
        details["failure"]["detail"],
        serde_json::json!("boom: provider exploded")
    );
}
