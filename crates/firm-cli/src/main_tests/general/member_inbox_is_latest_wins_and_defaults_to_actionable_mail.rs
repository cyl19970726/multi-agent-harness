use super::*;

    #[cfg(any())] // Historical inbox latest-wins projection; canonical MessageDelivery view owns current truth.
    fn member_inbox_is_latest_wins_and_defaults_to_actionable_mail() {
        let (store, root) = temp_store("team-inbox");
        let created = create_two_member_team_run(&store);
        let first = &created.member_runs[0];
        let assignment = seed_host_conversation(&store, &created, 0);

        let actionable =
            team_run_inbox(&store, &created.team_run.id, &first.id, false).expect("inbox");
        assert_eq!(actionable.len(), 1);
        assert_eq!(actionable[0].id, assignment.id);

        let mut acknowledged = assignment.clone();
        acknowledged.deliveries[0].status = TeamDeliveryStatus::Acknowledged;
        acknowledged.deliveries[0].attempt = 1;
        acknowledged.deliveries[0].updated_at = now_string();
        store
            .append_team_message(&acknowledged)
            .expect("append latest message row");

        assert!(
            team_run_inbox(&store, &created.team_run.id, &first.id, false)
                .expect("actionable inbox")
                .is_empty(),
            "acknowledged latest row is no longer actionable"
        );
        let history =
            team_run_inbox(&store, &created.team_run.id, &first.id, true).expect("all inbox");
        assert_eq!(history.len(), 1, "latest-wins must collapse append history");
        assert_eq!(
            history[0].deliveries[0].status,
            TeamDeliveryStatus::Acknowledged
        );

        let error = team_run_inbox(
            &store,
            &created.team_run.id,
            "member-run-from-another-team",
            false,
        )
        .expect_err("cross-run recipient is rejected");
        assert!(error.to_string().contains("does not belong"));
        let _ = std::fs::remove_dir_all(root);
    }

