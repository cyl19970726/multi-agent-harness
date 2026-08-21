use super::*;

#[test]
#[cfg(any())]
fn linked_team_messages_reject_unknown_and_cross_team_work_without_side_effects() {
    let harness = TestStore::new("linked-message-scope");
    let host = human("host");
    let team_run = seed_team(&harness.store, "linked-source", &["member-a", "member-b"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "run-a",
        false,
    );
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-b",
        "run-b",
        false,
    );
    seed_team_work(&harness.store, "linked-other-team", "other-team-work");
    let before = harness.store.canonical_operations().unwrap().len();
    for (id, work_id) in [
        ("unknown-link", "missing-work"),
        ("cross-team-link", "other-team-work"),
    ] {
        let mut linked = message(id, &team_run.id, &member_actor("member-a"), &["member-b"]);
        linked.work_id = Some(work_id.into());
        assert!(harness
            .store
            .create_trust_team_message_with_deliveries(
                &context(member_actor("member-a"), "message.create", id, 0),
                linked,
                "t4",
            )
            .is_err());
    }
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before,
        "unknown/cross-Team Work linkage must have zero canonical side effects"
    );
}
