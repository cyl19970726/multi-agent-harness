//! Regression for the NodeDaemon drain that used to wedge every mid-turn
//! member (#748, follows #746).
//!
//! The drain kills this daemon's owned provider process groups and settles
//! every mid-turn AgentSession as `Interrupted`. The next Supervisor generation
//! then re-adopts the TeamRun and must be able to either resume that Session or
//! close it; before this regression both exits were fenced and the only escape
//! was a brand new AgentMember.

use super::tests::TestTree;
use super::*;
use crate::ProviderEffectSettlement;
use crate::{bind_team_runtime_supervisor, ensure_team_message_fabric, TeamRunLedger};

use harness_core::agentfirm_api::{
    AgentSessionStatus, RuntimeActivity, RuntimeCommandKind, RuntimeCommandPhase,
    RuntimeEffectCertainty, RuntimeResidency,
};
use harness_core::{
    MemberRunStatus, NativeSessionAvailability, NativeSessionRef, ProviderRuntimeProjection,
    TeamSupervisorLeaseStatus,
};

/// The unit-test AgentTeam fixture binds its canonical AgentMembers here.
pub(super) const DRAIN_SPACE_ID: &str = "unit-test-space";
pub(super) const MID_TURN_MEMBER: &str = "agent-builder-a";
pub(super) const IDLE_MEMBER: &str = "agent-builder-b";

pub(super) struct DrainFixture {
    _tree: TestTree,
    pub(super) store: HarnessStore,
    pub(super) run_id: String,
    node_id: String,
    project_binding_id: String,
    daemon: TestDaemon,
    pub(super) daemon_generation: u64,
}

fn drain_native_session(native_session_id: &str) -> NativeSessionRef {
    NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        native_session_id: native_session_id.into(),
        native_locator_kind: "thread_id".into(),
        provider_version: Some("test".into()),
        adapter_contract_version: "test".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("unix-ms:99".into()),
        parent_native_session_id: None,
    }
}

pub(super) fn member_named(
    store: &HarnessStore,
    run_id: &str,
    agent_member_id: &str,
) -> ProviderRuntimeProjection {
    store
        .latest_member_runs()
        .expect("member runs")
        .into_iter()
        .find(|member| member.team_run_id == run_id && member.agent_member_id == agent_member_id)
        .unwrap_or_else(|| panic!("MemberRun for {agent_member_id}"))
}

pub(super) fn agent_session(
    store: &HarnessStore,
    agent_member_id: &str,
) -> harness_core::agentfirm_api::AgentSession {
    store
        .fabric_agent_sessions(DRAIN_SPACE_ID)
        .expect("agent sessions")
        .into_iter()
        .find(|session| session.agent_member_id == agent_member_id)
        .unwrap_or_else(|| panic!("AgentSession for {agent_member_id}"))
}

pub(super) fn drain_fixture(label: &str) -> DrainFixture {
    drain_fixture_with_pi(label, false)
}

