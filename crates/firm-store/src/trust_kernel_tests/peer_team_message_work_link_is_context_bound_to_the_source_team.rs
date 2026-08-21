use super::*;

#[test]
fn peer_team_message_work_link_is_context_bound_to_the_source_team() {
    let (store, root) = fabric_store();
    let (source_team, target_team, source_membership, _target_membership) =
        seed_peer_message_scope(&store);
    let work = insert_runtime_work(&store, "work-context-1", "source-team", "source-peer-run");
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
        "peer-work-context",
        &source_team,
        "remote-sender",
        "session-peer-sender",
        firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::Team,
            id: "target-team".into(),
        },
        Some(&work.id),
    );
    store
        .author_message_with_admission_authority(
            &service_context("message.author", "peer-work-context", 0),
            message.clone(),
            Some(&MessageAdmissionAuthority::PeerTeam(authority.clone())),
        )
        .expect("a context-only Work link of the source Team is preserved");
    let stored = store
        .fabric_messages("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == message.id)
        .unwrap();
    assert_eq!(stored.work_id.as_deref(), Some(work.id.as_str()));
    // The Work itself is untouched: no operation, delivery, or phase change.
    assert_eq!(
        store
            .latest_works()
            .unwrap()
            .into_iter()
            .find(|w| w.id == work.id)
            .map(|w| w.version),
        Some(work.version)
    );

    let foreign = peer_message_fixture(
        "peer-work-context-foreign",
        &source_team,
        "remote-sender",
        "session-peer-sender",
        firm_core::agentfirm_api::MessageRecipientRef {
            kind: MessageRecipientKind::Team,
            id: "target-team".into(),
        },
        Some("work-that-does-not-exist"),
    );
    let before = store.canonical_operations().unwrap();
    store
        .author_message_with_admission_authority(
            &service_context("message.author", "peer-work-context-foreign", 0),
            foreign,
            Some(&MessageAdmissionAuthority::PeerTeam(authority)),
        )
        .expect_err("a Work link must name a current Work of the source Team");
    assert_eq!(store.canonical_operations().unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
