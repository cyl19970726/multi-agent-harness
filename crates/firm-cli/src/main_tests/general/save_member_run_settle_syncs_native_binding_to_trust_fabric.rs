use super::*;

#[test]
fn save_member_run_settle_syncs_native_binding_to_trust_fabric() {
    let (store, root) = temp_store("settle-syncs-native-binding");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-settle-sync",
            std::process::id(),
            "test://settle-sync",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    let expected = created.member_runs[0].clone();
    assert!(
        expected.native_session.is_none(),
        "fresh-start precondition"
    );
    let mut settled = expected.clone();
    settled.native_session = Some(capacity_test_session());
    settled.status = MemberRunStatus::Idle;
    settled.last_event_at = Some(now_string());
    ledger
        .save_member_run(&expected, &settled)
        .expect("settle save succeeds");

    let space_id = store
        .trust_member_run_scope(&settled.id)
        .expect("trust MemberRun scope")
        .expect("trust fabric exists");
    let trust_run = store
        .trust_member_runs(&space_id)
        .expect("trust MemberRuns")
        .into_iter()
        .find(|run| run.id == settled.id)
        .expect("canonical trust MemberRun");
    assert_eq!(
        trust_run
            .native_session
            .as_ref()
            .map(|session| session.native_session_id.as_str()),
        Some("thread-capacity-recovery"),
        "save_member_run must sync the settled binding onto the trust MemberRun"
    );
    let sessions = store
        .fabric_agent_sessions(&space_id)
        .expect("canonical AgentSessions")
        .into_iter()
        .filter(|session| session.agent_member_id == settled.agent_member_id)
        .collect::<Vec<_>>();
    assert_eq!(sessions.len(), 1, "one current AgentSession: {sessions:?}");
    assert_eq!(
        sessions[0]
            .native_session_ref
            .as_ref()
            .map(|session| session.native_session_id.as_str()),
        Some("thread-capacity-recovery"),
        "save_member_run must sync the settled binding onto the canonical AgentSession"
    );

    // Lifecycle timestamps are part of the strict current projection and
    // therefore advance both ledgers even when the native binding itself
    // is unchanged.
    let before = store
        .canonical_operations()
        .expect("operations before idempotent saves")
        .len();
    let latest = ledger
        .latest_member_run(&settled.id)
        .expect("latest member")
        .expect("member exists");
    let mut status_only = latest.clone();
    status_only.last_event_at = Some(now_string());
    ledger
        .save_member_run(&latest, &status_only)
        .expect("status-only save succeeds");
    let after = store
        .canonical_operations()
        .expect("operations after idempotent saves")
        .len();
    assert_eq!(after, before + 1, "lifecycle timestamp syncs canonically");
    let canonical_after = store
        .trust_member_runs(&space_id)
        .expect("trust MemberRuns after timestamp update")
        .into_iter()
        .find(|run| run.id == settled.id)
        .expect("canonical trust MemberRun after timestamp update");
    assert_eq!(canonical_after.last_event_at, status_only.last_event_at);
    std::fs::remove_dir_all(root).expect("cleanup");
}
