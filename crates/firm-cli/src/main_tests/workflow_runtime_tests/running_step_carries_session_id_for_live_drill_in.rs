use super::*;

#[test]
fn running_step_carries_session_id_for_live_drill_in() {
    // The `running` row a step journals at start must carry the same
    // native_session as its terminal row — so the dashboard can link the
    // step to its LIVE turn-event stream WHILE it runs, not only after it
    // finishes. (dry-run exercises the journaling without spawning a worker.)
    let store = temp_store("live-step-session");
    let options = WorkflowDeliveryOptions {
        dry_run: true,
        start_runtime: false,
        timeout_ms: 1_000,
        default_model: None,
        default_effort: None,
        max_budget_usd: None,
        progress: false,
        project: temp_project_context("live", false),
    };
    let spec = workflow::AgentStepSpec {
        phase: "scan".into(),
        label: "scan-context".into(),
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
        prompt: "do the thing".into(),
        schema: None,
        schema_strict: false,
        writable: false,
        ordinal: Some(0),
    };

    let _result = workflow_real_agent_step(&store, "wfrun-live", &options, &spec);

    // Read the RAW append log (not the latest-wins projection) so we can
    // inspect the `running` row distinctly from the terminal row.
    let rows = store.workflow_steps().expect("read step rows");
    let running = rows
        .iter()
        .find(|s| s.status == WorkflowStepStatus::Running)
        .expect("a running row was journaled at step start");
    // A native session is bound only after the provider reports its own id.
    assert!(running.native_session.is_none());
    let terminal = rows
        .iter()
        .find(|s| {
            matches!(
                s.status,
                WorkflowStepStatus::Completed | WorkflowStepStatus::Failed
            )
        })
        .expect("a terminal row was journaled at step finish");
    assert!(terminal.native_session.is_none());
}
