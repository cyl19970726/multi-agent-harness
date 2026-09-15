//! ADR 0073: the successor recovers a predecessor whose death is proven, and
//! every authority-loss path leaves a record of the lanes it could not settle.
//!
//! The proofs are the contract here. Each refusal test removes exactly one
//! proof and asserts that recovery refuses by name, because the value of
//! automatic recovery is entirely in what it still refuses to do.

use super::adoption_tests::adoption_fixture;
use super::*;

/// A pid above every supported `pid_max`, so the process is provably absent.
const ABSENT_PID: &str = "2147483647";

struct RecoveryFixture {
    inner: super::adoption_tests::AdoptionFixture,
}

impl RecoveryFixture {
    fn new(label: &str) -> Self {
        let mut fixture = adoption_fixture(label);
        // The Supervisor lease and the machine authority must name the exact
        // Node the fixture TeamRun is bound to.
        fixture.daemon.set_node_identity(
            crate::daemon_support::latest_team_run(&fixture.store, &fixture.run_id)
                .expect("fixture TeamRun")
                .execution_node_id,
        );
        // The Node id the fixture TeamRun binds to is a constant, while the
        // recovery diagnostics are process-global: give every fixture its own
        // daemon instance so parallel tests read their own attempt.
        fixture
            .daemon
            .set_instance_id(format!("recovery-instance-{label}"));
        let node_id = fixture.daemon.node_id().to_string();
        if !fixture
            .store
            .latest_execution_nodes()
            .expect("read Nodes")
            .iter()
            .any(|node| node.id == node_id)
        {
            fixture
                .store
                .insert_execution_node(&harness_core::ExecutionNode {
                    id: node_id.clone(),
                    display_name: "Predecessor recovery test Node".to_string(),
                    status: harness_core::ExecutionNodeStatus::Active,
                    created_at: "unix-ms:1".to_string(),
                    updated_at: "unix-ms:1".to_string(),
                })
                .expect("insert recovery test Node");
        }
        fixture
            .store
            .register_node_project(
                &harness_core::NodeProjectRegistration {
                    node_id,
                    execution_space_id: fixture.execution_space_id.clone(),
                    project_binding_id: "unit-test-project".to_string(),
                    status: harness_core::NodeProjectRegistrationStatus::Active,
                    created_at: "unix-ms:1".to_string(),
                    updated_at: "unix-ms:1".to_string(),
                },
                &fixture.execution_space_id,
            )
            .expect("register recovery test project");
        Self { inner: fixture }
    }

    fn store(&self) -> &HarnessStore {
        &self.inner.store
    }

    fn daemon(&self) -> &TestDaemon {
        &self.inner.daemon
    }

    /// Seed one live, unreleased predecessor lease owned by a foreign daemon.
    /// It stays valid so the test can seed the lanes and Supervisor lease that
    /// generation owned; `expire` then ends its life.
    fn seed_predecessor(&self, instance_pid: &str) -> harness_core::NodeDaemonLease {
        let instance_id = format!("{instance_pid}:{}:dead-daemon", current_unix_ms_u64());
        self.store()
            .acquire_node_daemon_lease(
                self.daemon().node_id(),
                "dead-daemon",
                &instance_id,
                current_unix_ms_u64(),
                3_600_000,
            )
            .expect("seed predecessor lease")
    }

    /// End the predecessor's lease life the way a dead daemon does: its last
    /// renewal simply stops being in the future.
    fn expire(&self, lease: &harness_core::NodeDaemonLease) -> harness_core::NodeDaemonLease {
        let expired = self
            .store()
            .renew_node_daemon_lease(
                self.daemon().node_id(),
                &lease.daemon_id,
                lease.generation,
                &lease.instance_id,
                current_unix_ms_u64(),
                1,
            )
            .expect("expire the predecessor lease");
        // A 1ms TTL is in the past by the time the scan reads it; make that
        // unambiguous rather than racing the clock.
        std::thread::sleep(Duration::from_millis(5));
        expired
    }

    fn latest_lease(&self) -> harness_core::NodeDaemonLease {
        self.store()
            .latest_node_daemon_lease(self.daemon().node_id())
            .expect("read latest lease")
            .expect("a lease row")
    }

