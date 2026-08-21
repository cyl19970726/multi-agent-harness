use super::*;

#[test]
fn peer_team_authority_keeps_source_and_target_fences_distinct_then_claims_one_membership() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("operator", "identity.migrate", "peer-sender", 0),
            identity("remote-sender"),
        )
        .unwrap();
    store
        .create_agent_session(
            &service_context("session.create", "peer-sender-session", 0),
            session("session-peer-sender", "remote-sender"),
        )
        .unwrap();
    append_runtime_team(&store, "source-team", "source-peer-run");
    append_runtime_team(&store, "target-team", "target-peer-run");
    let source_team = store
        .agent_teams("space-test")
        .unwrap()
        .into_iter()
        .find(|team| team.id == "source-team")
        .unwrap();
    let target_team = store
        .agent_teams("space-test")
        .unwrap()
        .into_iter()
        .find(|team| team.id == "target-team")
        .unwrap();
    let source_membership = store
        .team_host_membership("space-test", "source-team", true)
        .unwrap();
    let target_membership = store
        .team_host_membership("space-test", "target-team", true)
        .unwrap();
    let target_subscription = store
        .fabric_message_subscriptions("space-test")
        .unwrap()
        .into_iter()
        .find(|subscription| subscription.id == "team-inbox:target-team")
        .unwrap();
    let source_policy_ref = "peer-team-message-admission.v1".to_string();
    let source_required_capability = "message.peer_team.author".to_string();
    let mut peer = PeerTeamMessageAdmissionAuthority {
        company_id: "company-test".into(),
        source_execution_space_id: "space-test".into(),
        source_team_id: source_team.id.clone(),
        source_team_revision: source_team.revision,
        source_membership_id: source_membership.id.clone(),
        source_membership_generation: source_membership.membership_generation,
        source_agent_member_id: "remote-sender".into(),
        source_session_id: "session-peer-sender".into(),
        source_session_generation: 1,
        source_node_id: source_team.node_id.clone(),
        source_node_daemon_id: "daemon-1".into(),
        source_node_daemon_generation: 1,
        target_execution_space_id: "space-test".into(),
        target_team_id: target_team.id.clone(),
        target_team_revision: target_team.revision,
        target_node_id: target_team.node_id.clone(),
        target_membership_id: None,
        target_membership_generation: None,
        target_agent_member_id: None,
        source_policy_ref,
        source_policy_revision: 1,
        source_policy_digest: String::new(),
        source_required_capability,
        target_subscription_id: target_subscription.id.clone(),
        target_subscription_revision: target_subscription.revision,
        target_authorization_policy_ref: target_subscription.authorization_policy_ref.clone(),
        target_policy_revision: target_subscription.policy_revision,
        target_policy_digest: String::new(),
        target_required_capability: "collaboration.peer_message_deliver".into(),
        authority_digest: String::new(),
    };
    peer.source_policy_digest = peer_team_source_policy_digest(&peer);
    peer.target_policy_digest = peer_team_target_policy_digest(&peer);
    assert_eq!(
        peer.target_policy_digest, target_subscription.policy_digest,
        "the frozen target policy digest is byte-equal to the durable subscription digest"
    );
    peer.authority_digest = peer_team_message_authority_digest(&peer);
    store
        .revalidate_peer_team_delivery_subscription("space-test", &peer)
        .expect("target durable subscription independently revalidates");

    let recipients = vec![firm_core::agentfirm_api::MessageRecipientRef {
        kind: MessageRecipientKind::Team,
        id: "target-team".into(),
    }];
    let mut message = Message {
        id: "peer-team-message".into(),
        source_execution_space_id: "space-test".into(),
        source_node_id: source_team.node_id,
        source_node_daemon_id: "daemon-1".into(),
        source_authority_generation: 1,
        sender_actor_ref: ActorRef {
            kind: ActorKind::AgentMember,
            id: "remote-sender".into(),
        },
        sender_agent_member_id: Some("remote-sender".into()),
        sender_session_id: Some("session-peer-sender".into()),
        address_kind: firm_core::agentfirm_api::MessageAddressKind::TeamChannel,
        target_ref: recipients[0].clone(),
        recipients,
        team_id: Some("source-team".into()),
        team_run_id: None,
        work_id: None,
        collaboration_scope: Some(firm_core::collaboration::CollaborationScope {
            source_team_id: "source-team".into(),
            target_team_id: "target-team".into(),
            delegation_id: None,
            expected_delegation_revision: None,
            source_work_ref: None,
            target_work_ref: None,
        }),
        kind: firm_core::agentfirm_api::MessageKind::Message,
        body: "ordinary peer conversation".into(),
        body_digest: format!("sha256:{:x}", Sha256::digest(b"ordinary peer conversation")),
        correlation_id: "peer-correlation".into(),
        causation_id: None,
        response_intent: firm_core::agentfirm_api::ResponseIntent::Informational,
        evidence_refs: Vec::new(),
        content_fingerprint: String::new(),
        schema_version: 1,
        idempotency_key: "peer-team-message".into(),
        created_at: "t-peer".into(),
    };
    message.content_fingerprint = message_content_fingerprint(&message);
    let before_hostile = store.canonical_operations().unwrap();
    let mut cross_wired = peer.clone();
    cross_wired.source_required_capability = "collaboration.peer_message_deliver".into();
    cross_wired.target_required_capability = "message.peer_team.author".into();
    cross_wired.authority_digest = peer_team_message_authority_digest(&cross_wired);
    store
        .author_message_with_admission_authority(
            &service_context("message.author", "peer-hostile", 0),
            message.clone(),
            Some(&MessageAdmissionAuthority::PeerTeam(cross_wired)),
        )
        .expect_err("source and target capabilities cannot be cross-wired");
    assert_eq!(store.canonical_operations().unwrap(), before_hostile);

    store
        .author_message_with_admission_authority(
            &service_context("message.author", "peer-team-message", 0),
            message.clone(),
            Some(&MessageAdmissionAuthority::PeerTeam(peer.clone())),
        )
        .unwrap();
    let delivery = store
        .fabric_message_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.recipient_kind == MessageSubjectKind::Team)
        .unwrap();
    assert_eq!(delivery.recipient_agent_member_id, None);
    assert_eq!(delivery.recipient_session_id, None);
    assert_eq!(
        delivery.subscription_revision,
        peer.target_subscription_revision
    );
    let mut remote_message = message.clone();
    remote_message.id = "peer-team-message-remote".into();
    remote_message.idempotency_key = "peer-team-message-remote".into();
    remote_message.created_at = "t-peer-remote".into();
    remote_message.content_fingerprint = message_content_fingerprint(&remote_message);
    let make_peer_operation = |message: &Message, authority: &PeerTeamMessageAdmissionAuthority| {
        let message_reference = firm_fabric::MessageReference {
            message_id: message.id.clone(),
            body_digest: message.body_digest.clone(),
            canonical_message_envelope: Some(serde_json::to_value(message).unwrap()),
            message_object_ref: None,
        };
        let payload = serde_json::json!({
            "message_reference": message_reference,
            "message_admission_authority": MessageAdmissionAuthority::PeerTeam(authority.clone()),
        });
        let body = serde_json::to_value(firm_fabric::CollaborationBusinessReference {
            business_kind: "peer_message_deliver".into(),
            required_capability: "collaboration.peer_message_deliver".into(),
            business_actor_kind: "agent_member".into(),
            business_actor_id: authority.source_agent_member_id.clone(),
            target_team_id: authority.target_team_id.clone(),
            target_team_revision: authority.target_team_revision,
            placement_generation: 1,
            expected_revision: authority.target_subscription_revision,
            payload_digest: canonical_json_fingerprint(&payload),
            payload,
        })
        .unwrap();
        firm_fabric::RoutedOperation {
            id: format!("peer-route:{}", message.id),
            company_id: authority.company_id.clone(),
            kind: firm_fabric::COLLABORATION_BUSINESS_OPERATION_KIND.into(),
            source_authority: firm_fabric::OperationSourceAuthority::Node,
            source_node_id: Some(authority.source_node_id.clone()),
            target_node_id: authority.target_node_id.clone(),
            source_gateway_generation: Some(1),
            source_node_daemon_id: Some(authority.source_node_daemon_id.clone()),
            source_node_daemon_generation: Some(authority.source_node_daemon_generation),
            control_plane_generation: 1,
            source_execution_space_id: Some(authority.source_execution_space_id.clone()),
            target_execution_space_id: Some(authority.target_execution_space_id.clone()),
            actor: firm_fabric::AuthenticatedActor {
                company_id: authority.company_id.clone(),
                actor_id: authority.source_node_id.clone(),
                actor_kind: firm_fabric::ActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: format!(
                    "{}:{}",
                    authority.source_node_daemon_id, authority.source_node_daemon_generation
                ),
                issued_at_unix_ms: 1,
                expires_at_unix_ms: 90_000,
            },
            actor_runtime_generation: Some(authority.source_session_generation),
            authorization_context: BTreeMap::from([
                ("target_team_id".into(), authority.target_team_id.clone()),
                (
                    "target_team_revision".into(),
                    authority.target_team_revision.to_string(),
                ),
                ("placement_generation".into(), "1".into()),
                (
                    "required_capability".into(),
                    "collaboration.peer_message_deliver".into(),
                ),
                ("business_actor_kind".into(), "agent_member".into()),
                (
                    "business_actor_id".into(),
                    authority.source_agent_member_id.clone(),
                ),
                (
                    "business_actor_session_id".into(),
                    authority.source_session_id.clone(),
                ),
            ]),
            idempotency_key: format!("peer-route:{}", message.id),
            ordering_key: format!("team:{}", authority.target_team_id),
            correlation_id: message.correlation_id.clone(),
            causation_id: None,
            expected_target_revision: Some(authority.target_subscription_revision),
            body_schema: firm_fabric::COLLABORATION_BUSINESS_OPERATION_SCHEMA.into(),
            body_digest: firm_fabric::json_digest(&body).unwrap(),
            body,
            priority: firm_fabric::OperationPriority::Normal,
            created_at_unix_ms: 2,
            expires_at_unix_ms: 90_000,
            protocol_version: firm_fabric::FABRIC_PROTOCOL_VERSION,
            schema_version: firm_fabric::FABRIC_SCHEMA_VERSION.into(),
            canonicalization_version: firm_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
        }
    };
    let mut target_cross_wired = peer.clone();
    target_cross_wired.source_required_capability = "collaboration.peer_message_deliver".into();
    target_cross_wired.target_required_capability = "message.peer_team.author".into();
    target_cross_wired.source_policy_digest = peer_team_source_policy_digest(&target_cross_wired);
    target_cross_wired.target_policy_digest = peer_team_target_policy_digest(&target_cross_wired);
    target_cross_wired.authority_digest = peer_team_message_authority_digest(&target_cross_wired);
    let hostile_operation = make_peer_operation(&remote_message, &target_cross_wired);
    let hostile_context = service_context("remote_message_persist", &hostile_operation.id, 0);
    let before_target_hostile = store.canonical_operations().unwrap();
    store
        .persist_remote_message(
            &hostile_context,
            &hostile_operation,
            remote_message.clone(),
            &peer.target_node_id,
            "daemon-1",
            1,
        )
        .expect_err("target persistence cannot cross-wire source and target capabilities");
    assert_eq!(store.canonical_operations().unwrap(), before_target_hostile);

    let peer_operation = make_peer_operation(&remote_message, &peer);
    let mut peer_context = service_context("remote_message_persist", &peer_operation.id, 0);
    peer_context.request_fingerprint = Some(firm_fabric::json_digest(&peer_operation).unwrap());
    store
        .persist_remote_message(
            &peer_context,
            &peer_operation,
            remote_message.clone(),
            &peer.target_node_id,
            "daemon-1",
            1,
        )
        .expect("target persists one unresolved canonical Team delivery");
    let remote_deliveries = store
        .fabric_message_deliveries("space-test")
        .unwrap()
        .into_iter()
        .filter(|candidate| candidate.message_id == remote_message.id)
        .collect::<Vec<_>>();
    assert_eq!(remote_deliveries.len(), 1);
    assert_eq!(
        remote_deliveries[0].recipient_kind,
        MessageSubjectKind::Team
    );
    assert_eq!(remote_deliveries[0].recipient_ref, peer.target_team_id);
    assert_eq!(remote_deliveries[0].resolved_team_membership_id, None);
    assert_eq!(remote_deliveries[0].recipient_agent_member_id, None);
    assert_eq!(remote_deliveries[0].recipient_session_id, None);
    assert_eq!(
        remote_deliveries[0].subscription_revision,
        peer.target_subscription_revision
    );
    store
        .migrate_legacy_agent_identity_same_id(
            &context("operator", "identity.migrate", "peer-extra", 0),
            identity("peer-extra"),
        )
        .unwrap();
    let extra_membership = TeamMembership {
        id: "target-team-extra-membership".into(),
        team_id: "target-team".into(),
        agent_member_id: "peer-extra".into(),
        node_id: peer.target_node_id.clone(),
        role: TeamMembershipRole::Member,
        state: TeamMembershipStatus::Active,
        membership_generation: 1,
        default_subscription_refs: Vec::new(),
        created_by: actor("fixture-host"),
        revision: 1,
        joined_at: "t-peer-extra".into(),
        left_at: None,
    };
    store
        .join_team_membership(
            &context(
                "fixture-host",
                "membership.join",
                "target-team-extra-membership",
                0,
            ),
            extra_membership.clone(),
        )
        .unwrap();
    let before_ambiguous_claim = store.canonical_operations().unwrap();
    store
        .claim_team_message_delivery(
            &service_context("message.team_claim", "peer-claim-ambiguous", 0),
            &delivery.id,
            &TeamMessageDeliveryClaim {
                claim_id: "peer-claim-ambiguous".into(),
                team_membership_id: target_membership.id.clone(),
                membership_generation: target_membership.membership_generation,
                node_daemon_generation: 1,
                claim_expires_at: "t-expiry".into(),
            },
            "t-claim-ambiguous",
        )
        .expect_err("a second eligible membership keeps the Team delivery queued");
    assert_eq!(
        store.canonical_operations().unwrap(),
        before_ambiguous_claim
    );
    store
        .leave_team_membership(
            &context(
                "fixture-host",
                "membership.leave",
                "target-team-extra-membership:leave",
                1,
            ),
            &extra_membership.id,
            "t-peer-extra-left",
        )
        .unwrap();
    let stale_claim = TeamMessageDeliveryClaim {
        claim_id: "peer-claim-stale".into(),
        team_membership_id: target_membership.id.clone(),
        membership_generation: target_membership.membership_generation + 1,
        node_daemon_generation: 1,
        claim_expires_at: "t-expiry".into(),
    };
    let before_stale_claim = store.canonical_operations().unwrap();
    store
        .claim_team_message_delivery(
            &service_context("message.team_claim", "peer-claim-stale", 0),
            &delivery.id,
            &stale_claim,
            "t-claim-stale",
        )
        .expect_err("stale membership generation is fenced");
    assert_eq!(store.canonical_operations().unwrap(), before_stale_claim);
    let claimed = store
        .claim_team_message_delivery(
            &service_context("message.team_claim", "peer-claim", 0),
            &delivery.id,
            &TeamMessageDeliveryClaim {
                claim_id: "peer-claim".into(),
                team_membership_id: target_membership.id,
                membership_generation: target_membership.membership_generation,
                node_daemon_generation: 1,
                claim_expires_at: "t-expiry".into(),
            },
            "t-claim",
        )
        .unwrap();
    assert_eq!(
        claimed.projection.status,
        CanonicalMessageDeliveryStatus::Routed
    );
    assert_eq!(
        claimed.projection.recipient_agent_member_id.as_deref(),
        Some("fixture-host")
    );
    fs::remove_dir_all(root).unwrap();
}