pub(super) fn drain_fixture_with_pi(label: &str, pi_member: bool) -> DrainFixture {
    let tree = TestTree::new(label);
    let firm_home = tree.0.join("home");
    let space = crate::execution_space::register_and_activate(
        &firm_home,
        DRAIN_SPACE_ID,
        "Drain Recovery Space",
        Some("unit-test-project".to_string()),
        None,
        "unix-ms:1",
    )
    .expect("register drain Execution Space");
    let store = HarnessStore::new(space.store_root.clone());
    store.init().expect("initialize drain Store");

    let member = |agent_member_id: &str, name: &str, role: &str| crate::TeamMemberSpec {
        agent_member_id: agent_member_id.into(),
        name: name.into(),
        role: role.into(),
        provider: if pi_member && agent_member_id == MID_TURN_MEMBER {
            "pi"
        } else {
            "codex"
        }
        .into(),
        execution_mode: Some(
            if pi_member && agent_member_id == MID_TURN_MEMBER {
                "pi_rpc"
            } else {
                "codex_app_server"
            }
            .into(),
        ),
        model: None,
        effort: None,
        service_tier: None,
        provider_cwd_hint: None,
        owned_paths: Vec::new(),
        resume_native_session_id: None,
        initial_work: None,
    };
    let created = crate::create_team_run(
        &store,
        None,
        None,
        None,
        "Survive a NodeDaemon drain",
        None,
        "test",
        None,
        harness_core::HostControlMode::Managed,
        None,
        None,
        None,
        None,
        &[
            member(MID_TURN_MEMBER, "BuilderA", "module_a"),
            member(IDLE_MEMBER, "BuilderB", "module_b"),
            member("host", "Host", "host"),
        ],
    )
    .expect("create drain TeamRun");
    let run_id = created.team_run.id.clone();
    let node_id = created.team_run.execution_node_id.clone();
    let project_binding_id = created.team_run.project_binding_id.clone();

    // Both members carry a resumable provider-native session, so the drain has
    // real execution truth to preserve across the daemon generation.
    for agent_member_id in [MID_TURN_MEMBER, IDLE_MEMBER] {
        let expected = member_named(&store, &run_id, agent_member_id);
        let mut bound = expected.clone();
        bound.native_session = Some(drain_native_session(&format!(
            "thread-drain-{agent_member_id}"
        )));
        if pi_member && agent_member_id == MID_TURN_MEMBER {
            let native = bound.native_session.as_mut().unwrap();
            native.provider = "pi".into();
            native.execution_mode = "pi_rpc".into();
            native.native_locator_kind = "pi_session".into();
            native.native_session_id = tree.0.join("pi-native.jsonl").display().to_string();
        }
        bound.last_event_at = Some("unix-ms:drain-bound".into());
        store
            .compare_and_append_member_run(&expected, &bound)
            .expect("bind native session");
    }

    if !store
        .latest_execution_nodes()
        .expect("nodes")
        .iter()
        .any(|node| node.id == node_id)
    {
        store
            .insert_execution_node(&harness_core::ExecutionNode {
                id: node_id.clone(),
                display_name: "Drain Node".into(),
                status: harness_core::ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            })
            .expect("insert drain Node");
    }
    store
        .register_node_project(
            &harness_core::NodeProjectRegistration {
                node_id: node_id.clone(),
                execution_space_id: space.id.clone(),
                project_binding_id: project_binding_id.clone(),
                status: harness_core::NodeProjectRegistrationStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            },
            &space.id,
        )
        .expect("register drain project");

    let daemon_id = format!("node-daemon:{node_id}");
    let lease = store
        .acquire_node_daemon_lease(
            &node_id,
            &daemon_id,
            "drain-instance",
            current_unix_ms_u64(),
            600_000,
        )
        .expect("acquire drain daemon lease");
    ensure_team_message_fabric(
        &store,
        &run_id,
        &space.id,
        &lease.daemon_id,
        lease.generation,
    )
    .expect("materialize canonical AgentSessions");

    let daemon = TestDaemon::new(TestDaemonConfig {
        firm_home,
        node_id: node_id.clone(),
        daemon_id,
        instance_id: "drain-instance".into(),
        contexts: Vec::new(),
        application: Arc::new(DaemonApplication),
        max_concurrency: 1,
        input_acceptance_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        lease_ttl_override_ms: None,
        drain_timeout_override_ms: None,
    });

    DrainFixture {
        _tree: tree,
        store,
        run_id,
        node_id,
        project_binding_id,
        daemon,
        daemon_generation: lease.generation,
    }
}

impl DrainFixture {
    /// This machine's one NodeDaemon identity, for a test that needs to take a
    /// successor generation itself instead of going through `readopt`.
    pub(super) fn daemon_id(&self) -> &str {
        self.daemon.daemon_id()
    }

    pub(super) fn node_id(&self) -> &str {
        &self.node_id
    }