    /// One AgentSession owned by an exact daemon generation, attached and
    /// running: a lane that is not settled by any measure.
    fn seed_running_session(&self, session_id: &str, daemon_id: &str, generation: u64) {
        self.seed_session("agent-builder-a", session_id, daemon_id, generation, false)
    }

    /// One AgentSession of the same generation that is already at rest.
    fn seed_settled_session(&self, session_id: &str, daemon_id: &str, generation: u64) {
        self.seed_session("host", session_id, daemon_id, generation, true)
    }

    /// An AgentMember may own only one current AgentSession, so each lane in a
    /// test belongs to its own member.
    fn seed_session(
        &self,
        agent_member_id: &str,
        session_id: &str,
        daemon_id: &str,
        generation: u64,
        at_rest: bool,
    ) {
        self.store()
            .create_agent_session(
                &harness_core::agentfirm_api::MutationContext {
                    execution_space_id: self.inner.execution_space_id.clone(),
                    authenticated_actor: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: daemon_id.to_string(),
                    },
                    authority_actor: None,
                    command_name: "test.session.create".into(),
                    idempotency_key: format!("seed-session:{session_id}"),
                    expected_version: 0,
                    request_fingerprint: None,
                },
                harness_core::agentfirm_api::AgentSession {
                    id: session_id.to_string(),
                    agent_member_id: agent_member_id.to_string(),
                    node_id: self.daemon().node_id().to_string(),
                    execution_space_id: self.inner.execution_space_id.clone(),
                    node_daemon_id: daemon_id.to_string(),
                    node_daemon_generation: generation,
                    provider_kind: "codex".into(),
                    provider_profile_ref: "test".into(),
                    permission_envelope_ref: format!("agent-member:{agent_member_id}:permission"),
                    effective_permission_ceiling:
                        harness_core::agentfirm_api::PermissionCeiling::WorkspaceWrite,
                    workspace_cwd: None,
                    lifecycle: if at_rest {
                        harness_core::agentfirm_api::AgentSessionStatus::Idle
                    } else {
                        harness_core::agentfirm_api::AgentSessionStatus::Active
                    },
                    runtime_generation: 1,
                    control_state: harness_core::agentfirm_api::AgentSessionControlState {
                        runtime_residency: if at_rest {
                            harness_core::agentfirm_api::RuntimeResidency::Detached
                        } else {
                            harness_core::agentfirm_api::RuntimeResidency::Attached
                        },
                        activity: if at_rest {
                            harness_core::agentfirm_api::RuntimeActivity::Idle
                        } else {
                            harness_core::agentfirm_api::RuntimeActivity::Running
                        },
                        driver_generation: 1,
                        driver_ref: harness_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
                            node_daemon_id: daemon_id.to_string(),
                            node_daemon_generation: generation,
                        },
                        ..Default::default()
                    },
                    native_session_ref: None,
                    current_cycle_marker: None,
                    queued_input_count: 0,
                    version: 1,
                    opened_at: "unix-ms:1".into(),
                    last_active_at: "unix-ms:1".into(),
                    closed_at: None,
                },
            )
            .expect("seed AgentSession");
    }

    fn session(&self, session_id: &str) -> harness_core::agentfirm_api::AgentSession {
        self.store()
            .fabric_agent_sessions(&self.inner.execution_space_id)
            .expect("read AgentSessions")
            .into_iter()
            .find(|session| session.id == session_id)
            .expect("seeded AgentSession")
    }

    fn recovery_diagnostic(&self) -> Option<serde_json::Value> {
        let id = format!(
            "{}:{}:predecessor-recovery",
            self.daemon().node_id(),
            self.daemon().instance_id()
        );
        crate::lease_renewal_diagnostics::snapshot()
            .into_iter()
            .find(|row| row["id"] == serde_json::json!(id))
    }
}

