use super::*;

#[test]
fn idempotency_is_scoped_payload_exact_and_cas_protected() {
    let harness = TestStore::new("idempotency-cas");
    let host = human("host");
    let request = member("member-a", &host);
    let create = context(host.clone(), "member.create", "same-key", 0);

    let first = harness
        .store
        .create_trust_agent_member(&create, request.clone())
        .expect("first create");
    let replay = harness
        .store
        .create_trust_agent_member(&create, request.clone())
        .expect("exact replay");
    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(first.event.id, replay.event.id);
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 1);

    let mut drifted = request.clone();
    drifted.description = "different payload".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_agent_member(&create, drifted)
                .expect_err("payload drift must fail")
        ),
        TrustErrorCode::IdempotencyKeyReused
    );

    let wrong_cas = context(host.clone(), "member.pause", "pause-wrong-cas", 99);
    assert_eq!(
        trust_code(
            harness
                .store
                .transition_trust_agent_member(
                    &wrong_cas,
                    "member-a",
                    AgentMemberOrganizationStatus::Paused,
                    "t2",
                )
                .expect_err("stale CAS must fail")
        ),
        TrustErrorCode::VersionConflict
    );

    let other_actor = human("other-host");
    harness
        .store
        .create_trust_agent_member(
            &context(other_actor.clone(), "member.create", "same-key", 0),
            member("member-b", &other_actor),
        )
        .expect("same key is scoped by actor");
    let mut other_space = context(host.clone(), "member.create", "same-key", 0);
    other_space.execution_space_id = "space-other".into();
    harness
        .store
        .create_trust_agent_member(&other_space, member("member-c", &host))
        .expect("same key is scoped by execution space");
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 3);
}