    pub(super) fn supervise(
        &self,
        supervisor_id: &str,
        daemon_generation: u64,
    ) -> Arc<TeamRunLedger> {
        let lease = self
            .store
            .acquire_team_supervisor_under_node_lease(
                &self.run_id,
                &self.node_id,
                self.daemon.daemon_id(),
                daemon_generation,
                DRAIN_SPACE_ID,
                &self.project_binding_id,
                supervisor_id,
                std::process::id(),
                "test://drain-recovery",
                current_unix_ms_u64(),
                600_000,
            )
            .expect("acquire Supervisor lease");
        let run =
            crate::daemon_support::latest_team_run(&self.store, &self.run_id).expect("TeamRun");
        let members = crate::latest_member_runs_in_append_order(&self.store)
            .expect("member runs")
            .into_iter()
            .filter(|member| member.team_run_id == run.id)
            .collect();
        bind_team_runtime_supervisor(
            &self.store,
            &crate::PreparedTeamRunBody {
                run_id: run.id.clone(),
                objective: run.objective.clone(),
                run,
                members,
            },
            DRAIN_SPACE_ID,
            self.daemon.daemon_id(),
            supervisor_id,
            lease.generation,
        )
        .expect("bind Supervisor driver");
        Arc::new(TeamRunLedger::new(
            &self.store,
            &self.run_id,
            supervisor_id,
            lease.generation,
            Arc::new(AtomicBool::new(true)),
        ))
    }

    /// Put one member mid-turn: an Active Session with an attached, running
    /// provider handle and one settled StartCycle.
    fn start_one_cycle(&self, ledger: &TeamRunLedger) {
        self.start_cycle_for(ledger, "work-delivery:drain:turn:1");
    }

    /// The same mid-turn state, driven by one exact canonical WorkDelivery.
    pub(super) fn start_cycle_for(&self, ledger: &TeamRunLedger, delivery_id: &str) {
        let member = member_named(&self.store, &self.run_id, MID_TURN_MEMBER);
        crate::transition_provider_session_for_member(ledger, &member, AgentSessionStatus::Active)
            .expect("activate the mid-turn Session");
        crate::transition_provider_session_runtime_control(
            ledger,
            &member,
            RuntimeResidency::Attached,
            RuntimeActivity::Running,
        )
        .expect("attach the provider runtime");
        let admission = crate::prepare_provider_effect(
            ledger,
            &member,
            delivery_id,
            "execute the mid-turn work",
            1,
        )
        .expect("admit one StartCycle");
        crate::settle_provider_effect(
            ledger,
            &admission,
            ProviderEffectSettlement::APPLIED_SATISFIED,
            Some(serde_json::json!({
                "phase": "input_accepted",
                "provider_receipt": {
                    "command": "deliver",
                    "response_id": "provider-receipt:drain",
                    "success": true,
                },
            })),
            None,
        )
        .expect("settle the StartCycle before the drain");
    }

    pub(super) fn idle_one_member(&self, ledger: &TeamRunLedger) {
        let member = member_named(&self.store, &self.run_id, IDLE_MEMBER);
        crate::transition_provider_session_for_member(ledger, &member, AgentSessionStatus::Idle)
            .expect("idle the second Session");
        crate::transition_provider_session_runtime_control(
            ledger,
            &member,
            RuntimeResidency::Detached,
            RuntimeActivity::Idle,
        )
        .expect("detach the idle runtime");
    }

    /// The drain the r5 dogfood run observed: the Supervisor lease is released,
    /// this daemon settles its own Sessions after killing the owned provider
    /// process groups, then releases its machine authority.
    pub(super) fn drain(&self, supervisor_id: &str, supervisor_generation: u64) {
        self.store
            .release_team_supervisor_lease(
                &self.run_id,
                supervisor_id,
                supervisor_generation,
                current_unix_ms_u64(),
            )
            .expect("release the Supervisor lease before settlement");
        self.daemon
            .settle_node_authorities_for_shutdown()
            .expect("the daemon settles its own Sessions");
        let (result, report) = self.daemon.release_node_authorities();
        result.expect("the daemon releases its machine authority");
        assert_eq!(report.released_space_ids, vec![DRAIN_SPACE_ID.to_string()]);
    }

