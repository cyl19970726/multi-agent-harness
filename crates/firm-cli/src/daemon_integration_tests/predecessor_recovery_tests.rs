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
            .seed_machine_authority_for_test(
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
        // Simulating elapsed time, not a write: `expires` is monotonic within a
        // status (ADR 0075), so no API may shorten a live lease — that guard is
        // what bounds a backwards clock step. The document is written directly
        // for the same reason wall-clock time would have done it.
        let expired = self
            .store()
            .expire_machine_lease_for_test(self.daemon().node_id())
            .expect("expire the predecessor machine lease");
        let _ = self.store().renew_node_daemon_lease(
            self.daemon().node_id(),
            &lease.daemon_id,
            lease.generation,
            &lease.instance_id,
            current_unix_ms_u64(),
            1,
        );
        // Unambiguously in the past rather than racing the clock.
        std::thread::sleep(Duration::from_millis(5));
        expired
    }

    /// Who owns this machine now, asked of the record that decides it.
    ///
    /// The legacy Space row stopped being that record at cutover: the daemon
    /// writes only the document, so a row left behind by a predecessor would
    /// answer "the predecessor" forever (ADR 0075).
    fn current_lease(&self) -> harness_core::NodeDaemonLease {
        self.store()
            .current_authorized_machine_lease(self.daemon().node_id())
            .expect("read the machine lease")
            .expect("a machine lease")
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
    let lease = fixture.current_lease();
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

    let lease = fixture.current_lease();
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
    assert_eq!(fixture.current_lease().generation, dead.generation);

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
        .seed_machine_authority_for_test(
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
    assert_eq!(fixture.current_lease().generation, dead.generation);
    assert_eq!(fixture.current_lease().daemon_id, "dead-daemon");

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

#[test]
fn a_failed_bundle_rolls_back_the_lease_its_own_recovery_just_made_acquirable() {
    let mut fixture = RecoveryFixture::new("recovery-rollback");
    // A lease that has already expired by the time the bundle revalidates it
    // fails that revalidation — the same partial state a later Space's
    // failure produces, without racing a second Store. The Store floors every
    // TTL at 1 ms and the bundle acquires and revalidates inside one call, so
    // on a fast runner both land in the same millisecond and the lease is
    // still valid at revalidation (#990). The delay seam makes the bundle
    // sample its revalidation clock only after that millisecond has passed.
    fixture.inner.daemon.set_lease_ttl_override(Some(1));
    fixture.inner.daemon.set_bundle_revalidation_delay(Some(5));
    let dead = fixture.expire(&fixture.seed_predecessor(ABSENT_PID));

    let error = fixture
        .daemon()
        .ensure_node_authority_bundle()
        .expect_err("a bundle whose revalidation fails must not report authority");
    assert!(
        error
            .to_string()
            .contains("NODE_DAEMON_MACHINE_AUTHORITY_LOST"),
        "{error}"
    );

    // The recovery released the predecessor and acquired the next generation,
    // so that lease is this scan's own newly acquired one and must come back
    // Released — not be left behind because the flag was decided before the
    // recovery ran.
    let lease = fixture.current_lease();
    assert_eq!(lease.daemon_id, fixture.daemon().daemon_id());
    assert_eq!(lease.generation, dead.generation + 1);
    assert_eq!(
        lease.status,
        harness_core::NodeDaemonLeaseStatus::Released,
        "a lease this scan acquired over its own recovery must be rolled back"
    );
}

const DRAIN_JOURNAL_EXACT: &str = "daemon_integration_tests::predecessor_recovery_tests::a_stop_whose_drain_never_converges_journals_its_phases_and_flags_its_lanes";

/// The real stop path, not the marker helper: a Supervisor that ignores the
/// drain makes `graceful_shutdown` fail, and the generation must still leave
/// both records behind. Isolated in a child process because the stop path
/// sweeps the process-global provider process-group registry (#928).
#[test]
fn a_stop_whose_drain_never_converges_journals_its_phases_and_flags_its_lanes() {
    super::process_isolation::run_in_isolated_child(
        DRAIN_JOURNAL_EXACT,
        a_stop_whose_drain_never_converges_journals_its_phases_and_flags_its_lanes_body,
    );
}

fn a_stop_whose_drain_never_converges_journals_its_phases_and_flags_its_lanes_body() {
    let mut fixture = RecoveryFixture::new("drain-journal");
    // 2 s cooperative / 50 ms forced, the same bounds `stop_drain_tests` had
    // to settle on: the cooperative bound is shared with the recovery-scanner
    // wait, and a tighter one lets a loaded runner name the scanner as the
    // failed phase instead of the Supervisor that is actually spinning (#774).
    fixture
        .inner
        .daemon
        .set_drain_timeout_override(Some((2_000, 50)));
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
        "session-drain-journal",
        fixture.daemon().daemon_id(),
        owned.generation,
    );

    let release = Arc::new(AtomicBool::new(false));
    let thread_release = Arc::clone(&release);
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    // A Supervisor that ignores the drain entirely.
    let thread = std::thread::spawn(move || -> CliResult<TeamRunDriveOutcome> {
        while !thread_release.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        finished_tx.send(()).expect("publish provider thread exit");
        Ok(TeamRunDriveOutcome::Progressed {
            team_run_status: harness_core::TeamRunStatus::Completed,
        })
    });
    fixture
        .daemon()
        .push_context(OwnedTestContext::new(TestContextConfig {
            execution_space_id: fixture.inner.execution_space_id.clone(),
            project_binding_id: "unit-test-project".to_string(),
            run_id: fixture.inner.run_id.clone(),
            daemon_generation: owned.generation,
            supervisor_id: "drain-journal-supervisor".to_string(),
            supervisor_generation: 1,
            heartbeat_valid: Arc::new(AtomicBool::new(true)),
            serving_status: Arc::new(Mutex::new("running".to_string())),
            thread: Some(thread),
            started_at: Instant::now(),
        }));

    let execution_space_id = fixture.inner.execution_space_id.clone();
    let run_id = fixture.inner.run_id.clone();
    // Unix socket paths are bounded by SUN_LEN, well below a fixture tree
    // path, so the control socket lives directly under the temp root.
    let socket_path =
        std::env::temp_dir().join(format!("e1a-drain-journal-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("bind drain-journal control socket");
    listener
        .set_nonblocking(true)
        .expect("configure nonblocking listener");

    let super::adoption_tests::AdoptionFixture {
        _tree,
        store,
        daemon,
        ..
    } = fixture.inner;
    let daemon = Arc::new(daemon);

    std::thread::scope(|scope| {
        let serving = Arc::clone(&daemon);
        let server = scope.spawn(move || serving.serve_loop(&listener));
        let mut client = UnixStream::connect(&socket_path).expect("connect stop client");
        client
            .set_read_timeout(Some(Duration::from_secs(30)))
            .expect("bound stop response wait");
        let request = serde_json::json!({
            "cmd": "stop",
            "execution_space_id": execution_space_id,
            "daemon_generation": owned.generation,
        });
        writeln!(client, "{request}").expect("send stop request");
        client.flush().expect("flush stop request");
        let mut raw = String::new();
        std::io::BufReader::new(&mut client)
            .read_line(&mut raw)
            .expect("stop answers within the bounded drain window");
        let response: serde_json::Value =
            serde_json::from_str(raw.trim()).expect("stop response is complete JSON");
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["failed_phase"], "supervisor_drain", "{response}");
        assert!(server.join().expect("serve thread").is_err());
    });

    // The phases land, under the drain's own reason — never borrowing the
    // authority-loss reason, and never claiming a phase that did not happen.
    let phases = store
        .current_team_run_events(&run_id)
        .expect("read TeamRun events")
        .into_iter()
        .filter(|event| event.entity_type == "node_daemon" && event.operation == "self_stopped")
        .map(|event| {
            serde_json::from_str::<serde_json::Value>(&event.summary)
                .expect("structured self-stop summary")
        })
        .collect::<Vec<_>>();
    assert!(
        !phases.is_empty(),
        "an unconverged drain must journal its self-stop phases"
    );
    for phase in &phases {
        assert_eq!(phase["reason"], "NODE_DAEMON_DRAIN_INCOMPLETE", "{phase}");
        assert_eq!(phase["daemon_generation"], owned.generation);
    }
    let observed = phases
        .iter()
        .map(|phase| phase["phase"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    assert!(
        observed.contains(&"shutdown_initiated".to_string()),
        "{observed:?}"
    );
    assert!(
        observed.contains(&"drain_incomplete".to_string()),
        "{observed:?}"
    );
    assert!(
        !observed.contains(&"shutdown_complete".to_string())
            && !observed.contains(&"process_groups_terminated".to_string()),
        "a drain that did not converge must not claim it finished: {observed:?}"
    );

    // And the lane it could not settle is flagged, with nothing claiming it is
    // settled.
    let session = store
        .fabric_agent_sessions(&execution_space_id)
        .expect("read AgentSessions")
        .into_iter()
        .find(|session| session.id == "session-drain-journal")
        .expect("seeded AgentSession");
    let incomplete = session
        .control_state
        .settlement_incomplete
        .as_ref()
        .expect("the unsettled lane must be on the record");
    assert!(
        incomplete.reason.contains("NODE_DAEMON_DRAIN_INCOMPLETE"),
        "{}",
        incomplete.reason
    );
    assert_eq!(incomplete.node_daemon_generation, owned.generation);
    assert_eq!(
        session.control_state.runtime_residency,
        harness_core::agentfirm_api::RuntimeResidency::Attached,
        "a drain that proved nothing must not read as a settlement"
    );

    release.store(true, Ordering::Release);
    finished_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the spinning Supervisor thread exits after the test releases it");
    let _ = std::fs::remove_file(&socket_path);
    drop(_tree);
}