#[test]
fn automatic_recovery_settles_a_proven_dead_predecessor_and_takes_the_next_generation() {
    let fixture = RecoveryFixture::new("auto-recover-dead");
    let live = fixture.seed_predecessor(ABSENT_PID);
    fixture.seed_running_session("session-dead-generation", "dead-daemon", live.generation);
    // The predecessor was supervising this TeamRun, so the recovery has a
    // Host-visible place to journal itself.
    fixture
        .store()
        .acquire_team_supervisor_under_node_lease(
            &fixture.inner.run_id,
            fixture.daemon().node_id(),
            "dead-daemon",
            live.generation,
            &fixture.inner.execution_space_id,
            "unit-test-project",
            "dead-supervisor",
            4242,
            "test:dead",
            current_unix_ms_u64(),
            1,
        )
        .expect("seed predecessor Supervisor lease");
    let dead = fixture.expire(&live);

    let spaces = fixture
        .daemon()
        .ensure_node_authority_bundle()
        .expect("a proven-dead predecessor must not block the successor");
    assert!(spaces.contains(&fixture.inner.execution_space_id));

    // The successor owns the next generation, acquired over a lease that the
    // recovery explicitly Released rather than one it stole.
    let lease = fixture.latest_lease();
    assert_eq!(lease.daemon_id, fixture.daemon().daemon_id());
    assert_eq!(lease.instance_id, fixture.daemon().instance_id());
    assert_eq!(lease.generation, dead.generation + 1);
    assert_eq!(lease.status, harness_core::NodeDaemonLeaseStatus::Active);

    // The dead generation's lane is settled, not merely forgotten.
    let session = fixture.session("session-dead-generation");
    assert_eq!(
        session.control_state.runtime_residency,
        harness_core::agentfirm_api::RuntimeResidency::Detached
    );
    assert_eq!(
        session.lifecycle,
        harness_core::agentfirm_api::AgentSessionStatus::Interrupted
    );

    // The journal carries the proofs, not a summary of them.
    let events = fixture
        .store()
        .current_team_run_events(&fixture.inner.run_id)
        .expect("read TeamRun events");
    let recovered = events
        .iter()
        .find(|event| {
            event.entity_type == "node_daemon"
                && event.operation == "predecessor_recovered_automatically"
        })
        .expect("automatic recovery must be journaled on the supervised TeamRun");
    let summary = serde_json::from_str::<serde_json::Value>(&recovered.summary)
        .expect("structured recovery summary");
    assert_eq!(summary["instance_id"], dead.instance_id.as_str());
    assert_eq!(summary["status"], "released");
    assert_eq!(summary["recovered_by"]["automatic"], true);
    assert_eq!(
        summary["recovered_by"]["instance_id"],
        fixture.daemon().instance_id()
    );
    assert_eq!(summary["process_death_proof"]["reason"], "process_absent");
    assert_eq!(
        summary["process_death_proof"]["pid"],
        ABSENT_PID.parse::<i64>().expect("absent pid")
    );
    assert_eq!(summary["predecessor_expires_unix_ms"], dead.expires_unix_ms);
    assert_eq!(
        summary["space_settlements"][0]["sessions_detached"],
        serde_json::json!(["session-dead-generation"])
    );

    // Nothing refused, so the diagnostics row carries no proof failure.
    let diagnostic = fixture
        .recovery_diagnostic()
        .expect("recovery attempt is reported by daemon status");
    assert_eq!(diagnostic["kind"], "node_daemon_predecessor_recovery");
    assert_eq!(diagnostic["last_error"], serde_json::Value::Null);
}

#[test]
fn automatic_recovery_refuses_a_predecessor_lease_that_has_not_expired() {
    let fixture = RecoveryFixture::new("auto-recover-unexpired");
    let dead = fixture.seed_predecessor(ABSENT_PID);

    let error = fixture
        .daemon()
        .ensure_node_authority_bundle()
        .expect_err("an unexpired predecessor lease must still fence the successor");
    assert!(
        error
            .to_string()
            .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "{error}"
    );

    let lease = fixture.latest_lease();
    assert_eq!(lease.daemon_id, "dead-daemon");
    assert_eq!(lease.generation, dead.generation);
    assert_eq!(lease.status, harness_core::NodeDaemonLeaseStatus::Active);

    let diagnostic = fixture
        .recovery_diagnostic()
        .expect("the refused attempt is reported by daemon status");
    let last_error = diagnostic["last_error"]
        .as_str()
        .expect("a named proof failure");
    assert!(
        last_error.contains("NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE")
            && last_error.contains("lease_not_expired"),
        "{last_error}"
    );
}