    /// The successor NodeDaemon generation re-adopts the TeamRun exactly the way
    /// `team-run start` does after `daemon start`.
    pub(super) fn readopt(&self) -> u64 {
        let successor = self
            .store
            .acquire_node_daemon_lease(
                &self.node_id,
                self.daemon.daemon_id(),
                "drain-instance-2",
                current_unix_ms_u64(),
                600_000,
            )
            .expect("successor NodeDaemon generation");
        assert!(successor.generation > self.daemon_generation);
        ensure_team_message_fabric(
            &self.store,
            &self.run_id,
            DRAIN_SPACE_ID,
            &successor.daemon_id,
            successor.generation,
        )
        .expect("the successor generation reattaches every drained AgentSession");
        successor.generation
    }

    pub(super) fn start_cycles(&self) -> Vec<harness_core::agentfirm_api::RuntimeCommandRecord> {
        self.store
            .runtime_commands(DRAIN_SPACE_ID)
            .expect("runtime commands")
            .into_iter()
            .filter(|command| command.command == RuntimeCommandKind::StartCycle)
            .collect()
    }
}

#[test]
fn drained_mid_turn_member_resumes_under_the_next_supervisor_generation() {
    let fixture = drain_fixture("drain-resume");
    let ledger = fixture.supervise("supervisor-drain-1", fixture.daemon_generation);
    fixture.start_one_cycle(&ledger);
    fixture.idle_one_member(&ledger);
    let killed_cycle = fixture.start_cycles();
    assert_eq!(killed_cycle.len(), 1);
    drop(ledger);

    fixture.drain("supervisor-drain-1", 1);

    let interrupted = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(
        interrupted.lifecycle,
        AgentSessionStatus::Interrupted,
        "the drain cuts the mid-turn cycle"
    );
    assert_eq!(
        interrupted.control_state.runtime_residency,
        RuntimeResidency::Detached
    );
    assert!(interrupted.current_turn_id.is_none());
    assert_eq!(
        agent_session(&fixture.store, IDLE_MEMBER).lifecycle,
        AgentSessionStatus::Idle,
        "a member idle at drain time is unaffected"
    );

    // The exact hop the r5 run could not make. Since #779 the adoption seam
    // performs it, so asserting it after `readopt` would assert nothing: take
    // the successor generation and reattach the lane WITHOUT resuming it, then
    // make the call on a lane that is still `Interrupted`. That is the state
    // DEV-171 is about, and the only way this regression stays honest.
    let successor_generation =
        super::drain_blocked_member_tests::reattach_without_resuming(&fixture);
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Interrupted,
        "the fence must be exercised on an interrupted lane, not an already-resumed one"
    );
    let ledger = fixture.supervise("supervisor-drain-2", successor_generation);
    let mid_turn = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_for_member(&ledger, &mid_turn, AgentSessionStatus::Idle)
        .expect("a drained member must resume without INVALID_STATE_TRANSITION");
    let resumed = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(resumed.lifecycle, AgentSessionStatus::Idle);
    assert_eq!(
        resumed
            .native_session_ref
            .as_ref()
            .map(|native| native.native_session_id.as_str()),
        Some(format!("thread-drain-{MID_TURN_MEMBER}").as_str()),
        "resume keeps the provider-native session identity"
    );
    assert_eq!(resumed.node_daemon_generation, successor_generation);

    // The idle member still starts an ordinary cycle under the new generation.
    let idle = member_named(&fixture.store, &fixture.run_id, IDLE_MEMBER);
    crate::transition_provider_session_for_member(&ledger, &idle, AgentSessionStatus::Active)
        .expect("the idle member is unaffected by the drain");

    // The killed cycle stays exactly one settled, terminal, non-replayed fact.
    let after = fixture.start_cycles();
    assert_eq!(
        after.len(),
        1,
        "resume must open a new cycle, never replay the killed one"
    );
    assert_eq!(after[0].id, killed_cycle[0].id);
    assert_eq!(after[0].phase, RuntimeCommandPhase::Settled);
    assert_eq!(after[0].effect_certainty, RuntimeEffectCertainty::Applied);
    assert_eq!(
        after[0].target_node_daemon_generation, fixture.daemon_generation,
        "the killed cycle stays bound to the dead daemon generation"
    );
}

