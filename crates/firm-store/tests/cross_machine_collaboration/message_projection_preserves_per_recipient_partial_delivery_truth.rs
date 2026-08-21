use super::*;

#[test]
fn message_projection_preserves_per_recipient_partial_delivery_truth() {
    let recipients = vec![
        MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
            id: "member-b1".into(),
        },
        MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
            id: "member-b2".into(),
        },
    ];
    let mut message = Message {
        id: "message-1".into(),
        source_execution_space_id: "space-node-a".into(),
        source_node_id: "node-a".into(),
        source_node_daemon_id: "daemon-a".into(),
        source_authority_generation: 8,
        sender_actor_ref: actor(ActorKind::AgentMember, "host-a"),
        sender_agent_member_id: Some("host-a".into()),
        sender_session_id: Some("session-host-a".into()),
        address_kind: MessageAddressKind::DirectAgent,
        target_ref: recipients[0].clone(),
        recipients,
        team_id: Some("team-a".into()),
        team_run_id: None,
        work_id: Some("work-a".into()),
        collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
            source_team_id: "team-a".into(),
            target_team_id: "team-b".into(),
            delegation_id: Some("delegation-1".into()),
            expected_delegation_revision: Some(3),
            source_work_ref: Some(work_ref("node-a", "team-a", "work-a", 9)),
            target_work_ref: Some(work_ref("node-b", "team-b", "work-b", 1)),
        }),
        kind: MessageKind::Message,
        body: "Please review the delegated result".into(),
        body_digest: canonical_json_fingerprint(&serde_json::json!({
            "body": "Please review the delegated result"
        })),
        correlation_id: "correlation-1".into(),
        causation_id: None,
        response_intent: ResponseIntent::ResponseRequired,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: "message-1".into(),
        created_at: "2026-08-11T00:00:00Z".into(),
    };
    message.content_fingerprint = message_fingerprint(&message);
    validate_message_collaboration_scope(&message).expect("exact source/target scope");
    let mut forged_scope = message.clone();
    forged_scope
        .collaboration_scope
        .as_mut()
        .unwrap()
        .target_team_id = "team-a".into();
    assert!(validate_message_collaboration_scope(&forged_scope).is_err());
    let replica = persisted_replica(&message);
    let deliveries = vec![
        canonical_delivery(
            "delivery-1",
            "member-b1",
            CanonicalMessageDeliveryStatus::ProviderReceived,
        ),
        canonical_delivery(
            "delivery-2",
            "member-b2",
            CanonicalMessageDeliveryStatus::Queued,
        ),
    ];
    let projections = project_cross_node_deliveries(
        &message,
        &replica,
        &deliveries,
        "route-1",
        Some(9),
        44,
        "2026-08-11T00:00:01Z",
    )
    .expect("project independent recipient states");
    assert_eq!(projections.len(), 2);
    assert_eq!(
        projections[0].state,
        CanonicalMessageDeliveryStatus::ProviderReceived
    );
    assert_eq!(projections[1].state, CanonicalMessageDeliveryStatus::Queued);

    let mut duplicate = deliveries.clone();
    duplicate[1].recipient_agent_member_id = Some("member-b1".into());
    assert!(project_cross_node_deliveries(
        &message,
        &replica,
        &duplicate,
        "route-1",
        Some(9),
        44,
        "2026-08-11T00:00:01Z",
    )
    .is_err());

    let team_recipient = MessageRecipientRef {
        kind: MessageRecipientKind::Team,
        id: "team-b".into(),
    };
    let team_message = Message {
        target_ref: team_recipient.clone(),
        recipients: vec![team_recipient],
        ..message.clone()
    };
    let team_replica = persisted_replica(&team_message);
    let mut team_delivery = canonical_delivery(
        "delivery-team-1",
        "member-b1",
        CanonicalMessageDeliveryStatus::Routed,
    );
    team_delivery.recipient_kind = firm_core::agentfirm_api::MessageSubjectKind::Team;
    team_delivery.recipient_ref = "team-b".into();
    team_delivery.target_team_id = Some("team-b".into());
    team_delivery.resolved_team_membership_id = Some("membership-team-b-member-b1".into());
    assert_eq!(
        project_cross_node_deliveries(
            &team_message,
            &team_replica,
            std::slice::from_ref(&team_delivery),
            "route-team-1",
            Some(9),
            45,
            "2026-08-11T00:00:02Z",
        )
        .expect("one resolved Team-subject delivery projects one selected membership")
        .len(),
        1
    );

    let mut duplicate_team_delivery = team_delivery.clone();
    duplicate_team_delivery.id = "delivery-team-duplicate".into();
    duplicate_team_delivery.target_node_id = "node-c".into();
    let mixed_nodes = vec![team_delivery, duplicate_team_delivery];
    assert!(project_cross_node_deliveries(
        &team_message,
        &team_replica,
        &mixed_nodes,
        "route-team-1",
        Some(9),
        45,
        "2026-08-11T00:00:02Z",
    )
    .is_err());
}