#[test]
fn automatic_recovery_refuses_a_predecessor_process_that_still_exists() {
    let fixture = RecoveryFixture::new("auto-recover-live");
    // This test process is the predecessor: expired lease, live pid. The
    // starved-but-alive daemon is exactly the case that must stay human-gated.
    let live = fixture.seed_predecessor(&std::process::id().to_string());
    let dead = fixture.expire(&live);

    let error = fixture
        .daemon()
        .ensure_node_authority_bundle()
        .expect_err("a live predecessor process must fence the successor");
    assert!(
        error
            .to_string()
            .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "{error}"
    );
    assert_eq!(fixture.latest_lease().generation, dead.generation);

    let diagnostic = fixture
        .recovery_diagnostic()
        .expect("the refused attempt is reported by daemon status");
    let last_error = diagnostic["last_error"]
        .as_str()
        .expect("a named proof failure");
    assert!(
        last_error.contains("NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE")
            && last_error.contains("process_alive"),
        "{last_error}"
    );
}

#[test]
fn automatic_recovery_refuses_two_unreleased_predecessor_instances() {
    let fixture = RecoveryFixture::new("auto-recover-ambiguous");
    let dead = fixture.expire(&fixture.seed_predecessor(ABSENT_PID));
    // A second registered Execution Space holding a different unreleased
    // instance: recovery must not sweep an instance nobody asked about.
    let other = crate::execution_space::register_and_activate(
        &fixture.inner.firm_home(),
        "second-space",
        "Second Space",
        Some("unit-test-project".to_string()),
        None,
        "unix-ms:1",
    )
    .expect("register second Execution Space");
    let other_store = HarnessStore::new(other.store_root.clone());
    other_store.init().expect("initialize second Store");
    other_store
        .insert_execution_node(&harness_core::ExecutionNode {
            id: fixture.daemon().node_id().to_string(),
            display_name: "Predecessor recovery test Node".to_string(),
            status: harness_core::ExecutionNodeStatus::Active,
            created_at: "unix-ms:1".to_string(),
            updated_at: "unix-ms:1".to_string(),
        })
        .expect("insert Node in the second Space");
    other_store
        .acquire_node_daemon_lease(
            fixture.daemon().node_id(),
            "other-dead-daemon",
            &format!("{ABSENT_PID}:1:other-dead-daemon"),
            current_unix_ms_u64(),
            1,
        )
        .expect("seed a second unreleased instance");
    std::thread::sleep(Duration::from_millis(5));

    let error = fixture
        .daemon()
        .ensure_node_authority_bundle()
        .expect_err("two unreleased instances must fence the successor");
    assert!(
        error
            .to_string()
            .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "{error}"
    );
    assert_eq!(fixture.latest_lease().generation, dead.generation);
    assert_eq!(fixture.latest_lease().daemon_id, "dead-daemon");

    let last_error = fixture
        .recovery_diagnostic()
        .expect("the refused attempt is reported by daemon status")["last_error"]
        .as_str()
        .expect("a named proof failure")
        .to_string();
    assert!(
        last_error.contains("SUPERVISOR_GENERATION_FENCED")
            && last_error.contains("different unreleased predecessor instances"),
        "{last_error}"
    );
}

