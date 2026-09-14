use super::*;

/// `MemberRun.native_session` means two different things, and a reader must be
/// able to tell them apart without guessing: **requested** before any
/// AgentSession owns the pointer, **a projection of the authority** after.
///
/// The rule is exact — requested iff no AgentSession for this member at this
/// runtime generation owns a `native_session_ref` — so it is a function with a
/// test rather than a sentence in an ADR.
#[test]
fn requested_is_decided_by_whether_an_agent_session_owns_the_pointer() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "identity-requested", 0),
        "requested-member",
    );
    let session = session("session-requested", "requested-member");
    store
        .create_agent_session(
            &service_context("session.create", "session-requested", 0),
            session.clone(),
        )
        .unwrap();
    append_runtime_team(&store, "team-requested", "team-run-requested");
    join_runtime_membership(
        &store,
        "membership-requested",
        "team-requested",
        "requested-member",
        firm_core::agentfirm_api::TeamMembershipRole::Member,
    );
    let run_id = admit_fixture_member_run_for_session(&store, "team-run-requested", &session);

    // Nothing bound yet: requested.
    assert!(
        store
            .member_run_native_session_is_requested(
                "space-test",
                "requested-member",
                session.runtime_generation
            )
            .unwrap(),
        "with no AgentSession pointer, the MemberRun copy is a requested intent"
    );

    // Seed the pointer a `--resume-member` asked for. This is #845 pre-Open
    // attachment: the MemberRun now carries a pointer and it is STILL requested,
    // because no session has observed the provider-native conversation.
    let seeded = store
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
                "requested-seed",
                seeded.version,
            ),
            &run_id,
            1,
            requested_resume_seed(),
            "t-seed",
        )
        .expect("a resume seed lands before any session owns a pointer");
    assert!(
        store
            .member_run_native_session_is_requested(
                "space-test",
                "requested-member",
                session.runtime_generation
            )
            .unwrap(),
        "a seeded pointer is an intent, never an execution claim"
    );

    // The authority binds: no longer requested.
    store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "requested-bind", 1),
            "session-requested",
            1,
            requested_resume_seed(),
        )
        .expect("the authority binds the same conversation the seed named");
    assert!(
        !store
            .member_run_native_session_is_requested(
                "space-test",
                "requested-member",
                session.runtime_generation
            )
            .unwrap(),
        "once an AgentSession owns the pointer the MemberRun copy is a projection"
    );

    // A different generation is a different lane and is still requested.
    assert!(
        store
            .member_run_native_session_is_requested("space-test", "requested-member", 99)
            .unwrap(),
        "the predicate is per member AND runtime generation"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

/// An `external_interactive` Host never has an AgentSession, so its pointer is
/// requested for the lane's whole life — there is no later write that promotes
/// it, and nothing should report it as an execution claim.
#[test]
fn an_external_interactive_host_pointer_stays_requested_forever() {
    let (store, root) = fabric_store();
    seed_agent_member(
        &store,
        &context("host", "identity.create", "identity-external", 0),
        "external-host",
    );
    // Deliberately no AgentSession: that is what external_interactive means.
    assert!(
        store
            .member_run_native_session_is_requested("space-test", "external-host", 1)
            .unwrap(),
        "a Host with no AgentSession can never hold an authoritative pointer"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

/// The exact shape a `--resume-member` seed leaves on disk, taken from the 12
/// real objects in the `dev109-final-20260827-6d71ea3e` Execution Space: a
/// version-less, `unknown`-availability, non-resumable pointer whose parent id
/// equals its own — and, before N2a, a `native_locator_kind` no adapter emits.
///
/// Using the real shape matters: it is precisely the pre-session *requested*
/// case, and it proves the case exists rather than being a design hypothetical.
fn requested_resume_seed() -> NativeSessionRef {
    NativeSessionRef {
        provider: "deepseek_harness".into(),
        execution_mode: "deepseek_sdk".into(),
        native_session_id: "star-eca626c0-164a-47d0-be42-67b28b2a94f2".into(),
        native_locator_kind: "provider_native".into(),
        provider_version: None,
        adapter_contract_version: "deepseek-harness-native-v1".into(),
        availability: firm_core::agentfirm_api::NativeSessionAvailability::Unknown,
        supports_resume: false,
        last_verified_at: None,
        parent_native_session_id: Some("star-eca626c0-164a-47d0-be42-67b28b2a94f2".into()),
    }
}
