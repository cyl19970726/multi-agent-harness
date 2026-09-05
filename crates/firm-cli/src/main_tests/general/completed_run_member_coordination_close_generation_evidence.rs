use super::*;

/// Build one run with one idle member bound to a detached, idle native
/// Session last driven by the gen-1 Supervisor, plus that gen-1 lease.
fn completed_run_close_fixture(
    label: &str,
) -> (
    HarnessStore,
    PathBuf,
    harness_core::TeamSupervisorLease,
    harness_core::ProviderRuntimeProjection,
) {
    use harness_core::agentfirm_api::{AgentSessionStatus, RuntimeActivity, RuntimeResidency};

    let (store, root) = temp_store(label);
    let created = create_two_member_team_run(&store);
    let initial = created.member_runs[0].clone();
    let mut bound = initial.clone();
    bound.native_session = Some(capacity_test_session());
    bound.last_event_at = Some("unix-ms:completed-close-bound".into());
    store
        .compare_and_append_member_run(&initial, &bound)
        .expect("bind native session");

    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-completed-close-gen1",
            std::process::id(),
            "test://completed-close-gen1",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire gen-1 Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let run = latest_team_run(&store, &created.team_run.id).expect("TeamRun");
    let members = latest_member_runs_in_append_order(&store)
        .expect("members")
        .into_iter()
        .filter(|member| member.team_run_id == run.id)
        .collect();
    let body = PreparedTeamRunBody {
        run_id: run.id.clone(),
        objective: run.objective.clone(),
        run: run.clone(),
        members,
    };
    bind_team_runtime_supervisor(
        &store,
        &body,
        &lease.execution_space_id,
        &lease.node_daemon_id,
        &lease.supervisor_id,
        lease.generation,
    )
    .expect("bind gen-1 Supervisor driver");
    let ledger = TeamRunLedger::new(
        &store,
        &run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    transition_provider_session_for_member(&ledger, &bound, AgentSessionStatus::Idle)
        .expect("idle session");
    transition_provider_session_runtime_control(
        &ledger,
        &bound,
        RuntimeResidency::Attached,
        RuntimeActivity::Idle,
    )
    .expect("attach provider runtime");
    settle_provider_attempt_release(&ledger, &bound).expect("detach provider runtime");

    let mut idle = bound.clone();
    idle.status = MemberRunStatus::Idle;
    idle.last_event_at = Some("unix-ms:completed-close-idle".into());
    ledger.save_member_run(&bound, &idle).expect("idle member");

    (store, root, lease, idle)
}

fn acquire_gen2_lease(
    store: &HarnessStore,
    team_run_id: &str,
    now_unix_ms: u64,
) -> harness_core::TeamSupervisorLease {
    store
        .acquire_test_supervisor_lease(
            team_run_id,
            "supervisor-completed-close-gen2",
            std::process::id(),
            "test://completed-close-gen2",
            now_unix_ms,
            60_000,
        )
        .expect("acquire gen-2 Supervisor lease")
}

#[test]
fn completed_run_coordination_close_exact_current_generation_needs_no_evidence() {
    let (store, _root, lease, idle) = completed_run_close_fixture("completed-close-exact-current");

    let closed = crate::completed_run_members::close_completed_run_member_coordination(
        &store,
        &idle.team_run_id,
        &idle,
        &lease,
        "host",
        "completed TeamRun cleanup",
    )
    .expect("exact-current coordination Close")
    .expect("exact-current Close proceeds");
    assert_eq!(closed["status"], "stopped");
    assert_eq!(closed["coordination_status"], "closed");
}

#[test]
fn completed_run_coordination_close_newer_daemon_generation_is_the_evidence() {
    let (store, _root, lease, idle) =
        completed_run_close_fixture("completed-close-newer-daemon-evidence");
    let team_run_id = idle.team_run_id.clone();

    // The only accepted evidence: the session's NodeDaemon generation
    // reaches Released (a terminating drain or an Operator-confirmed
    // recovery) and a newer daemon generation takes the machine. A
    // Supervisor lease Released row is NOT evidence and is not consulted.
    store
        .release_team_supervisor_lease(
            &team_run_id,
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms_u64(),
        )
        .expect("release gen-1 Supervisor lease");
    let run = latest_team_run(&store, &team_run_id).expect("TeamRun");
    store
        .release_node_daemon_lease(
            &run.execution_node_id,
            &lease.node_daemon_id,
            lease.node_daemon_generation,
            "test-node-daemon-instance",
            current_unix_ms_u64(),
        )
        .expect("release gen-1 NodeDaemon lease");
    let daemon_gen2 = store
        .acquire_node_daemon_lease(
            &run.execution_node_id,
            &lease.node_daemon_id,
            "test-node-daemon-instance-2",
            current_unix_ms_u64(),
            u64::MAX / 2,
        )
        .expect("acquire gen-2 NodeDaemon lease");
    assert!(daemon_gen2.generation > lease.node_daemon_generation);
    let gen2 = acquire_gen2_lease(&store, &team_run_id, current_unix_ms_u64());
    assert!(gen2.generation > lease.generation);
    assert_eq!(gen2.node_daemon_generation, daemon_gen2.generation);

    let closed = crate::completed_run_members::close_completed_run_member_coordination(
        &store,
        &team_run_id,
        &idle,
        &gen2,
        "host",
        "completed TeamRun cleanup",
    )
    .expect("coordination Close under a newer daemon generation")
    .expect("the newer daemon generation admits the Close");
    assert_eq!(closed["status"], "stopped");
}

#[test]
fn completed_run_coordination_close_superseded_generation_without_evidence_fails_closed() {
    let (store, _root, lease, idle) =
        completed_run_close_fixture("completed-close-superseded-no-evidence");
    let team_run_id = idle.team_run_id.clone();

    // A superseded Supervisor generation under the SAME still-live
    // NodeDaemon generation: no daemon-lease fence and no recover-predecessor
    // ever intervenes, and the predecessor Supervisor's exit terminated
    // nothing. The gen-1 Supervisor lease only expires; it is never Released.
    let gen2 = acquire_gen2_lease(
        &store,
        &team_run_id,
        lease.expires_unix_ms.saturating_add(1),
    );
    assert!(gen2.generation > lease.generation);
    assert_eq!(gen2.node_daemon_generation, lease.node_daemon_generation);

    let error = crate::completed_run_members::close_completed_run_member_coordination(
        &store,
        &team_run_id,
        &idle,
        &gen2,
        "host",
        "completed TeamRun cleanup",
    )
    .expect_err("superseded Close without released evidence must be refused");
    let detail = error.to_string();
    assert!(
        detail.contains("DETACHED_MEMBER_RECOVERY_FENCED")
            && detail.contains("still-live NodeDaemon generation"),
        "refusal must name the unproven process-group termination: {detail}"
    );
    let member = latest_member_runs_in_append_order(&store)
        .expect("members")
        .into_iter()
        .find(|member| member.id == idle.id)
        .expect("member row");
    assert!(
        member.coordination_is_active() && member.status == MemberRunStatus::Idle,
        "a fenced Close must not persist any member mutation: {member:?}"
    );
}
