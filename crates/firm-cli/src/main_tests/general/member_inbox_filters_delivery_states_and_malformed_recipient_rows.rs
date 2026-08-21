use super::*;

    #[cfg(any())] // Historical malformed retired-delivery fixture; canonical schema/route negatives supersede it.
    fn member_inbox_filters_delivery_states_and_malformed_recipient_rows() {
        let (store, root) = temp_store("team-inbox-states");
        let created = create_two_member_team_run(&store);
        let first = &created.member_runs[0];
        let second = &created.member_runs[1];
        let template = seed_host_conversation(&store, &created, 0);

        let mut delivered = template.clone();
        delivered.id = generated_id("delivered");
        delivered.body = "already delivered but still actionable".into();
        delivered.deliveries[0].status = TeamDeliveryStatus::Delivered;
        delivered.deliveries[0].attempt = 1;
        store
            .append_team_message(&delivered)
            .expect("append delivered message");

        for state in [TeamDeliveryStatus::Failed, TeamDeliveryStatus::Expired] {
            let mut terminal = template.clone();
            terminal.id = generated_id("terminal");
            terminal.body = format!("terminal {state:?}");
            terminal.deliveries[0].status = state;
            terminal.deliveries[0].attempt = 1;
            store
                .append_team_message(&terminal)
                .expect("append terminal message");
        }

        let mut malformed = template.clone();
        malformed.id = generated_id("malformed");
        malformed.body = "envelope names first but delivery belongs to second".into();
        malformed.deliveries[0].member_id = second.id.clone();
        store
            .append_team_message(&malformed)
            .expect("append malformed compatibility row");

        let actionable =
            team_run_inbox(&store, &created.team_run.id, &first.id, false).expect("inbox");
        assert!(
            actionable.iter().any(|message| message.id == template.id),
            "queued assignment remains actionable"
        );
        assert!(
            actionable.iter().any(|message| message.id == delivered.id),
            "delivered mail remains actionable"
        );
        assert!(
            actionable.iter().all(|message| {
                message.deliveries.iter().any(|delivery| {
                    delivery.member_id == first.id
                        && matches!(
                            delivery.status,
                            TeamDeliveryStatus::Queued | TeamDeliveryStatus::Delivered
                        )
                })
            }),
            "default inbox excludes terminal and mismatched delivery rows"
        );

        let all = team_run_inbox(&store, &created.team_run.id, &first.id, true).expect("all inbox");
        assert_eq!(all.len(), 4, "all returns every valid received message");
        assert!(
            all.iter().all(|message| message.id != malformed.id),
            "recipient envelope without a matching delivery is not received mail"
        );
        let _ = std::fs::remove_dir_all(root);
    }

