use super::*;

#[test]
fn build_step_details_caps_large_worktree_diff() {
    let spec = workflow::AgentStepSpec {
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
        isolation: Some("worktree".into()),
        prompt: "hi".into(),
        schema: None,
        schema_strict: false,
        writable: false,
        ordinal: None,
    };
    let spawn = EphemeralSpawn {
        ok: true,
        reply: None,
        native_session: None,
        stderr: String::new(),
        exit_code: Some(0),
        timed_out: false,
        wall_timed_out: false,
        tokens: None,
        model: None,
        structured: None,
        cost_usd: None,
        warnings: Vec::new(),
    };
    let big = "x".repeat(WORKTREE_DIFF_CAP + 5_000);
    let details = build_step_details(&spec, &spawn, spec.model.as_deref(), 1, Some(&big), None);
    let stored = details["worktree_diff"].as_str().expect("diff string");
    assert_eq!(stored.len(), WORKTREE_DIFF_CAP);
    assert_eq!(details["worktree_diff_truncated"], serde_json::json!(true));

    // A small diff is stored whole and NOT flagged truncated.
    let small = "diff --git a b\n+added\n";
    let details = build_step_details(&spec, &spawn, spec.model.as_deref(), 1, Some(small), None);
    assert_eq!(details["worktree_diff"], serde_json::json!(small));
    assert_eq!(details["worktree_diff_truncated"], serde_json::json!(false));
}
