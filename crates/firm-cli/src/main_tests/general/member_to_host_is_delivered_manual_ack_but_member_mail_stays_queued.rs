use super::*;

    #[cfg(any())] // Historical CLI-send/manual-ack authority; canonical MessageDelivery journey supersedes it.
    fn member_to_host_is_delivered_manual_ack_but_member_mail_stays_queued() {
        let (store, root) = temp_store("team-delivery-routing");
        let created = create_two_member_team_run(&store);
        let first = &created.member_runs[0];
        let second = &created.member_runs[1];
        let seed = seed_host_conversation(&store, &created, 0);
        let correlation = seed.correlation_id.clone();

        let host_mail = send_team_message(
            &store,
            &created.team_run.id,
            &first.id,
            vec!["host".into()],
            ProviderDispatchIntent::Message,
            "Need a product decision",
            Some(correlation.clone()),
            Some(seed.id.clone()),
            None,
            None,
        )
        .expect("member to host");
        assert_eq!(
            host_mail.deliveries[0].policy,
            TeamDeliveryPolicy::ManualAck
        );
        assert_eq!(
            host_mail.deliveries[0].status,
            TeamDeliveryStatus::Delivered
        );
        assert_eq!(host_mail.deliveries[0].attempt, 1);

        let peer_mail = send_team_message(
            &store,
            &created.team_run.id,
            &first.id,
            vec![second.id.clone()],
            ProviderDispatchIntent::Message,
            "Shared interface is ready",
            Some(correlation.clone()),
            None,
            None,
            None,
        )
        .expect("member to peer");
        assert_eq!(peer_mail.deliveries[0].policy, TeamDeliveryPolicy::Queue);
        assert_eq!(peer_mail.deliveries[0].status, TeamDeliveryStatus::Queued);
        assert_eq!(peer_mail.deliveries[0].attempt, 0);

        let host_to_member = send_team_message(
            &store,
            &created.team_run.id,
            "host",
            vec![first.id.clone()],
            ProviderDispatchIntent::Message,
            "Use the stable interface",
            Some(correlation),
            Some(host_mail.id),
            None,
            None,
        )
        .expect("host to member");
        assert_eq!(
            host_to_member.deliveries[0].policy,
            TeamDeliveryPolicy::Queue
        );
        assert_eq!(
            host_to_member.deliveries[0].status,
            TeamDeliveryStatus::Queued
        );
        let _ = std::fs::remove_dir_all(root);
    }

