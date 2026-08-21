use super::*;

#[test]
fn peer_team_direct_membership_target_binds_one_delivery_without_claim() {
    let (store, root) = fabric_store();
    let (source_team, target_team, source_membership, target_membership) =
        seed_peer_message_scope(&store);
    let direct_subscription = store
        .fabric_message_subscriptions("space-test")
        .unwrap()
        .into_iter()
        .find(|subscription| {
            subscription.id
                == format!(
                    "direct:{}:{}",
                    target_membership.agent_member_id, target_membership.id
                )
        })
        .expect("durable direct subscription");
    let authority = peer_authority_fixture(
        "company-test",
        &source_team,
        &source_membership,
        "remote-sender",
        "session-peer-sender",
        &target_team,
        &direct_subscription,
        Some(&target_membership),
    );
    assert_eq!(
        authority.target_policy_digest, direct_subscription.policy_digest,
        "direct target policy digest is byte-equal to the durable subscription digest"
    );
    store
        .revalidate_peer_team_delivery_subscription("space-test", &authority)
        .expect("direct target subscription independently revalidates");

    let message = peer_message_fixture(
        "peer-direct-message",
        &source_team,
        "remote-sender",
        "session-peer-sender",
        firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
            id: target_membership.agent_member_id.clone(),
        },
        None,
    );
    store
        .author_message_with_admission_authority(
            &service_context("message.author", "peer-direct-message", 0),
            message.clone(),
            Some(&MessageAdmissionAuthority::PeerTeam(authority.clone())),
        )
        .expect("same-Space direct peer authoring");
    let deliveries = store
        .fabric_message_deliveries("space-test")
        .unwrap()
        .into_iter()
        .filter(|delivery| delivery.message_id == message.id)
        .collect::<Vec<_>>();
    assert_eq!(deliveries.len(), 1, "exactly one bound delivery");
    let delivery = &deliveries[0];
    assert_eq!(delivery.recipient_kind, MessageSubjectKind::AgentMember);
    assert_eq!(
        delivery.recipient_agent_member_id.as_deref(),
        Some(target_membership.agent_member_id.as_str())
    );
    assert_eq!(
        delivery.resolved_team_membership_id.as_deref(),
        Some(target_membership.id.as_str())
    );
    assert_eq!(delivery.status, CanonicalMessageDeliveryStatus::Queued);
    assert_eq!(delivery.recipient_session_id, None);
    assert_eq!(delivery.subscription_id, direct_subscription.id);

    // A member-bound delivery is already resolved; the Team Inbox claim
    // path must reject it with zero side effects.
    let before = store.canonical_operations().unwrap();
    store
        .claim_team_message_delivery(
            &service_context("message.team_claim", "peer-direct-claim", 0),
            &delivery.id,
            &TeamMessageDeliveryClaim {
                claim_id: "peer-direct-claim".into(),
                team_membership_id: target_membership.id.clone(),
                membership_generation: target_membership.membership_generation,
                node_daemon_generation: 1,
                claim_expires_at: "t-expiry".into(),
            },
            "t-claim",
        )
        .expect_err("a member-bound delivery is not Team-claimable");
    assert_eq!(store.canonical_operations().unwrap(), before);

    // A stale target membership generation is fenced before any delivery.
    let mut stale = authority.clone();
    stale.target_membership_generation = Some(target_membership.membership_generation + 1);
    stale.source_policy_digest = peer_team_source_policy_digest(&stale);
    stale.target_policy_digest = peer_team_target_policy_digest(&stale);
    stale.authority_digest = peer_team_message_authority_digest(&stale);
    store
        .revalidate_peer_team_delivery_subscription("space-test", &stale)
        .expect_err("stale membership generation is fenced");

    // A recipient that disagrees with the frozen authority cannot author.
    let cross_wired = peer_message_fixture(
        "peer-direct-cross-wired",
        &source_team,
        "remote-sender",
        "session-peer-sender",
        firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
            id: "remote-sender".into(),
        },
        None,
    );
    let before = store.canonical_operations().unwrap();
    store
        .author_message_with_admission_authority(
            &service_context("message.author", "peer-direct-cross-wired", 0),
            cross_wired,
            Some(&MessageAdmissionAuthority::PeerTeam(authority.clone())),
        )
        .expect_err("recipient cannot diverge from the frozen direct target");
    assert_eq!(store.canonical_operations().unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
