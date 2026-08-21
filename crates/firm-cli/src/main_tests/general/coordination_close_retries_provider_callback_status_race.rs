use super::*;

    #[test]
    fn coordination_close_retries_provider_callback_status_race() {
        use std::cell::Cell;

        let (store, root) = temp_store("coordination-close-provider-status-race");
        let created = create_two_member_team_run(&store);
        let initial = created.member_runs[0].clone();
        let mut waiting = initial.clone();
        waiting.status = MemberRunStatus::Waiting;
        waiting.last_event_at = Some("unix-ms:100".into());
        store
            .compare_and_append_member_run(&initial, &waiting)
            .expect("seed provider interaction wait");

        let attempts = Cell::new(0_usize);
        let closed = mark_member_coordination_closed_with_hook(
            &store,
            &created.team_run.id,
            &waiting.id,
            |attempt, expected| {
                attempts.set(attempts.get() + 1);
                if attempt == 0 {
                    let mut callback_resumed = expected.clone();
                    callback_resumed.status = MemberRunStatus::Running;
                    callback_resumed.last_event_at = Some("unix-ms:101".into());
                    store.compare_and_append_member_run(expected, &callback_resumed)?;
                }
                Ok(())
            },
        )
        .expect("coordination close retries the provider callback CAS");

        assert!(closed.coordination_is_closed());
        assert_eq!(closed.status, MemberRunStatus::Running);
        assert_eq!(attempts.get(), 2, "the retry must be bounded and exact");
        assert_eq!(
            latest_member_runs_in_append_order(&store)
                .expect("read members")
                .into_iter()
                .find(|member| member.id == waiting.id)
                .expect("closed member")
                .coordination_status,
            MemberCoordinationStatus::Closed
        );

        let other_initial = created.member_runs[1].clone();
        let mut other_waiting = other_initial.clone();
        other_waiting.status = MemberRunStatus::Waiting;
        other_waiting.last_event_at = Some("unix-ms:200".into());
        store
            .compare_and_append_member_run(&other_initial, &other_waiting)
            .expect("seed second provider interaction wait");
        let unrelated_attempts = Cell::new(0_usize);
        let error = mark_member_coordination_closed_with_hook(
            &store,
            &created.team_run.id,
            &other_waiting.id,
            |attempt, expected| {
                unrelated_attempts.set(unrelated_attempts.get() + 1);
                if attempt == 0 {
                    let mut unrelated = expected.clone();
                    unrelated.runtime_generation += 1;
                    unrelated.last_event_at = Some("unix-ms:201".into());
                    store.compare_and_advance_member_run_generation(expected, &unrelated)?;
                }
                Ok(())
            },
        )
        .expect_err("successor runtime generation fails closed");
        assert!(error
            .to_string()
            .contains("changed outside the exact runtime generation admitted by Close"));
        assert_eq!(
            unrelated_attempts.get(),
            1,
            "successor authority drift is not retried"
        );
        assert!(
            latest_member_runs_in_append_order(&store)
                .expect("read members")
                .into_iter()
                .find(|member| member.id == other_waiting.id)
                .expect("second member")
                .coordination_is_active(),
            "failed close must not mutate coordination authority"
        );
        let _ = std::fs::remove_dir_all(root);
    }

