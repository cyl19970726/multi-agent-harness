use super::*;

/// Binding the authority also projects it, in ONE atomic ledger rewrite.
///
/// Before ADR 0071 the same pointer was written by three calls in three
/// separate transactions, ledger row first, so a projection could be observed
/// without its authority — or outlive an authority write that never landed.
#[test]
fn native_session_bind_projects_both_copies_atomically() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "identity-project-both", 0),
        "project-both",
    );
    let session = session("session-project-both", "project-both");
    store
        .create_agent_session(
            &service_context("session.create", "session-project-both", 0),
            session.clone(),
        )
        .unwrap();
    append_runtime_team(&store, "team-project", "team-run-project");
    join_runtime_membership(
        &store,
        "membership-project-both",
        "team-project",
        "project-both",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    let run_id = admit_fixture_member_run_for_session(&store, "team-run-project", &session);

    // The MemberRun starts with no pointer: the provider thread does not exist
    // when a fresh-start run is materialized.
    let before = store
        .trust_member_runs("space-test")
        .unwrap()
        .into_iter()
        .find(|run| run.id == run_id)
        .expect("fixture MemberRun");
    assert_eq!(before.native_session, None);

    let native = settled_native_session("thread-project-both");
    store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "project-both-bind", 1),
            "session-project-both",
            1,
            native.clone(),
        )
        .expect("the authority binds and projects in one transaction");

    let bound_session = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == "session-project-both")
        .expect("bound AgentSession");
    assert_eq!(
        bound_session.native_session_ref.as_ref(),
        Some(&native),
        "the AgentSession is the authority and holds the exact bound value"
    );

    let projected = store
        .trust_member_runs("space-test")
        .unwrap()
        .into_iter()
        .find(|run| run.id == run_id)
        .expect("projected MemberRun");
    assert_eq!(
        projected.native_session.as_ref(),
        Some(&native),
        "the MemberRun projects the authority value, not one of its own"
    );
    assert_eq!(
        projected.version,
        before.version + 1,
        "the projection advances exactly one revision"
    );

    // The legacy runtime row is projected under the same write lock.
    let legacy = store
        .latest_member_runs()
        .unwrap()
        .into_iter()
        .find(|row| row.id == run_id)
        .expect("legacy runtime row");
    assert_eq!(
        legacy.native_session.as_ref(),
        Some(&native),
        "the member_runs.jsonl copy projects the same authority value"
    );

    // One rewrite, so both envelopes share the store sequence neighbourhood and
    // neither exists without the other.
    let space_envelopes = store.trust_operation_envelopes_unlocked().unwrap();
    let session_bind = space_envelopes
        .iter()
        .filter(|envelope| {
            envelope.operation.event.aggregate_kind == "agent_session"
                && envelope.operation.event.transition == "native_session_bound"
        })
        .count();
    let run_projection = space_envelopes
        .iter()
        .filter(|envelope| {
            envelope.operation.event.aggregate_kind == "member_run"
                && envelope.operation.event.transition == "native_session_projected"
        })
        .count();
    assert_eq!(
        (session_bind, run_projection),
        (1, 1),
        "exactly one authority envelope and exactly one projection envelope"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

/// A projection that would disagree with the authority is refused, and the
/// refusal leaves NEITHER record written — the whole point of one rewrite.
#[test]
fn a_disagreeing_projection_is_refused_without_a_partial_write() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "identity-disagree", 0),
        "disagree",
    );
    let session = session("session-disagree", "disagree");
    store
        .create_agent_session(
            &service_context("session.create", "session-disagree", 0),
            session.clone(),
        )
        .unwrap();
    append_runtime_team(&store, "team-disagree", "team-run-disagree");
    join_runtime_membership(
        &store,
        "membership-disagree",
        "team-disagree",
        "disagree",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    let run_id = admit_fixture_member_run_for_session(&store, "team-run-disagree", &session);

    // The real shape this guards: a `--resume-member` seed lands a REQUESTED
    // pointer on the MemberRun before any session owns one, and the provider
    // then opens a different native session.
    let seeded_run = store
        .trust_member_runs("space-test")
        .unwrap()
        .into_iter()
        .find(|run| run.id == run_id)
        .expect("fixture MemberRun");
    store
        .bind_member_run_native_session(
            &context(
                "host",
                "member_run.native.seed",
                "disagree-seed",
                seeded_run.version,
            ),
            &run_id,
            1,
            settled_native_session("thread-seeded-other"),
            "t-seed",
        )
        .expect("a requested pointer may be seeded before any session owns one");

    let run_before = store
        .trust_member_runs("space-test")
        .unwrap()
        .into_iter()
        .find(|run| run.id == run_id)
        .expect("fixture MemberRun");
    let session_before = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == "session-disagree")
        .expect("fixture AgentSession");

    let conflicting = settled_native_session("thread-authority");
    let refused = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "disagree-bind", 1),
            "session-disagree",
            1,
            conflicting,
        )
        .expect_err("a projection that names another native session is refused");
    assert!(
        refused.to_string().contains("NATIVE_SESSION_PROJECTION_DISAGREES"),
        "{refused}"
    );

    // The refusal happens BEFORE the rewrite, so the authority was not written
    // either: no partial write, nothing to reconcile by hand.
    let run_after = store
        .trust_member_runs("space-test")
        .unwrap()
        .into_iter()
        .find(|run| run.id == run_id)
        .expect("MemberRun still readable");
    let session_after = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == "session-disagree")
        .expect("AgentSession still readable");
    assert_eq!(
        run_after, run_before,
        "the projection is untouched by the refused bind"
    );
    assert_eq!(
        session_after, session_before,
        "the authority is untouched by the refused bind: no partial write"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

/// The projection cannot land on a MemberRun that is no longer active, and that
/// refusal is also all-or-nothing.
#[test]
fn a_closed_member_run_refuses_the_projection_and_the_authority_write() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "identity-closed", 0),
        "closed-run",
    );
    let session = session("session-closed-run", "closed-run");
    store
        .create_agent_session(
            &service_context("session.create", "session-closed-run", 0),
            session.clone(),
        )
        .unwrap();
    append_runtime_team(&store, "team-closed", "team-run-closed");
    join_runtime_membership(
        &store,
        "membership-closed-run",
        "team-closed",
        "closed-run",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    let run_id = admit_fixture_member_run_for_session(&store, "team-run-closed", &session);

    let active = store
        .trust_member_runs("space-test")
        .unwrap()
        .into_iter()
        .find(|run| run.id == run_id)
        .expect("fixture MemberRun");
    store
        .transition_current_team_member_lifecycle(
            &context(
                "host",
                "member_run.close",
                &format!("close-{run_id}"),
                active.version,
            ),
            &run_id,
            CurrentTeamMemberLifecycleTransition::Close,
            "t-closed",
        )
        .expect("close the fixture MemberRun");

    let refused = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "closed-run-bind", 1),
            "session-closed-run",
            1,
            settled_native_session("thread-closed-run"),
        )
        .expect_err("a closed MemberRun cannot acquire a projection");
    assert!(
        refused.to_string().contains("only an active MemberRun"),
        "{refused}"
    );
    let session_after = store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == "session-closed-run")
        .expect("AgentSession still readable");
    assert_eq!(
        session_after.native_session_ref, None,
        "the authority write is refused with the projection, not left behind"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}
