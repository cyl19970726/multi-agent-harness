use super::*;

#[test]
fn workflow_run_defaults_do_not_override_leaf_model_or_effort() {
    let options = WorkflowDeliveryOptions {
        dry_run: false,
        start_runtime: false,
        timeout_ms: 1_000,
        default_model: Some("run-model".into()),
        default_effort: Some("medium".into()),
        max_budget_usd: None,
        progress: false,
        project: temp_project_context("eff", false),
    };
    let mut spec = workflow::AgentStepSpec {
        phase: "p".into(),
        label: "l".into(),
        provider: "codex".into(),
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

    assert_eq!(workflow_effective_model(&options, &spec), Some("run-model"));
    assert_eq!(workflow_effective_effort(&options, &spec), Some("medium"));

    spec.model = Some("leaf-model".into());
    spec.effort = Some("high".into());
    assert_eq!(
        workflow_effective_model(&options, &spec),
        Some("leaf-model")
    );
    assert_eq!(workflow_effective_effort(&options, &spec), Some("high"));
}
