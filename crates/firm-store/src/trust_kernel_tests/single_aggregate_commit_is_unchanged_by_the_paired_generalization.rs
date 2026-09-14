use super::*;
use crate::trust_kernel::trust_foundation::PAIRED_KEY_SEPARATOR;

/// Generalizing the paired commit primitive touched the code path EVERY
/// canonical aggregate is written through. This pins the property that makes
/// that safe: with no pair, the primitive takes the untouched `else` branch and
/// appends exactly one envelope, carrying no paired key and no invented side
/// records.
///
/// The broader parity proof is the rest of the suite: every existing
/// single-aggregate test — including the schema-fixture parity tests, which
/// compare serialized Rust output against checked-in JSON — writes through this
/// same primitive and still passes unchanged.
#[test]
fn an_unpaired_commit_appends_exactly_one_envelope() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "identity-unpaired", 0),
        "unpaired",
    );
    let before = store.trust_operation_envelopes_unlocked().unwrap().len();
    store
        .create_agent_session(
            &service_context("session.create", "session-unpaired", 0),
            session("session-unpaired", "unpaired"),
        )
        .unwrap();
    let after = store.trust_operation_envelopes_unlocked().unwrap();
    assert_eq!(
        after.len(),
        before + 1,
        "an unpaired commit appends exactly one envelope"
    );

    let appended = after.last().expect("appended envelope");
    assert_eq!(appended.operation.event.aggregate_kind, "agent_session");
    assert!(
        !appended
            .operation
            .event
            .idempotency_key
            .contains(PAIRED_KEY_SEPARATOR),
        "an unpaired commit never derives a paired idempotency key"
    );
    assert!(
        appended.operation.immutable_side_records.is_empty()
            && appended.operation.initial_outbox_records.is_empty(),
        "an unpaired commit invents no side records"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

/// A paired commit adds its second envelope and changes nothing about the
/// primary one: same aggregate, same transition, same side records, and a
/// paired key that is the primary's plus the separator and the paired
/// transition — which is exactly how every existing scan still resolves the
/// primary by its own exact key.
#[test]
fn a_paired_commit_leaves_the_primary_envelope_identical() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "identity-paired-shape", 0),
        "paired-shape",
    );
    let session = session("session-paired-shape", "paired-shape");
    store
        .create_agent_session(
            &service_context("session.create", "session-paired-shape", 0),
            session.clone(),
        )
        .unwrap();
    append_runtime_team(&store, "team-paired-shape", "team-run-paired-shape");
    join_runtime_membership(
        &store,
        "membership-paired-shape",
        "team-paired-shape",
        "paired-shape",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    admit_fixture_member_run_for_session(&store, "team-run-paired-shape", &session);

    let before = store.trust_operation_envelopes_unlocked().unwrap().len();
    store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "paired-shape-bind", 1),
            "session-paired-shape",
            1,
            settled_native_session("thread-paired-shape"),
        )
        .expect("bind and project");
    let after = store.trust_operation_envelopes_unlocked().unwrap();
    assert_eq!(
        after.len(),
        before + 2,
        "a paired commit appends exactly two envelopes"
    );

    let primary = &after[after.len() - 2];
    let paired = &after[after.len() - 1];
    assert_eq!(primary.operation.event.aggregate_kind, "agent_session");
    assert_eq!(primary.operation.event.transition, "native_session_bound");
    assert!(
        primary.operation.immutable_side_records.is_empty(),
        "the primary envelope gains no side record from being paired"
    );
    assert_eq!(paired.operation.event.aggregate_kind, "member_run");
    assert_eq!(
        paired.operation.event.transition,
        "native_session_projected"
    );
    assert!(
        paired.operation.immutable_side_records.is_empty(),
        "a projection is not a journalled transition of its own, so it carries no WorkEvent-style side record"
    );
    assert_eq!(
        paired.operation.event.idempotency_key,
        format!(
            "{}{PAIRED_KEY_SEPARATOR}native_session_projected",
            primary.operation.event.idempotency_key
        ),
        "the paired key derives from the primary's, so an exact-key scan still finds the primary"
    );
    assert_eq!(
        paired.operation.event.store_sequence,
        primary.operation.event.store_sequence + 1,
        "the pair occupies consecutive store sequences in one rewrite"
    );
    assert_eq!(
        paired.operation.event.created_at, primary.operation.event.created_at,
        "one rewrite, one timestamp: the pair cannot be dated as two decisions"
    );
    assert_eq!(
        paired.operation.event.canonical_request_fingerprint,
        primary.operation.event.canonical_request_fingerprint,
        "the pair answers one request"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