#[test]
fn drained_mid_turn_member_can_be_closed_by_the_host() {
    let fixture = drain_fixture("drain-close");
    let ledger = fixture.supervise("supervisor-close-1", fixture.daemon_generation);
    fixture.start_one_cycle(&ledger);
    drop(ledger);
    fixture.drain("supervisor-close-1", 1);
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Interrupted
    );

    let successor_generation = fixture.readopt();
    let ledger = fixture.supervise("supervisor-close-2", successor_generation);
    let supervisor = fixture
        .store
        .latest_team_supervisor_lease(&fixture.run_id)
        .expect("supervisor lease read")
        .expect("live Supervisor lease");
    assert_eq!(supervisor.status, TeamSupervisorLeaseStatus::Active);

    // The re-adopted member is Blocked on the failed provider attempt: Close is
    // the Host's escape hatch and must not be fenced on the lifecycle label
    // while the runtime is provably dead.
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:drain-blocked".into());
    ledger
        .save_member_run(&expected, &blocked)
        .expect("block the re-adopted member");

    let receipt = crate::close_detached_blocked_member_for_recovery(
        &fixture.store,
        &fixture.run_id,
        &blocked,
        &supervisor,
        "host",
        "drained runtime is gone; close the member",
    )
    .expect("Close must not be fenced on an interrupted, detached lane")
    .expect("the detached recovery Close applies");
    assert_eq!(receipt["coordination_effect"], "member_closed_for_recovery");
    assert_eq!(
        member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER).status,
        MemberRunStatus::Stopped
    );
    assert_eq!(
        fixture.start_cycles().len(),
        1,
        "Close never replays the killed cycle"
    );
}

/// GitHub #937: a dead Team Supervisor must not leave the Sessions it drove
/// falsely `Active`. The failure settles them truthfully as
/// `RecoveryRequired` (the lane's effect is unproven — never Idle/Detached),
/// while quiet unrelated Sessions stay untouched and the TeamRun-level
/// recovery marker still lands. Red today: the driven Session stays `Active`.
#[test]
fn a_dead_supervisor_settles_driven_active_sessions_as_recovery_required() {
    let fixture = drain_fixture("supervisor-failure-settlement");
    let ledger = fixture.supervise("supervisor-settlement-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:settle:1");
    let driven = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(
        driven.lifecycle,
        AgentSessionStatus::Active,
        "the fixture drives the member Session Active before the failure"
    );
    let quiet_before = agent_session(&fixture.store, IDLE_MEMBER).lifecycle;
    let (supervisor_id, supervisor_generation) = bound_supervisor(&driven);

    // The window3 death: the Supervisor thread fails on a Store write-lock
    // timeout while the daemon survives.
    let context = supervisor_failure_context(&fixture, &supervisor_id, supervisor_generation, None);
    fixture.daemon.block_finished_supervisor_failure(
        &context,
        &CliError::Usage(
            "store error: timed out waiting for store write lock; lock_wait_ms=2001; lock_budget_ms=2000"
                .into(),
        ),
    );

    // The TeamRun-level diagnosis still lands (existing behavior).
    assert!(
        fixture
            .store
            .member_actions()
            .expect("read member actions")
            .into_iter()
            .any(|action| action.team_run_id == fixture.run_id
                && action.action_type == "team_supervisor_recovery_required"),
        "the TeamRun-level recovery marker must still be written"
    );

    // The truthful per-Session settlement (#937): the driven, falsely Active
    // Session is marked RecoveryRequired — uncertain, never Idle/Detached.
    let settled = agent_session(&fixture.store, MID_TURN_MEMBER);
    assert_eq!(
        settled.lifecycle,
        AgentSessionStatus::RecoveryRequired,
        "a dead Supervisor must settle its driven Active Session as RecoveryRequired, not leave it falsely Active"
    );

    // The unrelated quiet Session is untouched by another member's failure.
    assert_eq!(
        agent_session(&fixture.store, IDLE_MEMBER).lifecycle,
        quiet_before,
        "an unrelated quiet Session is never settled by a Supervisor failure"
    );
}

