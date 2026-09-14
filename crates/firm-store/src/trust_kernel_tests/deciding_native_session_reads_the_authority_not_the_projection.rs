use super::*;

/// The readers that DECIDE from this pointer read the AgentSession when one
/// owns it, not whichever copy is nearest.
///
/// Proven by making the two disagree in the only direction the Store allows:
/// the projection is seeded first (a `requested` pointer), the authority binds
/// a value, and the reader must report the authority's.
#[test]
fn deciding_native_session_prefers_the_agent_session_over_the_projection() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "identity-deciding", 0),
        "deciding",
    );
    let session = session("session-deciding", "deciding");
    store
        .create_agent_session(
            &service_context("session.create", "session-deciding", 0),
            session.clone(),
        )
        .unwrap();

    let stale_projection = settled_native_session("thread-stale-projection");
    let authority = settled_native_session("thread-authority");

    // No AgentSession owns a pointer yet: the projection IS the answer, and it
    // is the `requested` pointer (#845 pre-Open attachment, and every
    // external_interactive Host, which never gets an AgentSession at all).
    let requested = store
        .deciding_native_session(
            "space-test",
            "deciding",
            session.runtime_generation,
            Some(&stale_projection),
        )
        .expect("resolve the requested pointer");
    assert_eq!(
        requested.as_ref(),
        Some(&stale_projection),
        "before a session owns one, the MemberRun pointer is the requested answer"
    );

    // Bind the authority.
    store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "deciding-bind", 1),
            "session-deciding",
            1,
            authority.clone(),
        )
        .expect("bind the authority");

    let decided = store
        .deciding_native_session(
            "space-test",
            "deciding",
            session.runtime_generation,
            Some(&stale_projection),
        )
        .expect("resolve the authoritative pointer");
    assert_eq!(
        decided.as_ref(),
        Some(&authority),
        "once an AgentSession owns the pointer, a stale projection cannot answer"
    );

    // A different generation is a different lane; the authority of one lane
    // never answers for another.
    let other_generation = store
        .deciding_native_session("space-test", "deciding", 99, Some(&stale_projection))
        .expect("resolve another generation");
    assert_eq!(
        other_generation.as_ref(),
        Some(&stale_projection),
        "the authority is per member AND runtime generation"
    );

    // Nothing anywhere is honestly nothing.
    let nothing = store
        .deciding_native_session("space-test", "unknown-member", 1, None)
        .expect("resolve an unknown member");
    assert_eq!(nothing, None);

    std::fs::remove_dir_all(root).expect("cleanup");
}
