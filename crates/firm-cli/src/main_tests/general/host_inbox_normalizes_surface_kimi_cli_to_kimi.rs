use super::*;

    #[cfg(any())] // Historical host-inbox surface normalization over retired message authoring.
    fn host_inbox_normalizes_surface_kimi_cli_to_kimi() {
        let (store, root) = temp_store("host-inbox-kimi-cli-to-kimi");
        let created = create_two_member_team_run(&store);
        let member = &created.member_runs[0];
        let assignment = seed_host_conversation(&store, &created, 0);
        let current = latest_team_run(&store, &created.team_run.id).expect("current run");
        let mut bound = current.clone();
        bound.host_surface = "kimi-cli".into();
        bound.host_thread_id = Some("thread-t2".into());
        bound.updated_at = "unix-ms:host-bound".into();
        store
            .compare_and_append_team_run(&current, &bound)
            .expect("bind native Host");

        send_team_message(
            &store,
            &bound.id,
            &member.id,
            vec!["host".into()],
            ProviderDispatchIntent::Message,
            "QUESTION: reverse normalization check",
            Some(assignment.correlation_id.clone()),
            Some(assignment.id.clone()),
            None,
            None,
        )
        .expect("member asks Host");

        // Query with "kimi" — should find the "kimi-cli"-bound run
        let result =
            host_inbox_for_native_thread(&store, "kimi", "thread-t2", false).expect("kimi inbox");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["team_run_id"], bound.id);

        let _ = std::fs::remove_dir_all(root);
    }