/// The exact failed driver of one Session, as the Supervisor bound it.
fn bound_supervisor(session: &harness_core::agentfirm_api::AgentSession) -> (String, u64) {
    match &session.control_state.driver_ref {
        harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor {
            team_supervisor_id,
            team_supervisor_generation,
            ..
        } => (team_supervisor_id.clone(), *team_supervisor_generation),
        other => panic!("expected a TeamSupervisor driver, got {other:?}"),
    }
}

/// The dead Supervisor's context, as the daemon carried it into the reap.
fn supervisor_failure_context(
    fixture: &DrainFixture,
    supervisor_id: &str,
    supervisor_generation: u64,
    thread: Option<std::thread::JoinHandle<CliResult<TeamRunDriveOutcome>>>,
) -> OwnedTestContext {
    OwnedTestContext::new(TestContextConfig {
        execution_space_id: DRAIN_SPACE_ID.into(),
        project_binding_id: fixture.project_binding_id.clone(),
        run_id: fixture.run_id.clone(),
        daemon_generation: fixture.daemon_generation,
        supervisor_id: supervisor_id.into(),
        supervisor_generation,
        heartbeat_valid: Arc::new(AtomicBool::new(false)),
        serving_status: Arc::new(Mutex::new("running".into())),
        thread,
        started_at: Instant::now(),
    })
}

const SUPERVISOR_LOCK_DEATH: &str =
    "store error: timed out waiting for store write lock; lock_wait_ms=2001; lock_budget_ms=2000";

/// #937: the real route — a Supervisor THREAD that fails is joined by
/// reap_finished, which must route the error into the same truthful
/// settlement (not only a direct setter call).
#[test]
fn reap_of_a_failed_supervisor_thread_settles_its_driven_sessions() {
    let fixture = drain_fixture("supervisor-failure-reap-route");
    let ledger = fixture.supervise("supervisor-reap-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:reap:1");
    let driven = agent_session(&fixture.store, MID_TURN_MEMBER);
    let (supervisor_id, supervisor_generation) = bound_supervisor(&driven);
    let quiet_before = agent_session(&fixture.store, IDLE_MEMBER).lifecycle;

    // The window3 death, driven through the real failed-thread/reap route.
    let failing = std::thread::spawn(|| -> CliResult<TeamRunDriveOutcome> {
        Err(CliError::Usage(SUPERVISOR_LOCK_DEATH.into()))
    });
    while !failing.is_finished() {
        std::thread::sleep(Duration::from_millis(5));
    }
    fixture.daemon.push_context(supervisor_failure_context(
        &fixture,
        &supervisor_id,
        supervisor_generation,
        Some(failing),
    ));
    fixture
        .daemon
        .reap_finished()
        .expect("reap joins the failed Supervisor thread");

    assert!(
        fixture
            .store
            .member_actions()
            .expect("read member actions")
            .into_iter()
            .any(|action| action.team_run_id == fixture.run_id
                && action.action_type == "team_supervisor_recovery_required"),
        "the reap must still write the TeamRun-level recovery marker"
    );
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired,
        "the reap route must settle the driven Active Session as RecoveryRequired"
    );
    assert_eq!(
        agent_session(&fixture.store, IDLE_MEMBER).lifecycle,
        quiet_before,
        "an unrelated quiet Session is never settled by a Supervisor failure"
    );
}

