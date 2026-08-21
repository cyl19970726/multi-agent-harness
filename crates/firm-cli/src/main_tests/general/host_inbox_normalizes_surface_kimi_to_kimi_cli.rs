use super::*;

    #[cfg(any())] // Historical host-inbox surface normalization over retired message authoring.
    fn host_inbox_normalizes_surface_kimi_to_kimi_cli() {
        let (store, root) = temp_store("host-inbox-kimi-to-kimi-cli");
        let created = create_two_member_team_run(&store);
        let member = &created.member_runs[0];
        let assignment = seed_host_conversation(&store, &created, 0);
        let current = latest_team_run(&store, &created.team_run.id).expect("current run");
        let mut bound = current.clone();
        bound.host_surface = "kimi".into();
        bound.host_thread_id = Some("thread-t1".into());
        bound.updated_at = "unix-ms:host-bound".into();
        store
            .compare_and_append_team_run(&current, &bound)
            .expect("bind native Host");

        let mail = send_team_message(
            &store,
            &bound.id,
            &member.id,
            vec!["host".into()],
            ProviderDispatchIntent::Message,
            "QUESTION: test surface normalization",
            Some(assignment.correlation_id.clone()),
            Some(assignment.id.clone()),
            None,
            None,
        )
        .expect("member asks Host");

        // Query with "kimi-cli" — should find the "kimi"-bound run
        let result = host_inbox_for_native_thread(&store, "kimi-cli", "thread-t1", false)
            .expect("kimi-cli inbox");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["team_run_id"], bound.id);
        assert_eq!(result[0]["messages"][0]["id"], mail.id);

        // Also works with "kimi-code"
        let result2 = host_inbox_for_native_thread(&store, "kimi-code", "thread-t1", false)
            .expect("kimi-code inbox");
        assert_eq!(result2.len(), 1);

        let _ = std::fs::remove_dir_all(root);
    }

