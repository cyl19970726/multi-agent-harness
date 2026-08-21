use super::*;

#[test]
fn immutable_message_transfer_persists_exact_replica_before_delivery_and_replays() {
    let recipients = vec![MessageRecipientRef {
        kind: MessageRecipientKind::AgentMember,
        id: "member-b1".into(),
    }];
    let mut message = Message {
        id: "remote-message-1".into(),
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
        body: "immutable remote body".into(),
        body_digest: format!(
            "sha256:{}",
            firm_fabric::sha256_hex(b"immutable remote body")
        ),
        correlation_id: "remote-correlation-1".into(),
        causation_id: None,
        response_intent: ResponseIntent::ResponseRequired,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: "remote-message-1".into(),
        created_at: "2026-08-11T00:00:00Z".into(),
    };
    message.content_fingerprint = message_fingerprint(&message);
    let bytes = serde_json::to_vec(&message).unwrap();
    let port = FaithfulReplicaStore::default();
    let inline = ImmutableMessageTransferPayload::CanonicalBytes {
        canonical_message_bytes: bytes.clone(),
    };
    let queued = queue_remote_message_transfer(
        &message,
        &placement(13),
        inline.clone(),
        "2026-08-11T00:00:00Z",
    )
    .expect("source Node queues the already-authored Message while Control Plane is offline");
    assert_eq!(
        queued.state,
        RemoteMessageTransferState::QueuedForControlPlane
    );
    assert_eq!(queued.message_id, message.id);
    let expectation = |persisted_at: &str| RemoteMessageReplicaExpectation {
        source_execution_space_id: message.source_execution_space_id.clone(),
        message_id: message.id.clone(),
        schema_version: message.schema_version,
        content_fingerprint: message.content_fingerprint.clone(),
        body_digest: message.body_digest.clone(),
        persisted_at: persisted_at.into(),
    };
    let first = persist_verified_remote_message_replica(
        &port,
        &inline,
        &expectation("2026-08-11T00:00:01Z"),
    )
    .expect("target persists exact inline remote replica");
    let replay = persist_verified_remote_message_replica(
        &port,
        &inline,
        &expectation("2026-08-11T00:00:02Z"),
    )
    .expect("same Message bytes replay the original target replica");
    assert_eq!(first, replay);
    assert_eq!(port.replicas.lock().unwrap().len(), 1);

    let object_ref = "message-object:remote-message-1";
    let object_digest =
        canonical_json_fingerprint(&serde_json::from_slice::<serde_json::Value>(&bytes).unwrap());
    port.objects
        .lock()
        .unwrap()
        .insert(object_ref.into(), bytes.clone());
    let referenced = ImmutableMessageTransferPayload::MessageObjectRef {
        message_object_ref: object_ref.into(),
        authenticated_content_digest: object_digest,
    };
    assert!(persist_verified_remote_message_replica(
        &port,
        &referenced,
        &expectation("2026-08-11T00:00:03Z"),
    )
    .is_ok());

    let before = port.replicas.lock().unwrap().clone();
    let mut forged = message.clone();
    forged.body = "forged body".into();
    let forged_payload = ImmutableMessageTransferPayload::CanonicalBytes {
        canonical_message_bytes: serde_json::to_vec(&forged).unwrap(),
    };
    assert!(persist_verified_remote_message_replica(
        &port,
        &forged_payload,
        &expectation("2026-08-11T00:00:04Z"),
    )
    .is_err());
    assert_eq!(*port.replicas.lock().unwrap(), before);

    let deliveries = vec![CanonicalMessageDelivery {
        message_id: message.id.clone(),
        ..canonical_delivery(
            "remote-delivery-1",
            "member-b1",
            CanonicalMessageDeliveryStatus::Queued,
        )
    }];
    assert_eq!(
        project_cross_node_deliveries(
            &message,
            &first,
            &deliveries,
            "route-remote-1",
            Some(9),
            45,
            "2026-08-11T00:00:05Z",
        )
        .expect("delivery projection is derived only after replica persistence")
        .len(),
        1
    );
}