/// #937: settlement is bound to the exact failed driver. A lane already
/// re-bound to a successor Supervisor is never marked by a stale failure.
#[test]
fn a_stale_supervisor_failure_never_marks_a_successors_lane() {
    let fixture = drain_fixture("supervisor-stale-generation");
    let ledger = fixture.supervise("supervisor-stale-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:stale:1");
    let (failed_id, failed_generation) =
        bound_supervisor(&agent_session(&fixture.store, MID_TURN_MEMBER));

    let first = supervisor_failure_context(&fixture, &failed_id, failed_generation, None);
    fixture
        .daemon
        .block_finished_supervisor_failure(&first, &CliError::Usage(SUPERVISOR_LOCK_DEATH.into()));
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired,
        "the first failure settles its own lane"
    );

    // Reconcile the lane as proven dead and re-drive it under a successor
    // Supervisor generation.
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_runtime_control(
        &ledger,
        &member,
        RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )
    .expect("detach the reaped lane");
    crate::transition_provider_session_for_member(&ledger, &member, AgentSessionStatus::Idle)
        .expect("reconcile the RecoveryRequired lane to Idle");
    fixture
        .store
        .release_team_supervisor_lease(
            &fixture.run_id,
            &failed_id,
            failed_generation,
            current_unix_ms_u64(),
        )
        .expect("release the dead Supervisor lease");
    let successor = fixture.supervise("supervisor-stale-2", fixture.daemon_generation);
    fixture.start_cycle_for(&successor, "work-delivery:stale:2");
    let (successor_id, successor_generation) =
        bound_supervisor(&agent_session(&fixture.store, MID_TURN_MEMBER));
    assert_ne!(
        (failed_id.clone(), failed_generation),
        (successor_id, successor_generation),
        "the successor binding must be a different driver generation"
    );
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Active,
        "the successor drives the lane again"
    );

    // The stale failure fires again: the successor's live lane stays untouched.
    let stale = supervisor_failure_context(&fixture, &failed_id, failed_generation, None);
    fixture
        .daemon
        .block_finished_supervisor_failure(&stale, &CliError::Usage(SUPERVISOR_LOCK_DEATH.into()));
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Active,
        "a stale Supervisor failure must never mark a lane its driver no longer owns"
    );
}

/// #937: the documented supported recovery route end to end — after the
/// failure, explicit recovery plus a proven-dead lane returns the member to
/// real execution, with no replayed cycle.
#[test]
fn the_supported_recovery_route_returns_the_member_to_real_execution() {
    let fixture = drain_fixture("supervisor-failure-recovery-route");
    let ledger = fixture.supervise("supervisor-route-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:route:1");
    let (supervisor_id, supervisor_generation) =
        bound_supervisor(&agent_session(&fixture.store, MID_TURN_MEMBER));

    let failure = supervisor_failure_context(&fixture, &supervisor_id, supervisor_generation, None);
    fixture.daemon.block_finished_supervisor_failure(
        &failure,
        &CliError::Usage(SUPERVISOR_LOCK_DEATH.into()),
    );
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired
    );

    // 1. Explicit operator recovery of the run (clears the failure hold).
    fixture
        .daemon
        .clear_team_run_supervisor_recovery(
            DRAIN_SPACE_ID,
            &fixture.store,
            &fixture.run_id,
            &supervisor_id,
            supervisor_generation,
        )
        .expect("explicit operator recovery clears the hold");

    // 2. The lane is proven dead (process reaped); journal the member Blocked
    // the way the runner does, then recover it.
    let member = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    crate::transition_provider_session_runtime_control(
        &ledger,
        &member,
        RuntimeResidency::Detached,
        RuntimeActivity::Idle,
    )
    .expect("detach the reaped lane");
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:route-blocked".into());
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the member Blocked the way the runner does");
    let report = crate::team_run_recover(&fixture.store, &fixture.run_id, false)
        .expect("lane-level recovery repairs");
    assert_eq!(
        report["restarted_blocked_members"],
        serde_json::json!(1),
        "a proven-dead RecoveryRequired lane is recovered: {report}"
    );

    // 3. Real execution continues under the next Supervisor generation —
    // exactly one new cycle, never a replay of the killed one.
    fixture
        .store
        .release_team_supervisor_lease(
            &fixture.run_id,
            &supervisor_id,
            supervisor_generation,
            current_unix_ms_u64(),
        )
        .expect("release the dead Supervisor lease");
    let successor = fixture.supervise("supervisor-route-2", fixture.daemon_generation);
    fixture.start_cycle_for(&successor, "work-delivery:route:2");
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Active,
        "the recovered member executes again"
    );
    assert_eq!(
        fixture.start_cycles().len(),
        2,
        "one pre-failure cycle and one post-recovery cycle; nothing replayed"
    );
}

