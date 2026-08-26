use super::*;

#[test]
fn source_node_authors_cross_node_message_only_with_frozen_delegation_authority() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-remote-sender", 0),
            identity("remote-sender"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "remote-sender-session", 0),
            session("session-remote-sender", "remote-sender"),
        )
        .unwrap();
    append_runtime_team(&store, "source-team", "source-team-run");
    let source_membership = store
        .team_host_membership("space-test", "source-team", true)
        .unwrap();
    let source_work = insert_runtime_work(&store, "source-work", "source-team", "source-team-run");
    let source_work = assign_runtime_work(&store, &source_work, &source_membership);
    store
        .bind_work_execution_fixture(
            &context("fixture-host", "work.bind", "source-work-binding", 0),
            WorkExecutionBinding {
                id: "source-work-binding".into(),
                work_id: source_work.id.clone(),
                work_revision: source_work.version,
                team_id: "source-team".into(),
                team_membership_id: source_membership.id,
                agent_member_id: "remote-sender".into(),
                agent_session_id: "session-remote-sender".into(),
                agent_session_generation: 1,
                delivery_id: format!("work-delivery:{}:1", source_work.id),
                binding_generation: 1,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: actor("fixture-host"),
                bound_at: "t-binding".into(),
                ended_at: None,
            },
        )
        .unwrap();
    let source_work_ref = firm_core::collaboration::RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: "space-test".into(),
        node_id: "11111111-1111-4111-8111-111111111111".into(),
        team_id: "source-team".into(),
        team_revision: 1,
        placement_generation: 1,
        work_id: source_work.id.clone(),
        work_revision: source_work.version,
        work_event_id: "source-work-event".into(),
        digest: canonical_json_fingerprint(&serde_json::to_value(&source_work).unwrap()),
    };
    let target_work_ref = firm_core::collaboration::RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: "space-target".into(),
        node_id: "22222222-2222-4222-8222-222222222222".into(),
        team_id: "target-team".into(),
        team_revision: 1,
        placement_generation: 1,
        work_id: "target-work".into(),
        work_revision: 1,
        work_event_id: "target-work-event".into(),
        digest: format!("sha256:{:064x}", 2),
    };
    let policy = firm_core::collaboration::DelegationInboundPolicySnapshot {
        policy_id: "policy-source-target".into(),
        policy_revision: 1,
        policy_digest: format!("sha256:{:064x}", 3),
        mode: firm_core::collaboration::DelegationInboundMode::HostApprovalRequired,
        allowed_outcome_classes: vec!["implementation".into()],
        max_active_delegations: 1,
    };
    let mut authority = CollaborationMessageAuthority {
        company_id: "company-test".into(),
        delegation_id: "delegation-a-b".into(),
        delegation_revision: 3,
        source_work_ref: source_work_ref.clone(),
        target_work_ref: target_work_ref.clone(),
        target_placement: firm_core::collaboration::TargetPlacementRef {
            team_id: "target-team".into(),
            team_revision: 1,
            node_id: "22222222-2222-4222-8222-222222222222".into(),
            placement_generation: 1,
        },
        source_owner_ref: ActorRef {
            kind: ActorKind::AgentMember,
            id: "remote-sender".into(),
        },
        source_host_ref: ActorRef {
            kind: ActorKind::AgentMember,
            id: "remote-sender".into(),
        },
        target_host_ref: ActorRef {
            kind: ActorKind::AgentMember,
            id: "target-host-on-another-node".into(),
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
    let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
        kind: MessageRecipientKind::AgentMember,
        id: "target-host-on-another-node".into(),
    }];
    let mut message = Message {
        id: "cross-node-message".into(),
        source_execution_space_id: "space-test".into(),
        source_node_id: "11111111-1111-4111-8111-111111111111".into(),
        source_node_daemon_id: "daemon-1".into(),
        source_authority_generation: 1,
        sender_actor_ref: ActorRef {
            kind: ActorKind::AgentMember,
            id: "remote-sender".into(),
        },
        sender_agent_member_id: Some("remote-sender".into()),
        sender_session_id: Some("session-remote-sender".into()),
        address_kind: firm_core::agentfirm_api::MessageAddressKind::DirectAgent,
        target_ref: recipients[0].clone(),
        recipients,
        team_id: Some("source-team".into()),
        team_run_id: Some("source-team-run".into()),
        work_id: Some(source_work.id.clone()),
        collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
            source_team_id: "source-team".into(),
            target_team_id: "target-team".into(),
            delegation_id: Some("delegation-a-b".into()),
            expected_delegation_revision: Some(3),
            source_work_ref: Some(source_work_ref),
            target_work_ref: Some(target_work_ref),
        }),
        kind: firm_core::agentfirm_api::MessageKind::Message,
        body: "cross-node immutable body".into(),
        body_digest: format!("sha256:{:x}", Sha256::digest(b"cross-node immutable body")),
        correlation_id: "cross-node-correlation".into(),
        causation_id: None,
        response_intent: firm_core::agentfirm_api::ResponseIntent::ResponseRequired,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: "cross-node-message".into(),
        created_at: "t2".into(),
    };
    message.content_fingerprint = message_content_fingerprint(&message);

    let before = store.canonical_operations().unwrap();
    let messages_before = store.fabric_messages("space-test").unwrap();
    let deliveries_before = store.fabric_message_deliveries("space-test").unwrap();
    store
        .author_message(
            &service_context("message.author", "cross-node-message", 0),
            message.clone(),
        )
        .expect_err("caller-built collaboration scope is not Message authority");
    assert_eq!(store.canonical_operations().unwrap(), before);
    assert_eq!(
        store.fabric_messages("space-test").unwrap(),
        messages_before
    );
    assert_eq!(
        store.fabric_message_deliveries("space-test").unwrap(),
        deliveries_before
    );
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-wrong-source", 0),
            identity("wrong-source"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "wrong-source-session", 0),
            session("session-wrong-source", "wrong-source"),
        )
        .unwrap();
    join_runtime_membership(
        &store,
        "wrong-source-membership",
        "source-team",
        "wrong-source",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    let mut wrong_source = message.clone();
    wrong_source.id = "cross-node-message-wrong-source".into();
    wrong_source.sender_actor_ref = ActorRef {
        kind: ActorKind::AgentMember,
        id: "wrong-source".into(),
    };
    wrong_source.sender_agent_member_id = Some("wrong-source".into());
    wrong_source.sender_session_id = Some("session-wrong-source".into());
    wrong_source.idempotency_key = wrong_source.id.clone();
    wrong_source.content_fingerprint = message_content_fingerprint(&wrong_source);
    let before_wrong_source = store.canonical_operations().unwrap();
    store
        .author_message_with_collaboration_authority(
            &service_context("message.author", &wrong_source.id, 0),
            wrong_source,
            Some(&authority),
        )
        .expect_err("ordinary source Team Member cannot impersonate Delegation authority");
    assert_eq!(
        store.canonical_operations().unwrap(),
        before_wrong_source,
        "hostile source actor has zero Message/Delivery side effects"
    );
    assert_eq!(
        store.fabric_messages("space-test").unwrap(),
        messages_before
    );
    assert_eq!(
        store.fabric_message_deliveries("space-test").unwrap(),
        deliveries_before
    );
    let authored = store
        .author_message_with_collaboration_authority(
            &service_context("message.author", "cross-node-message", 0),
            message.clone(),
            Some(&authority),
        )
        .expect("source Node validates frozen Delegation authority under the Store lock");
    assert_eq!(authored.projection, message);
    assert!(store
        .fabric_message_deliveries("space-test")
        .unwrap()
        .is_empty());
    fs::remove_dir_all(root).unwrap();
}
