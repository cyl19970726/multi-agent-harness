use super::*;

    #[test]
    fn team_inbox_projection_lists_queued_then_all_with_claim_binding() {
        use sha2::Digest;
        let fixture = PeerMessagingFixture::new("peer-inbox-projection");
        let draft = fixture.team_draft("hello peer team");
        let resolved = resolve_peer_team_message_admission_authority(
            &fixture.store,
            &fixture.firm_home,
            "space-test",
            &fixture.node_id,
            &fixture.sender_actor(),
            &draft,
            None,
        )
        .expect("local peer resolution");
        let recipient = harness_core::agentfirm_api::MessageRecipientRef {
            kind: harness_core::agentfirm_api::MessageRecipientKind::Team,
            id: fixture.target_team_id.clone(),
        };
        let mut message = harness_core::agentfirm_api::Message {
            id: "message:peer-inbox".into(),
            source_execution_space_id: "space-test".into(),
            source_node_id: fixture.node_id.clone(),
            source_node_daemon_id: "daemon-1".into(),
            source_authority_generation: 1,
            sender_actor_ref: fixture.sender_actor(),
            sender_agent_member_id: Some(fixture.sender_member_id.clone()),
            sender_session_id: Some("session-sender".into()),
            address_kind: harness_core::agentfirm_api::MessageAddressKind::TeamChannel,
            target_ref: recipient.clone(),
            recipients: vec![recipient],
            team_id: Some(fixture.source_team_id.clone()),
            team_run_id: None,
            work_id: None,
            collaboration_scope: draft.collaboration_scope.clone(),
            kind: harness_core::agentfirm_api::MessageKind::Message,
            body: draft.body.clone(),
            body_digest: String::new(),
            correlation_id: draft.correlation_id.clone(),
            causation_id: None,
            response_intent: harness_core::agentfirm_api::ResponseIntent::Informational,
            evidence_refs: Vec::new(),
            content_fingerprint: String::new(),
            schema_version: 1,
            idempotency_key: "peer-inbox".into(),
            created_at: "t-peer-inbox".into(),
        };
        message.body_digest = format!("sha256:{:x}", sha2::Sha256::digest(message.body.as_bytes()));
        message.content_fingerprint = harness_store::message_content_fingerprint(&message);
        fixture
            .store
            .author_message_with_admission_authority(
                &harness_core::agentfirm_api::MutationContext {
                    execution_space_id: "space-test".into(),
                    authenticated_actor: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: "daemon-1".into(),
                    },
                    authority_actor: None,
                    command_name: "unit_test.message.author".into(),
                    idempotency_key: "peer-inbox".into(),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                message.clone(),
                Some(
                    &harness_core::collaboration::MessageAdmissionAuthority::PeerTeam(
                        resolved.authority.clone(),
                    ),
                ),
            )
            .expect("author peer message");

        // One queued Team Inbox delivery; the Member fan-out never happens.
        let inbox = team_inbox_projection(&fixture.store, "space-test", "target-team", false)
            .expect("team inbox projection");
        assert_eq!(inbox["item_count"], serde_json::json!(1));
        let item = &inbox["items"][0];
        assert_eq!(item["delivery_status"], "queued");
        assert_eq!(item["resolved_team_membership_id"], serde_json::Value::Null);
        assert_eq!(item["message"]["sender_agent_member_id"], "sender-member");
        assert_eq!(
            item["message"]["collaboration_scope"]["source_team_id"],
            "source-team"
        );
        assert_eq!(
            item["message"]["collaboration_scope"]["target_team_id"],
            "target-team"
        );
        assert_eq!(item["message"]["correlation_id"], "correlation-test");
        assert_eq!(item["message"]["body"], "hello peer team");

        // Claim binds one exact membership generation; the read view then
        // shows the binding without implying provider receipt or acceptance.
        let target_membership = fixture
            .store
            .fabric_team_memberships("space-test")
            .expect("memberships")
            .into_iter()
            .find(|membership| {
                membership.team_id == "target-team"
                    && membership.state == harness_core::agentfirm_api::TeamMembershipStatus::Active
            })
            .expect("active target membership");
        let claimed = fixture
            .store
            .claim_team_message_delivery(
                &canonical_delivery_context(
                    "space-test",
                    "daemon-1",
                    "node_daemon.team_message.claim",
                    "peer-inbox-claim".into(),
                    0,
                ),
                item["delivery_id"].as_str().expect("delivery id"),
                &harness_core::agentfirm_api::TeamMessageDeliveryClaim {
                    claim_id: "peer-inbox-claim".into(),
                    team_membership_id: target_membership.id.clone(),
                    membership_generation: target_membership.membership_generation,
                    node_daemon_generation: 1,
                    claim_expires_at: "t-expiry".into(),
                },
                "t-claim",
            )
            .expect("claim team inbox delivery");
        assert!(!claimed.replayed);
        let inbox = team_inbox_projection(&fixture.store, "space-test", "target-team", false)
            .expect("queued-only projection");
        assert_eq!(
            inbox["item_count"],
            serde_json::json!(0),
            "claimed deliveries leave the actionable queue"
        );
        let inbox = team_inbox_projection(&fixture.store, "space-test", "target-team", true)
            .expect("full projection");
        assert_eq!(inbox["item_count"], serde_json::json!(1));
        let item = &inbox["items"][0];
        assert_eq!(item["delivery_status"], "routed");
        assert_eq!(
            item["resolved_team_membership_id"].as_str(),
            Some(target_membership.id.as_str())
        );
        assert_eq!(
            item["recipient_agent_member_id"].as_str(),
            Some(target_membership.agent_member_id.as_str())
        );
        fixture.cleanup();
    }