/// #937: the Unknown fence is preserved — without a proven-dead lane the
/// recovery route reports and refuses to restart, never forcing Idle.
#[test]
fn recovery_never_restarts_a_lane_that_is_not_proven_dead() {
    let fixture = drain_fixture("supervisor-failure-unknown-fence");
    let ledger = fixture.supervise("supervisor-unknown-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:unknown:1");
    let (supervisor_id, supervisor_generation) =
        bound_supervisor(&agent_session(&fixture.store, MID_TURN_MEMBER));

    let failure = supervisor_failure_context(&fixture, &supervisor_id, supervisor_generation, None);
    fixture.daemon.block_finished_supervisor_failure(
        &failure,
        &CliError::Usage(SUPERVISOR_LOCK_DEATH.into()),
    );
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired
    );

    // The lane is still Attached/Running — nothing proves the provider died.
    let expected = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    let mut blocked = expected.clone();
    blocked.status = MemberRunStatus::Blocked;
    blocked.last_event_at = Some("unix-ms:unknown-blocked".into());
    fixture
        .store
        .compare_and_append_member_run(&expected, &blocked)
        .expect("journal the member Blocked the way the runner does");
    let report = crate::team_run_recover(&fixture.store, &fixture.run_id, false)
        .expect("recovery reports instead of restarting");
    assert_eq!(
        report["restarted_blocked_members"],
        serde_json::json!(0),
        "an unproven lane is never restarted: {report}"
    );
    assert_eq!(
        member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER).status,
        MemberRunStatus::Blocked,
        "the member stays Blocked while its lane is unproven"
    );
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired,
        "the lane stays honestly RecoveryRequired, never forced Idle"
    );
    assert_eq!(
        fixture.start_cycles().len(),
        1,
        "no new cycle and no replay while the effect is unknown"
    );
}

/// #937: when the Store was unreadable at failure time, the explicit
/// recovery route is the documented completion path — it settles the lane
/// once access returns (idempotently, still bound to the failed driver).
#[test]
fn explicit_recovery_completes_settlement_that_a_store_outage_deferred() {
    let fixture = drain_fixture("supervisor-failure-deferred-settlement");
    let ledger = fixture.supervise("supervisor-deferred-1", fixture.daemon_generation);
    fixture.start_cycle_for(&ledger, "work-delivery:deferred:1");
    let (supervisor_id, supervisor_generation) =
        bound_supervisor(&agent_session(&fixture.store, MID_TURN_MEMBER));

    // The outage boundary: nothing was persisted at failure time — only the
    // volatile adoption hold exists and the lane is still honestly Active.
    fixture
        .daemon
        .insert_volatile_hold((DRAIN_SPACE_ID.into(), fixture.run_id.clone()), None);
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::Active,
        "the deferred boundary leaves the lane honestly unsettled"
    );

    // After access returns, the documented explicit route completes it.
    fixture
        .daemon
        .clear_team_run_supervisor_recovery(
            DRAIN_SPACE_ID,
            &fixture.store,
            &fixture.run_id,
            &supervisor_id,
            supervisor_generation,
        )
        .expect("explicit recovery completes the deferred settlement");
    assert_eq!(
        agent_session(&fixture.store, MID_TURN_MEMBER).lifecycle,
        AgentSessionStatus::RecoveryRequired,
        "the explicit route settles the deferred lane as RecoveryRequired"
    );
}
