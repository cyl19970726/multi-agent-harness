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

use harness_core::agentfirm_api::{
    AgentSessionStatus, RuntimeActivity, RuntimeCommandKind, RuntimeCommandStatus,
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
    daemon: MultiTeamDaemon,
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
    crate::latest_member_runs_in_append_order(store)
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
        provider: "codex".into(),
        execution_mode: Some("codex_app_server".into()),
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

    let daemon = MultiTeamDaemon {
        firm_home,
        node_id: node_id.clone(),
        daemon_id,
        instance_id: "drain-instance".into(),
        contexts: Mutex::new(Vec::new()),
        supervisor_start_gate: Mutex::new(()),
        session_runtimes: Mutex::new(HashMap::new()),
        native_session_wake_endpoint: Arc::new(Mutex::new(HashMap::new())),
        max_concurrency: 1,
        idle_timeout_secs: 1,
        scan_interval: Duration::from_secs(1),
        stop_requested: Arc::new(AtomicBool::new(false)),
        authority_shutdown: Arc::new(AtomicBool::new(false)),
        authority_lost: AtomicBool::new(false),
        control_worker_failed: AtomicBool::new(false),
        recovery_blocked_runs: Mutex::new(HashMap::new()),
        settling_runs: Mutex::new(HashSet::new()),
        lease_ttl_override_ms: None,
        deferred_stop_responses: Mutex::new(Vec::new()),
        drain_timeout_override_ms: None,
    };

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
                &self.daemon.daemon_id,
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
        let run = crate::latest_team_run(&self.store, &self.run_id).expect("TeamRun");
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
            &self.daemon.daemon_id,
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
            true,
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
                &self.daemon.daemon_id,
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

    let successor_generation = fixture.readopt();
    let ledger = fixture.supervise("supervisor-drain-2", successor_generation);
    let mid_turn = member_named(&fixture.store, &fixture.run_id, MID_TURN_MEMBER);
    // The exact call the r5 run could not make: the re-adopted member re-enters
    // its wake loop, which idles the Session first.
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
    assert_eq!(after[0].status, RuntimeCommandStatus::Applied);
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
