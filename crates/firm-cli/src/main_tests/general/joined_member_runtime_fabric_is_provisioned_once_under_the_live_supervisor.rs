use super::*;

/// A member admitted into a live TeamRun (`team-run add-member`) misses the
/// adoption pass that materializes AgentSessions, so the Supervisor provisions
/// it on first drive — exactly once, and never again for a member that already
/// owns one (#749).
///
/// This pins the durable half of that seam. It calls no provider executable on
/// purpose: `ensure_joined_member_runtime_fabric` freezes the provider profile
/// first, and that version probe belongs to the daemon integration test where
/// a deterministic PATH shim exists.
#[test]
fn joined_member_runtime_fabric_is_provisioned_once_under_the_live_supervisor() {
    let (store, root) = temp_store("joined-member-runtime-fabric");
    let created = create_two_member_team_run(&store);
    let execution_space_id =
        team_run_execution_space_id(&store, &created.team_run).expect("TeamRun Execution Space");
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-joined-member-fabric",
            std::process::id(),
            "test://joined-member-fabric",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("acquire the live Supervisor lease");
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );

    let late = TeamMemberSpec {
        agent_member_id: "agent-joined-late".into(),
        name: "JoinedLate".into(),
        role: "reviewer".into(),
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: vec!["crates/review".into()],
        resume_native_session_id: None,
        initial_work: None,
    };
    ensure_unit_test_canonical_members(
        &store,
        &execution_space_id,
        &created.team_run.agent_team_id,
        std::slice::from_ref(&late),
    )
    .expect("the joining AgentMember earns its durable TeamMembership first");
    let (run, joined, _) = add_team_run_member(&store, None, &created.team_run.id, &late, None)
        .expect("admit the member into the live run");
    assert!(
        member_needs_agent_session(&store, &execution_space_id, &joined)
            .expect("session inventory"),
        "the defect precondition is that add-member leaves the joined member sessionless"
    );

    let session_id =
        provision_member_agent_session(&ledger, &lease, &run, &execution_space_id, &joined)
            .expect("the live Supervisor provisions the joined member's AgentSession");
    let sessions = current_sessions(&store, &execution_space_id, &joined.agent_member_id);
    let [session] = sessions.as_slice() else {
        panic!(
            "one AgentMember owns exactly one current AgentSession, found {}",
            sessions.len()
        );
    };
    assert_eq!(session.id, session_id);
    assert_eq!(session.node_id, created.team_run.execution_node_id);
    assert_eq!(session.execution_space_id, execution_space_id);
    assert_eq!(session.node_daemon_id, lease.node_daemon_id);
    assert_eq!(session.node_daemon_generation, lease.node_daemon_generation);
    assert_eq!(session.provider_kind, joined.provider);
    assert_eq!(
        session.control_state.driver_ref,
        harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor {
            team_run_id: created.team_run.id.clone(),
            team_supervisor_id: lease.supervisor_id.clone(),
            team_supervisor_generation: lease.generation,
        },
        "the joined member's session must be bound to the live Supervisor generation"
    );
    let provisioned = session.clone();

    // The outer seam is now a no-op for this member. Re-binding a live member
    // would reset residency/activity and lie about an attached provider handle.
    assert!(
        !member_needs_agent_session(&store, &execution_space_id, &joined)
            .expect("session inventory"),
        "a provisioned member must not be provisioned again"
    );
    provision_member_agent_session(&ledger, &lease, &run, &execution_space_id, &joined)
        .expect("re-provisioning the same generation is idempotent");
    assert_eq!(
        current_sessions(&store, &execution_space_id, &joined.agent_member_id).as_slice(),
        [provisioned],
        "an idempotent re-run must not mint a second session or rewrite control state"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

fn current_sessions(
    store: &HarnessStore,
    execution_space_id: &str,
    agent_member_id: &str,
) -> Vec<harness_core::agentfirm_api::AgentSession> {
    store
        .fabric_agent_sessions(execution_space_id)
        .expect("canonical AgentSession fabric")
        .into_iter()
        .filter(|session| {
            session.agent_member_id == agent_member_id
                && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        })
        .collect()
}
