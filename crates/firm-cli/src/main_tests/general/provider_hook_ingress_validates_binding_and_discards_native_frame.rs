use super::*;

    #[test]
    fn provider_hook_ingress_validates_binding_and_discards_native_frame() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("hook")));
        let store = HarnessStore::new(&root);
        let mut member = make_member("agent-1");
        member.provider_runtime_id = Some("runtime-1".into());
        store.append_member(&member).expect("append member");
        store
            .append_runtime(&ProviderProcess {
                id: "runtime-1".into(),
                agent_member_id: member.id.clone(),
                provider: "codex".into(),
                status: ProviderProcessStatus::Running,
                pid: None,
                control_endpoint: None,
                command: "codex".into(),
                args: Vec::new(),
                started_at: "unix-ms:1".into(),
                ended_at: None,
                last_event_at: Some("unix-ms:1".into()),
                health: ProviderProcessHealth::default(),
            })
            .expect("append runtime");
        let before_member = latest_member(&store, "agent-1").expect("member before");
        let before_runtime = latest_runtime(&store, "runtime-1")
            .expect("runtime before")
            .expect("runtime exists");
        let args = vec![
            "--agent".to_string(),
            "agent-1".to_string(),
            "--runtime".to_string(),
            "runtime-1".to_string(),
        ];
        accept_provider_hook_event(
            &store,
            &args,
            "codex",
            &serde_json::json!({
                "hook_event_name":"Stop",
                "session_id":"private-session",
                "turn_id":"private-turn",
                "tool_name":"private-tool"
            }),
        )
        .expect("exact hook binding is accepted and discarded");

        assert_eq!(latest_member(&store, "agent-1").unwrap(), before_member);
        assert_eq!(
            latest_runtime(&store, "runtime-1").unwrap().unwrap(),
            before_runtime
        );
        assert!(store.evidence().unwrap().is_empty());
        assert!(store.messages().unwrap().is_empty());
        assert!(accept_provider_hook_event(&store, &args, "kimi", &serde_json::json!({})).is_err());

        let _ = std::fs::remove_dir_all(root);
    }

