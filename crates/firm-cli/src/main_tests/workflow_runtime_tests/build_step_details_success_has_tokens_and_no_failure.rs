use super::*;

    #[test]
    fn build_step_details_success_has_tokens_and_no_failure() {
        let spec = workflow::AgentStepSpec {
            phase: "p".into(),
            label: "l".into(),
            provider: "codex".into(),
            model: Some("gpt-5-codex".into()),
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
            ok: true,
            reply: Some("done".into()),
            native_session: None,
            stderr: String::new(),
            exit_code: Some(0),
            timed_out: false,
            wall_timed_out: false,
            tokens: Some(TokenUsage {
                input: 10,
                output: 4,
                total: 14,
            }),
            model: None,
            structured: None,
            cost_usd: None,
            warnings: Vec::new(),
        };
        let details = build_step_details(&spec, &spawn, spec.model.as_deref(), 1234, None, None);
        // spec.model wins over the (absent) worker-reported model.
        assert_eq!(details["model"], serde_json::json!("gpt-5-codex"));
        assert_eq!(details["exit_code"], serde_json::json!(0));
        assert_eq!(details["duration_ms"], serde_json::json!(1234));
        assert_eq!(details["tokens"]["total"], serde_json::json!(14));
        assert!(details.get("failure").is_none());
        assert!(details.get("worktree_diff").is_none());
    }

