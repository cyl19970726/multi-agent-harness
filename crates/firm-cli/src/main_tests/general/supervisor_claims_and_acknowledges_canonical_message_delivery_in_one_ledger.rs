use super::*;

    #[test]
    #[cfg(any())]
    fn supervisor_claims_and_acknowledges_canonical_message_delivery_in_one_ledger() {
        let (store, root) = temp_store("canonical-supervisor-message-delivery");
        let created = create_two_member_team_run(&store);
        let member = created.member_runs[0].clone();
        let lease = store
            .acquire_test_supervisor_lease(
                &created.team_run.id,
                "canonical-supervisor",
                std::process::id(),
                "test://canonical-supervisor",
                current_unix_ms_u64(),
                60_000,
            )
            .expect("acquire supervisor lease");
        let ledger = TeamRunLedger::new(
            &store,
            &created.team_run.id,
            &lease.supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        );
        let sender = harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Service,
            id: "test-host".into(),
        };
        store
            .create_trust_team_message_with_deliveries(
                &harness_core::agentfirm_api::MutationContext {
                    execution_space_id: "unit-test-space".into(),
                    authenticated_actor: sender.clone(),
                    authority_actor: None,
                    command_name: "test.team_message.create".into(),
                    idempotency_key: "canonical-supervisor-message".into(),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                harness_core::agentfirm_api::TeamMessage {
                    id: "canonical-supervisor-message".into(),
                    team_run_id: created.team_run.id.clone(),
                    work_id: None,
                    sender,
                    recipients: vec![harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                        id: member.agent_member_id.clone(),
                    }],
                    kind: harness_core::agentfirm_api::TeamMessageKind::Message,
                    body: "deliver through the NodeDaemon supervisor".into(),
                    correlation_id: "canonical-supervisor-correlation".into(),
                    causation_id: None,
                    response_intent: harness_core::agentfirm_api::ResponseIntent::ResponseRequired,
                    evidence_refs: Vec::new(),
                    created_at: "unix-ms:3".into(),
                },
                "unix-ms:3",
            )
            .expect("create canonical TeamMessageProjection and delivery");

        let claimed = claim_canonical_messages_for_member(&ledger, &member)
            .expect("supervisor claim")
            .pop()
            .expect("one claimed message");
        let after_claim = store
            .trust_message_deliveries("unit-test-space")
            .expect("delivery after claim")
            .into_iter()
            .find(|delivery| delivery.message_id == claimed.id)
            .expect("claimed delivery");
        assert_eq!(
            after_claim.status,
            harness_core::agentfirm_api::MessageDeliveryStatus::Claimed
        );
        assert_eq!(
            after_claim.claimed_supervisor_generation,
            Some(lease.generation)
        );

        mark_message_delivered(
            &ledger,
            &claimed,
            &member.id,
            &member.name,
            "provider-receipt-canonical",
        )
        .expect("provider receipt and acknowledgement");
        let acknowledged = store
            .trust_message_deliveries("unit-test-space")
            .expect("delivery after acknowledgement")
            .into_iter()
            .find(|delivery| delivery.message_id == claimed.id)
            .expect("acknowledged delivery");
        assert_eq!(
            acknowledged.status,
            harness_core::agentfirm_api::MessageDeliveryStatus::Acknowledged
        );
        assert_eq!(
            acknowledged.provider_receipt_id.as_deref(),
            Some("provider-receipt-canonical")
        );
        assert!(store
            .legacy_team_messages()
            .expect("legacy TeamMessages")
            .iter()
            .all(|message| message.id != claimed.id));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