#[test]
fn a_fenced_heartbeat_records_the_lanes_it_could_not_settle() {
    let fixture = RecoveryFixture::new("loss-records-lane");
    let space = fixture
        .daemon()
        .registered_spaces()
        .expect("list Spaces")
        .into_iter()
        .find_map(|(space, _)| (space.id == fixture.inner.execution_space_id).then_some(space))
        .expect("the fixture Execution Space");
    let owned = fixture
        .daemon()
        .ensure_node_authority(&space, fixture.store())
        .expect("acquire the fixture lease");
    fixture.seed_running_session(
        "session-lost-authority",
        fixture.daemon().daemon_id(),
        owned.generation,
    );

    // Another instance of this daemon id holds the authority this generation
    // thought it had: the next heartbeat is fenced, exactly as it is when a
    // starved daemon's lease is taken over.
    fixture.daemon().remember_node_lease(
        &fixture.inner.execution_space_id,
        fixture.store(),
        &harness_core::NodeDaemonLease {
            instance_id: format!("{}-superseded", owned.instance_id),
            ..owned.clone()
        },
    );
    let error = fixture
        .daemon()
        .refresh_held_node_authorities()
        .expect_err("the fenced heartbeat must lose machine authority");
    assert!(
        error
            .to_string()
            .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "{error}"
    );

    // The lane is flagged, and nothing about it claims to be settled: the
    // dying generation had no process-group termination proof to offer.
    let session = fixture.session("session-lost-authority");
    let incomplete = session
        .control_state
        .settlement_incomplete
        .as_ref()
        .expect("an unsettled lane must be on the record");
    assert_eq!(incomplete.node_daemon_id, fixture.daemon().daemon_id());
    assert_eq!(incomplete.node_daemon_generation, owned.generation);
    assert_eq!(incomplete.instance_id, fixture.daemon().instance_id());
    assert!(
        incomplete
            .reason
            .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "{}",
        incomplete.reason
    );
    assert_eq!(
        session.control_state.runtime_residency,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        "a record of failure must never be written as a settlement"
    );
    assert_eq!(
        session.lifecycle,
        harness_core::agentfirm_api::AgentSessionStatus::Active
    );
    assert!(session.current_cycle_marker.is_none());
}

#[test]
fn an_unconverged_drain_records_the_lanes_it_could_not_settle_exactly_once() {
    let fixture = RecoveryFixture::new("drain-incomplete-record");
    let space = fixture
        .daemon()
        .registered_spaces()
        .expect("list Spaces")
        .into_iter()
        .find_map(|(space, _)| (space.id == fixture.inner.execution_space_id).then_some(space))
        .expect("the fixture Execution Space");
    let owned = fixture
        .daemon()
        .ensure_node_authority(&space, fixture.store())
        .expect("acquire the fixture lease");
    fixture.seed_running_session(
        "session-drain-incomplete",
        fixture.daemon().daemon_id(),
        owned.generation,
    );
    // A lane already detached and idle is settled; flagging it would be a
    // false record.
    fixture.seed_settled_session(
        "session-at-rest",
        fixture.daemon().daemon_id(),
        owned.generation,
    );

    let reason = "NODE_DAEMON_DRAIN_INCOMPLETE: a Supervisor did not converge within the documented stop bound";
    let recorded = fixture
        .daemon()
        .record_settlement_incomplete_markers(reason);
    assert_eq!(
        recorded,
        vec!["session-drain-incomplete".to_string()],
        "only the lane that is actually unsettled goes on the record"
    );
    let flagged = fixture.session("session-drain-incomplete");
    assert_eq!(
        flagged
            .control_state
            .settlement_incomplete
            .as_ref()
            .expect("flag")
            .reason,
        reason
    );
    assert_eq!(
        flagged.control_state.runtime_residency,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        "the drain proved nothing, so the record must claim nothing"
    );
    assert!(fixture
        .session("session-at-rest")
        .control_state
        .settlement_incomplete
        .is_none());

    // The same observation twice is a replay, not a second record.
    assert!(
        fixture
            .daemon()
            .record_settlement_incomplete_markers(reason)
            .is_empty(),
        "an identical observation must not rewrite the lane"
    );
    // A different reason is a different observation of the same lane, and the
    // latest one stands: a generation that first lost its drain and then its
    // lease must be able to say so.
    let second_reason =
        "NODE_DAEMON_MACHINE_AUTHORITY_LOST: the lease expired before the drain converged";
    assert_eq!(
        fixture
            .daemon()
            .record_settlement_incomplete_markers(second_reason),
        vec!["session-drain-incomplete".to_string()]
    );
    assert_eq!(
        fixture
            .session("session-drain-incomplete")
            .control_state
            .settlement_incomplete
            .expect("flag")
            .reason,
        second_reason
    );
}

