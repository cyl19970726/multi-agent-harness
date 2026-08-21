use super::*;

#[test]
fn remote_message_persists_before_delivery_and_replays_without_route_duplication() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "remote-recipient", 0),
            identity("remote-recipient"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "remote-recipient-session", 0),
            session("session-remote-recipient", "remote-recipient"),
        )
        .unwrap();
    append_runtime_team(&store, "target-team", "target-team-run");
    let target_work = insert_runtime_work(&store, "target-work", "target-team", "target-team-run");
    join_runtime_membership(
        &store,
        "target-membership",
        "target-team",
        "remote-recipient",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    let subscription = MessageSubscription {
        id: "remote-direct-recipient".into(),
        subscriber_kind: MessageSubjectKind::AgentMember,
        subscriber_ref: "remote-recipient".into(),
        execution_space_id: "space-test".into(),
        target_team_id: None,
        target_node_id: "11111111-1111-4111-8111-111111111111".into(),
        source_kind: MessageSubscriptionKind::Agent,
        source_ref: "remote-sender".into(),
        delivery_mode: firm_core::agentfirm_api::RuntimeDispatchMode::StartIfIdle,
        history_policy: firm_core::agentfirm_api::MessageHistoryPolicy::FromJoin,
        membership_ref: None,
        authorization_policy_ref: "direct.remote.test".into(),
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
                &context("host", "subscription.create", "remote-subscription", 0),
                "message_subscription_set",
                "remote-recipient",
                "created",
                serde_json::to_value(&subscription).unwrap(),
                &serde_json::json!({"recipient_agent_member_id": "remote-recipient"}),
                vec![serde_json::to_value(&subscription).unwrap()],
                Vec::new(),
            )
            .unwrap();
    }

    let make_message = |body: &str| {
        let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
            id: "remote-recipient".into(),
        }];
        let mut message = Message {
            id: "message-remote-1".into(),
            source_execution_space_id: "space-source".into(),
            source_node_id: "22222222-2222-4222-8222-222222222222".into(),
            source_node_daemon_id: "daemon-source".into(),
            source_authority_generation: 4,
            sender_actor_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: "remote-sender".into(),
            },
            sender_agent_member_id: Some("remote-sender".into()),
            sender_session_id: Some("remote-sender-session".into()),
            address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
            target_ref: recipients[0].clone(),
            recipients,
            team_id: Some("source-team".into()),
            team_run_id: None,
            work_id: Some("source-work".into()),
            collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
                source_team_id: "source-team".into(),
                target_team_id: "target-team".into(),
                delegation_id: Some("delegation-source-target".into()),
                expected_delegation_revision: Some(3),
                source_work_ref: Some(firm_core::collaboration::RemoteWorkRef {
                    schema_version: "agentfirm.remote-work-ref.v1".into(),
                    execution_space_id: "space-source".into(),
                    node_id: "22222222-2222-4222-8222-222222222222".into(),
                    team_id: "source-team".into(),
                    team_revision: 1,
                    placement_generation: 1,
                    work_id: "source-work".into(),
                    work_revision: 1,
                    work_event_id: "source-work-event".into(),
                    digest: format!("sha256:{:064x}", 1),
                }),
                target_work_ref: Some(firm_core::collaboration::RemoteWorkRef {
                    schema_version: "agentfirm.remote-work-ref.v1".into(),
                    execution_space_id: "space-test".into(),
                    node_id: "11111111-1111-4111-8111-111111111111".into(),
                    team_id: "target-team".into(),
                    team_revision: 1,
                    placement_generation: 1,
                    work_id: target_work.id.clone(),
                    work_revision: target_work.version,
                    work_event_id: "target-work-event".into(),
                    digest: canonical_json_fingerprint(
                        &serde_json::to_value(&target_work).unwrap(),
                    ),
                }),
            }),
            kind: firm_core::agentfirm_api::MessageKind::Message,
            body: body.into(),
            body_digest: format!("sha256:{:x}", Sha256::digest(body.as_bytes())),
            correlation_id: "remote-correlation-1".into(),
            causation_id: None,
            response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
            evidence_refs: Vec::new(),
            content_fingerprint: String::new(),
            schema_version: 1,
            idempotency_key: "source-message-key-1".into(),
            created_at: "t2".into(),
        };
        message.content_fingerprint = message_content_fingerprint(&message);
        message
    };
    let make_operation = |message: &Message| {
        let scope = message.collaboration_scope.as_ref().unwrap();
        let policy = firm_core::collaboration::DelegationInboundPolicySnapshot {
            policy_id: "policy-source-target".into(),
            policy_revision: 1,
            policy_digest: format!("sha256:{:064x}", 4),
            mode: firm_core::collaboration::DelegationInboundMode::HostApprovalRequired,
            allowed_outcome_classes: vec!["implementation".into()],
            max_active_delegations: 1,
        };
        let mut authority = CollaborationMessageAuthority {
            company_id: "company-test".into(),
            delegation_id: scope.delegation_id.clone().unwrap(),
            delegation_revision: scope.expected_delegation_revision.unwrap(),
            source_work_ref: scope.source_work_ref.clone().unwrap(),
            target_work_ref: scope.target_work_ref.clone().unwrap(),
            target_placement: firm_core::collaboration::TargetPlacementRef {
                team_id: "target-team".into(),
                team_revision: 1,
                node_id: "11111111-1111-4111-8111-111111111111".into(),
                placement_generation: 1,
            },
            source_owner_ref: message.sender_actor_ref.clone(),
            source_host_ref: message.sender_actor_ref.clone(),
            target_host_ref: ActorRef {
                kind: ActorKind::AgentMember,
                id: "target-host".into(),
            },
            inbound_policy_snapshot: policy,
            authority_digest: String::new(),
        };
        authority.authority_digest = canonical_json_fingerprint(&serde_json::json!({
            "company_id": authority.company_id,
            "delegation_id": authority.delegation_id,
            "delegation_revision": authority.delegation_revision,
            "source_work_ref": authority.source_work_ref,
            "target_work_ref": authority.target_work_ref,
            "target_placement": authority.target_placement,
            "source_owner_ref": authority.source_owner_ref,
            "source_host_ref": authority.source_host_ref,
            "target_host_ref": authority.target_host_ref,
            "inbound_policy_snapshot": authority.inbound_policy_snapshot,
        }));
        let message_reference = firm_fabric::MessageReference {
            message_id: message.id.clone(),
            body_digest: message.body_digest.clone(),
            canonical_message_envelope: Some(serde_json::to_value(message).unwrap()),
            message_object_ref: None,
        };
        let payload = serde_json::json!({
            "message_reference": message_reference,
            "delegation_authority": authority,
        });
        let body = serde_json::to_value(firm_fabric::CollaborationBusinessReference {
            business_kind: "team_message_deliver".into(),
            required_capability: "collaboration.team_message_deliver".into(),
            business_actor_kind: "agent_member".into(),
            business_actor_id: "remote-sender".into(),
            target_team_id: "target-team".into(),
            target_team_revision: 1,
            placement_generation: 1,
            expected_revision: 3,
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
        })
        .unwrap();
        firm_fabric::RoutedOperation {
            id: "remote-route-1".into(),
            company_id: "company-test".into(),
            kind: firm_fabric::COLLABORATION_BUSINESS_OPERATION_KIND.into(),
            source_authority: firm_fabric::OperationSourceAuthority::Node,
            source_node_id: Some(message.source_node_id.clone()),
            target_node_id: "11111111-1111-4111-8111-111111111111".into(),
            source_gateway_generation: Some(4),
            source_node_daemon_id: Some(message.source_node_daemon_id.clone()),
            source_node_daemon_generation: Some(message.source_authority_generation),
            control_plane_generation: 2,
            source_execution_space_id: Some(message.source_execution_space_id.clone()),
            target_execution_space_id: Some("space-test".into()),
            actor: firm_fabric::AuthenticatedActor {
                company_id: "company-test".into(),
                actor_id: "remote-sender".into(),
                actor_kind: firm_fabric::ActorKind::AgentMember,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "remote-sender-session".into(),
                issued_at_unix_ms: 10,
                expires_at_unix_ms: 90_000,
            },
            actor_runtime_generation: Some(3),
            authorization_context: BTreeMap::from([
                ("target_team_id".into(), "target-team".into()),
                ("target_team_revision".into(), "1".into()),
                ("placement_generation".into(), "1".into()),
                (
                    "required_capability".into(),
                    "collaboration.team_message_deliver".into(),
                ),
                ("business_actor_kind".into(), "agent_member".into()),
                ("business_actor_id".into(), "remote-sender".into()),
            ]),
            idempotency_key: "remote-route-1".into(),
            ordering_key: "message:remote-recipient".into(),
            correlation_id: message.correlation_id.clone(),
            causation_id: None,
            expected_target_revision: Some(3),
            body_schema: firm_fabric::COLLABORATION_BUSINESS_OPERATION_SCHEMA.into(),
            body_digest: firm_fabric::json_digest(&body).unwrap(),
            body,
            priority: firm_fabric::OperationPriority::Normal,
            created_at_unix_ms: 20,
            expires_at_unix_ms: 90_000,
            protocol_version: firm_fabric::FABRIC_PROTOCOL_VERSION,
            schema_version: firm_fabric::FABRIC_SCHEMA_VERSION.into(),
            canonicalization_version: firm_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
        }
    };

    let message = make_message("remote hello");
    let operation = make_operation(&message);
    let mut no_delegation_authority = operation.clone();
    no_delegation_authority
        .body
        .get_mut("payload")
        .and_then(serde_json::Value::as_object_mut)
        .unwrap()
        .remove("delegation_authority");
    no_delegation_authority.body_digest =
        firm_fabric::json_digest(&no_delegation_authority.body).unwrap();
    let mut rejected_context =
        service_context("remote_message_persist", &no_delegation_authority.id, 0);
    rejected_context.request_fingerprint =
        Some(firm_fabric::json_digest(&no_delegation_authority).unwrap());
    let operations_before_reject = store.canonical_operations().unwrap();
    let messages_before_reject = store.fabric_messages("space-test").unwrap();
    let deliveries_before_reject = store.fabric_message_deliveries("space-test").unwrap();
    store
        .persist_remote_message(
            &rejected_context,
            &no_delegation_authority,
            message.clone(),
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
        )
        .expect_err("target application requires the frozen Delegation authority");
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_reject
    );
    assert_eq!(
        store.fabric_messages("space-test").unwrap(),
        messages_before_reject
    );
    assert_eq!(
        store.fabric_message_deliveries("space-test").unwrap(),
        deliveries_before_reject
    );
    let mut persist_context = service_context("remote_message_persist", &operation.id, 0);
    persist_context.request_fingerprint = Some(firm_fabric::json_digest(&operation).unwrap());
    let before = store.canonical_operations().unwrap().len();
    let first = store
        .persist_remote_message(
            &persist_context,
            &operation,
            message.clone(),
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
        )
        .unwrap();
    assert!(!first.replayed);
    assert_eq!(store.canonical_operations().unwrap().len(), before + 1);
    let deliveries = store.fabric_message_deliveries("space-test").unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].message_id, message.id);
    assert_eq!(
        deliveries[0].recipient_agent_member_id.as_deref(),
        Some("remote-recipient")
    );

    let replay = store
        .persist_remote_message(
            &persist_context,
            &operation,
            message,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(store.canonical_operations().unwrap().len(), before + 1);
    assert_eq!(
        store.fabric_message_deliveries("space-test").unwrap(),
        deliveries
    );

    let hostile_message = make_message("rewritten remote body");
    let hostile_operation = make_operation(&hostile_message);
    let mut hostile_context = persist_context;
    hostile_context.request_fingerprint =
        Some(firm_fabric::json_digest(&hostile_operation).unwrap());
    let hostile = store
        .persist_remote_message(
            &hostile_context,
            &hostile_operation,
            hostile_message,
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
        )
        .expect_err("same route id cannot rewrite an immutable Message");
    assert!(hostile.to_string().contains("IDEMPOTENCY_KEY_REUSED"));
    assert_eq!(store.canonical_operations().unwrap().len(), before + 1);
    assert_eq!(
        store.fabric_message_deliveries("space-test").unwrap(),
        deliveries
    );
    fs::remove_dir_all(root).unwrap();
}
