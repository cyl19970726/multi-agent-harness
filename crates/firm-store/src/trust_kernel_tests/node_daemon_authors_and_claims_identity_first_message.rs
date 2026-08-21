use super::*;

#[test]
fn node_daemon_authors_and_claims_identity_first_message() {
    let (store, root) = fabric_store();
    for id in ["sender", "recipient"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", &format!("identity-{id}"), 0),
                identity(id),
            )
            .unwrap();
    }
    store
        .create_agent_session(
            &service_context("session.create", "sender-session", 0),
            session("session-sender", "sender"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "recipient-session", 0),
            session("session-recipient", "recipient"),
        )
        .unwrap();

    let subscription = MessageSubscription {
        id: "direct-recipient".into(),
        subscriber_kind: MessageSubjectKind::AgentMember,
        subscriber_ref: "recipient".into(),
        execution_space_id: "space-test".into(),
        target_team_id: None,
        target_node_id: "11111111-1111-4111-8111-111111111111".into(),
        source_kind: MessageSubscriptionKind::Agent,
        source_ref: "sender".into(),
        delivery_mode: firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
        history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
        membership_ref: None,
        authorization_policy_ref: "direct.test".into(),
        policy_revision: 1,
        policy_digest: canonical_json_fingerprint(&serde_json::json!({"direct": true})),
        status: MessageSubscriptionStatus::Active,
        revision: 1,
        created_by: actor("host"),
        created_at: "t1".into(),
        revoked_at: None,
    };
    {
        let _lock = store.acquire_write_lock().unwrap();
        store
            .commit_trust_projection_unlocked(
                &context("host", "subscription.create", "subscription", 0),
                "message_subscription_set",
                "recipient",
                "created",
                serde_json::to_value(&subscription).unwrap(),
                &serde_json::json!({"recipient_agent_member_id": "recipient"}),
                vec![serde_json::to_value(&subscription).unwrap()],
                Vec::new(),
            )
            .unwrap();
    }
    let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
        kind: MessageRecipientKind::AgentMember,
        id: "recipient".into(),
    }];
    let body_digest = format!("sha256:{:x}", Sha256::digest(b"hello"));
    let fingerprint = canonical_json_fingerprint(&serde_json::json!({
        "sender_actor_ref": {"kind": "agent_member", "id": "sender"},
        "sender_agent_member_id": "sender",
        "sender_session_id": "session-sender",
        "address_kind": "direct_agent",
        "target_ref": {"kind": "agent_member", "id": "recipient"},
        "recipients": recipients,
        "team_id": null,
        "team_run_id": null,
        "work_id": null,
        "collaboration_scope": null,
        "kind": firm_core::agentfirm_api::MessageKind::Message,
        "body": "hello",
        "body_digest": body_digest,
        "correlation_id": "corr-1",
        "causation_id": null,
        "response_intent": firm_core::agentfirm_api::ResponseIntent::Informational,
        "evidence_refs": Vec::<String>::new(),
        "schema_version": 1,
        "idempotency_key": "message-1",
    }));
    let authored = store
        .author_message(
            &service_context("message.author", "message-1", 0),
            Message {
                id: "message-1".into(),
                source_execution_space_id: "space-test".into(),
                source_node_id: "11111111-1111-4111-8111-111111111111".into(),
                source_node_daemon_id: "daemon-1".into(),
                source_authority_generation: 1,
                sender_actor_ref: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: "sender".into(),
                },
                sender_agent_member_id: Some("sender".into()),
                sender_session_id: Some("session-sender".into()),
                address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
                target_ref: firm_core::agentfirm_api::MessageRecipientRef {
                    kind: MessageRecipientKind::AgentMember,
                    id: "recipient".into(),
                },
                recipients,
                team_id: None,
                team_run_id: None,
                work_id: None,
                collaboration_scope: None,
                kind: firm_core::agentfirm_api::MessageKind::Message,
                body: "hello".into(),
                body_digest,
                correlation_id: "corr-1".into(),
                causation_id: None,
                response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
                evidence_refs: Vec::new(),
                content_fingerprint: fingerprint.clone(),
                schema_version: 1,
                idempotency_key: "message-1".into(),
                created_at: "t2".into(),
            },
        )
        .unwrap();
    assert!(!authored.replayed);
    let delivery = store.fabric_message_deliveries("space-test").unwrap();
    assert_eq!(delivery.len(), 1);
    assert_eq!(delivery[0].recipient_session_id, None);

    let dispatch = store
        .claim_message_for_provider(
            &service_context("message.claim", "claim-1", 0),
            &delivery[0].id,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "claim-1",
            firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
            "t3",
        )
        .unwrap();
    assert_eq!(dispatch.projection.recipient_agent_member_id, "recipient");
    assert_eq!(
        dispatch.projection.recipient_session_id,
        "session-recipient"
    );
    assert_eq!(dispatch.projection.content_fingerprint, fingerprint);

    let operations_before = store.canonical_operations().unwrap().len();
    let stale = store
        .claim_message_for_provider(
            &service_context("message.claim", "claim-stale", 0),
            &delivery[0].id,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            0,
            "claim-stale",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t4",
        )
        .expect_err("stale daemon is fenced");
    assert!(stale.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
    assert_eq!(
        store.canonical_operations().unwrap().len(),
        operations_before
    );

    let mut reconcile_context =
        service_context("node_daemon.message_delivery.reconcile", "reconcile-1", 2);
    reconcile_context.request_fingerprint = Some(canonical_json_fingerprint(
        &serde_json::json!({"outcome":"retry_safe_failure","evidence_ref":"audit:no-provider-receipt"}),
    ));
    let reconciled = store
        .reconcile_canonical_message_delivery(
            &reconcile_context,
            &delivery[0].id,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            DeliveryReconcileOutcome::RetrySafeFailure,
            "audit:no-provider-receipt",
            "t5",
        )
        .unwrap();
    assert_eq!(
        reconciled.projection.status,
        CanonicalMessageDeliveryStatus::Queued
    );
    assert_eq!(reconciled.projection.attempt, 2);
    let replay = store
        .reconcile_canonical_message_delivery(
            &reconcile_context,
            &delivery[0].id,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            DeliveryReconcileOutcome::RetrySafeFailure,
            "audit:no-provider-receipt",
            "t5",
        )
        .unwrap();
    assert!(replay.replayed);
    fs::remove_dir_all(root).unwrap();
}
