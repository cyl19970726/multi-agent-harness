use super::*;

    #[test]
    fn member_run_open_native_print_only_never_launches_an_application() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("native-open")));
        let store = HarnessStore::new(&root);
        store.init().expect("initialize native-open test store");
        append_jsonl_value(
            &store.root().join("team_runs.jsonl"),
            &serde_json::to_value(AgentTeamRun {
                id: "team-native-open".into(),
                agent_team_id: "team-native-open-definition".into(),
                execution_node_id: "node-native-open".into(),
                previous_run_id: None,
                project_binding_id: "project-native-open".into(),
                host_surface: "test".into(),
                host_thread_id: None,
                host_actor: None,
                host_control_mode: Default::default(),
                objective: "native open print-only test".into(),
                execution_root: None,
                status: TeamRunStatus::Planning,
                member_run_ids: vec!["member-native-open".into()],
                budget_limit_usd: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            })
            .expect("serialize Legacy TeamRun fixture"),
        )
        .expect("declare native-open test member");
        append_jsonl_value(
            &store.root().join("member_runs.jsonl"),
            &serde_json::to_value(native_open_test_member(
                "claude",
                "claude_agent_sdk",
                "851b37dd-1234-5678-9abc-0123456789ab",
            ))
            .expect("serialize Legacy member fixture"),
        )
        .expect("append member run");
        member_run_command(
            &store,
            &[
                "open-native".into(),
                "--id".into(),
                "member-native-open".into(),
                "--print-only".into(),
            ],
        )
        .expect("print-only route");
        let _ = std::fs::remove_dir_all(root);
    }