#[test]
fn recovery_settles_and_clears_a_lane_a_dead_generation_flagged() {
    let fixture = RecoveryFixture::new("recovery-clears-flag");
    let live = fixture.seed_predecessor(ABSENT_PID);
    fixture.seed_running_session("session-flagged", "dead-daemon", live.generation);
    fixture
        .store()
        .acquire_team_supervisor_under_node_lease(
            &fixture.inner.run_id,
            fixture.daemon().node_id(),
            "dead-daemon",
            live.generation,
            &fixture.inner.execution_space_id,
            "unit-test-project",
            "dead-supervisor",
            4242,
            "test:dead",
            current_unix_ms_u64(),
            1,
        )
        .expect("seed predecessor Supervisor lease");
    // The predecessor's own last honest act before it went dark.
    let flagged = fixture
        .store()
        .record_node_daemon_settlement_incomplete(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: fixture.inner.execution_space_id.clone(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: "dead-daemon".into(),
                },
                authority_actor: None,
                command_name: "node_daemon.settlement_incomplete".into(),
                idempotency_key: "predecessor-flag".into(),
                expected_version: live.generation,
                request_fingerprint: None,
            },
            fixture.daemon().node_id(),
            "dead-daemon",
            live.generation,
            &live.instance_id,
            "NODE_DAEMON_DRAIN_INCOMPLETE: the predecessor never proved its process groups terminal",
            "unix-ms:5",
        )
        .expect("flag the unsettled lane");
    assert_eq!(flagged, vec!["session-flagged".to_string()]);
    fixture.expire(&live);

    fixture
        .daemon()
        .ensure_node_authority_bundle()
        .expect("a proven-dead predecessor must not block the successor");

    // Recovery is the first party with a termination proof, so it settles the
    // flagged lane and clears the flag rather than trusting the dead
    // generation's last look at it.
    let session = fixture.session("session-flagged");
    assert!(session.control_state.settlement_incomplete.is_none());
    assert_eq!(
        session.control_state.runtime_residency,
        harness_core::agentfirm_api::RuntimeResidency::Detached
    );
    assert_eq!(
        session.lifecycle,
        harness_core::agentfirm_api::AgentSessionStatus::Interrupted
    );

    let events = fixture
        .store()
        .current_team_run_events(&fixture.inner.run_id)
        .expect("read TeamRun events");
    let summary = events
        .iter()
        .find(|event| event.operation == "predecessor_recovered_automatically")
        .map(|event| {
            serde_json::from_str::<serde_json::Value>(&event.summary).expect("structured summary")
        })
        .expect("automatic recovery must be journaled");
    assert_eq!(
        summary["space_settlements"][0]["sessions_settlement_incomplete"],
        serde_json::json!(["session-flagged"]),
        "the receipt must name the lanes that were known to be unsettled"
    );
}

#[test]
fn only_the_exact_daemon_service_may_record_its_own_unsettled_lanes() {
    let fixture = RecoveryFixture::new("flag-authorization");
    let live = fixture.seed_predecessor(ABSENT_PID);
    fixture.seed_running_session("session-guarded", "dead-daemon", live.generation);
    let context = |actor: &str| harness_core::agentfirm_api::MutationContext {
        execution_space_id: fixture.inner.execution_space_id.clone(),
        authenticated_actor: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Service,
            id: actor.into(),
        },
        authority_actor: None,
        command_name: "node_daemon.settlement_incomplete".into(),
        idempotency_key: "guarded-flag".into(),
        expected_version: live.generation,
        request_fingerprint: None,
    };

    let foreign = fixture
        .store()
        .record_node_daemon_settlement_incomplete(
            &context("some-other-daemon"),
            fixture.daemon().node_id(),
            "dead-daemon",
            live.generation,
            &live.instance_id,
            "NODE_DAEMON_DRAIN_INCOMPLETE: test",
            "unix-ms:5",
        )
        .expect_err("only the exact daemon Service may record its own lanes");
    assert!(
        foreign
            .to_string()
            .contains("NODE_DAEMON_SETTLEMENT_INCOMPLETE_UNAUTHORIZED"),
        "{foreign}"
    );

    let unexplained = fixture
        .store()
        .record_node_daemon_settlement_incomplete(
            &context("dead-daemon"),
            fixture.daemon().node_id(),
            "dead-daemon",
            live.generation,
            &live.instance_id,
            "   ",
            "unix-ms:5",
        )
        .expect_err("an unsettled lane must name why it could not be settled");
    assert!(
        unexplained
            .to_string()
            .contains("NODE_DAEMON_SETTLEMENT_INCOMPLETE_REASON_REQUIRED"),
        "{unexplained}"
    );
    assert!(fixture
        .session("session-guarded")
        .control_state
        .settlement_incomplete
        .is_none());
}
