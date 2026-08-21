use super::*;

    #[test]
    fn peer_message_resolver_fences_remote_topology_and_revisions() {
        let fixture = PeerMessagingFixture::new("peer-resolver-remote-fences");
        let draft = fixture.team_draft("hello remote team");
        let request = |node: &str, space: &str, subscription_revision: Option<u64>| {
            fabric_runtime::QueueCollaborationMessageRequest {
                company_id: "company-test".into(),
                target_team_id: fixture.target_team_id.clone(),
                target_team_revision: 1,
                target_node_id: node.into(),
                target_execution_space_id: space.into(),
                target_subscription_revision: subscription_revision,
                expected_delegation_revision: 0,
                expires_unix_ms: current_unix_ms_u64() + 60_000,
            }
        };
        // Same-Node route facts are rejected: the fabric never loops back.
        resolve_peer_team_message_admission_authority(
            &fixture.store,
            &fixture.firm_home,
            "space-test",
            &fixture.node_id,
            &fixture.sender_actor(),
            &draft,
            Some(&request(&fixture.node_id, "space-remote", Some(1))),
        )
        .expect_err("same-Node cross-Space routing is closed");
        // A genuinely remote target needs the caller's current target
        // subscription revision; nothing is hardcoded or guessed.
        resolve_peer_team_message_admission_authority(
            &fixture.store,
            &fixture.firm_home,
            "space-test",
            &fixture.node_id,
            &fixture.sender_actor(),
            &draft,
            Some(&request("node-remote", "space-remote", None)),
        )
        .expect_err("remote resolution without the subscription revision fails closed");
        let resolved = resolve_peer_team_message_admission_authority(
            &fixture.store,
            &fixture.firm_home,
            "space-test",
            &fixture.node_id,
            &fixture.sender_actor(),
            &draft,
            Some(&request("node-remote", "space-remote", Some(7))),
        )
        .expect("remote resolution with caller-declared subscription revision");
        assert!(resolved.requires_remote_route);
        assert_eq!(resolved.authority.target_subscription_revision, 7);
        assert_eq!(resolved.authority.target_node_id, "node-remote");
        fixture.cleanup();
    }

