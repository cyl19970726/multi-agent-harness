use super::*;

#[test]
fn peer_team_claim_replays_exactly_and_resolved_delivery_rejects_new_claims() {
    let (store, root) = fabric_store();
    let (source_team, target_team, source_membership, _target_membership) =
        seed_peer_message_scope(&store);
    let target_subscription = store
        .fabric_message_subscriptions("space-test")
        .unwrap()
        .into_iter()
        .find(|subscription| subscription.id == "team-inbox:target-team")
        .unwrap();
    let authority = peer_authority_fixture(
        "company-test",
        &source_team,
        &source_membership,
        "remote-sender",
        "session-peer-sender",
        &target_team,
        &target_subscription,
        None,
    );
    let message = peer_message_fixture(
        "peer-claim-replay",
        &source_team,
        "remote-sender",
        "session-peer-sender",
        firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::Team,
            id: "target-team".into(),
        },
        None,
    );
    store
        .author_message_with_admission_authority(
            &service_context("message.author", "peer-claim-replay", 0),
            message.clone(),
            Some(&MessageAdmissionAuthority::PeerTeam(authority)),
        )
        .unwrap();
    let delivery = store
        .fabric_message_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.message_id == message.id)
        .unwrap();
    let claim = TeamMessageDeliveryClaim {
        claim_id: "peer-claim-exact".into(),
        team_membership_id: store
            .team_host_membership("space-test", "target-team", true)
            .unwrap()
            .id,
        membership_generation: 1,
        node_daemon_generation: 1,
        claim_expires_at: "t-expiry".into(),
    };
    let claimed = store
        .claim_team_message_delivery(
            &service_context("message.team_claim", "peer-claim-exact", 0),
            &delivery.id,
            &claim,
            "t-claim",
        )
        .unwrap();
    assert!(!claimed.replayed);
    // An exact retry returns the original result without a new operation.
    let replayed = store
        .claim_team_message_delivery(
            &service_context("message.team_claim", "peer-claim-exact", 0),
            &delivery.id,
            &claim,
            "t-claim",
        )
        .unwrap();
    assert!(replayed.replayed);
    assert_eq!(replayed.projection.version, claimed.projection.version);
    assert_eq!(
        store
            .canonical_operations()
            .unwrap()
            .iter()
            .filter(|operation| operation.event.aggregate_kind == "team_message_delivery_claim")
            .count(),
        1
    );
    // A different claim on the resolved delivery is side-effect free.
    let before = store.canonical_operations().unwrap();
    store
        .claim_team_message_delivery(
            &service_context("message.team_claim", "peer-claim-second", 0),
            &delivery.id,
            &TeamMessageDeliveryClaim {
                claim_id: "peer-claim-second".into(),
                ..claim
            },
            "t-claim-2",
        )
        .expect_err("a resolved Team delivery cannot be claimed twice");
    assert_eq!(store.canonical_operations().unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
