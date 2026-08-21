use super::*;

#[test]
fn provider_authority_never_borrows_a_sole_foreign_space_session() {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeCommandKind};

    let (store, root) = temp_store("provider-session-exact-space");
    let created = create_two_member_team_run(&store);
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "provider-session-exact-space",
            std::process::id(),
            "test://provider-session-exact-space",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire canonical test Supervisor");
    let foreign_space = "foreign-sole-provider-session-space";
    ensure_foreign_test_message_fabric(&store, &created, &lease, foreign_space);
    let member = &created.member_runs[0];
    let before = store
        .fabric_agent_sessions(foreign_space)
        .expect("foreign sessions")
        .into_iter()
        .find(|session| session.agent_member_id == member.agent_member_id)
        .expect("sole foreign provider session");
    assert_ne!(before.lifecycle, AgentSessionStatus::Closed);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );

    for error in [
        require_member_provider_session_authority(&ledger, member, false)
            .expect_err("authority cannot borrow the foreign session"),
        transition_provider_session_for_member(&ledger, member, AgentSessionStatus::Active)
            .expect_err("transition cannot mutate the foreign session"),
        prepare_provider_process_effect(&ledger, member)
            .expect_err("process effect cannot target the foreign session"),
        prepare_provider_effect_kind(
            &ledger,
            member,
            "foreign-session-input",
            "must remain local",
            RuntimeCommandKind::DispatchProvider,
            "provider.dispatch",
        )
        .expect_err("provider effect cannot target the foreign session"),
    ] {
        assert!(
            error.to_string().contains("found 0"),
            "exact local-space absence must fail closed: {error}"
        );
    }
    let after = store
        .fabric_agent_sessions(foreign_space)
        .expect("foreign sessions after rejection")
        .into_iter()
        .find(|session| session.id == before.id)
        .expect("foreign provider session remains");
    assert_eq!(after, before);
    assert!(store
        .runtime_commands(&lease.execution_space_id)
        .expect("local RuntimeCommands")
        .is_empty());
    assert!(store
        .runtime_commands(foreign_space)
        .expect("foreign RuntimeCommands")
        .is_empty());
    std::fs::remove_dir_all(root).expect("cleanup");
}
