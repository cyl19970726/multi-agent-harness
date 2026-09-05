use super::*;

/// GitHub #755 (DEV-232): `RecoveryRequired` had no outgoing transition, so a
/// session that reached it from `Active` (an unrecoverable provider error on
/// an open cycle) or from `Cold` (a failed open) was wedged one state over
/// from #748. It now has exactly one exit, `Idle`, admitted only under the
/// terminated-lane proof the drain exit uses — never on the lifecycle alone —
/// and every other target stays an ordinary invalid transition.
fn current(store: &HarnessStore, session_id: &str) -> AgentSession {
    store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|session| session.id == session_id)
        .expect("AgentSession")
}

fn create_identity(store: &HarnessStore, member: &str) {
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", &format!("identity-{member}"), 0),
            identity(member),
        )
        .unwrap();
}

/// An attached, mid-cycle lane: Active with an open turn.
fn active_lane(store: &HarnessStore, suffix: &str) -> AgentSession {
    let member = format!("recovery-{suffix}");
    create_identity(store, &member);
    let mut lane = session(&format!("session-{member}"), &member);
    lane.native_session_ref = Some(settled_native_session(&format!("thread-{member}")));
    lane.control_state.runtime_residency = RuntimeResidency::Attached;
    lane.control_state.activity = RuntimeActivity::Running;
    store
        .create_agent_session(
            &service_context("session.create", &format!("session-{member}"), 0),
            lane.clone(),
        )
        .unwrap();
    let active = store
        .transition_agent_session(
            &service_context("session.activate", &format!("activate-{member}"), 1),
            &lane.id,
            AgentSessionStatus::Active,
            "t-active",
        )
        .unwrap()
        .projection;
    assert!(active.current_turn_id.is_some());
    active
}

/// A lane whose provider never opened: Cold, detached, quiet.
fn cold_lane(store: &HarnessStore, suffix: &str) -> AgentSession {
    let member = format!("recovery-{suffix}");
    create_identity(store, &member);
    let mut lane = session(&format!("session-{member}"), &member);
    lane.lifecycle = AgentSessionStatus::Cold;
    lane.native_session_ref = Some(settled_native_session(&format!("thread-{member}")));
    lane.control_state.runtime_residency = RuntimeResidency::Detached;
    lane.control_state.activity = RuntimeActivity::Idle;
    store
        .create_agent_session(
            &service_context("session.create", &format!("session-{member}"), 0),
            lane.clone(),
        )
        .unwrap();
    current(store, &lane.id)
}

/// The runner records an unrecoverable provider error on that lane.
fn mark_recovery_required(store: &HarnessStore, lane: &AgentSession) -> AgentSession {
    let failed = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                &format!("recovery-required-{}", lane.id),
                lane.version,
            ),
            &lane.id,
            AgentSessionStatus::RecoveryRequired,
            "t-recovery-required",
        )
        .expect("the runner records an unrecoverable provider error")
        .projection;
    assert_eq!(failed.lifecycle, AgentSessionStatus::RecoveryRequired);
    assert!(failed.current_turn_id.is_none());
    failed
}

/// Record the operator's reconciliation: the handle is gone and the lane is
/// quiet. Every other control-state field keeps its value.
fn detach(store: &HarnessStore, lane: &AgentSession, key: &str) -> AgentSession {
    let mut detached = lane.control_state.clone();
    detached.runtime_residency = RuntimeResidency::Detached;
    detached.activity = RuntimeActivity::Idle;
    store
        .bind_agent_session_control_state(
            &service_context("node_daemon.session.control", key, lane.version),
            &lane.id,
            lane.runtime_generation,
            detached,
            "t-detach",
        )
        .expect("record the dropped handle")
        .projection
}

fn resume_refused(store: &HarnessStore, lane: &AgentSession, key: &str) {
    let operations_before = store.canonical_operations().unwrap();
    let error = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                key,
                lane.version,
            ),
            &lane.id,
            AgentSessionStatus::Idle,
            "t-refused",
        )
        .expect_err("a RecoveryRequired lane that does not prove its runtime gone stays put");
    assert!(
        error
            .to_string()
            .contains(firm_core::agentfirm_api::AGENT_SESSION_RECOVERY_REQUIRED_NOT_YET_RESUMABLE),
        "the fence names itself: {error}"
    );
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before,
        "a refusal writes nothing"
    );
    assert_eq!(
        current(store, &lane.id).lifecycle,
        AgentSessionStatus::RecoveryRequired
    );
}

fn ordinary_invalid(
    store: &HarnessStore,
    lane: &AgentSession,
    next: AgentSessionStatus,
    key: &str,
) {
    let error = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                key,
                lane.version,
            ),
            &lane.id,
            next,
            "t-invalid",
        )
        .expect_err("RecoveryRequired has exactly one exit");
    assert!(
        error.to_string().contains(&format!(
            "invalid AgentSession transition RecoveryRequired->{next:?}"
        )),
        "{error}"
    );
    assert_eq!(
        current(store, &lane.id).lifecycle,
        AgentSessionStatus::RecoveryRequired
    );
}

#[test]
fn recovery_required_lane_resumes_only_after_reconciliation() {
    let (store, root) = fabric_store();

    // Attached and mid-cycle: the one exit is shut, and no other opens.
    let attached = mark_recovery_required(&store, &active_lane(&store, "attached"));
    resume_refused(&store, &attached, "rr-attached-idle");
    ordinary_invalid(
        &store,
        &attached,
        AgentSessionStatus::Closed,
        "rr-attached-closed",
    );
    ordinary_invalid(
        &store,
        &attached,
        AgentSessionStatus::Cold,
        "rr-attached-cold",
    );
    ordinary_invalid(
        &store,
        &attached,
        AgentSessionStatus::Active,
        "rr-attached-active",
    );

    // Detached and quiet, but a RuntimeCommand admitted on the lane is still
    // ambiguous: the canonical #755 shape (a failed open leaves the lane Cold
    // and detached, and `Cold -> RecoveryRequired` is admitted unconditionally)
    // — a resume could replay that command, so the lane stays put until it
    // is settled.
    let cold = cold_lane(&store, "ambiguous");
    let (inspect, inspect_context) = runtime_command_fixture(
        "runtime-rr-ambiguous-inspect",
        RuntimeCommandKind::InspectCommandEffect,
        &cold,
        "inspect_command_effect",
    );
    let admitted = store
        .prepare_runtime_command(&inspect_context, &inspect, current_unix_ms(), "t-inspect")
        .expect("an operator command is admitted on the cold lane");
    let ambiguous = mark_recovery_required(&store, &current(&store, &cold.id));
    assert_eq!(
        ambiguous.control_state.runtime_residency,
        RuntimeResidency::Detached
    );
    resume_refused(&store, &ambiguous, "rr-ambiguous-idle");
    store
        .settle_runtime_command_with_postcondition(
            &service_context(
                "runtime.inspect.settle",
                "runtime-rr-ambiguous-inspect:settle",
                admitted.projection.version,
            ),
            &inspect.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            RuntimePostconditionStatus::Satisfied,
            Some(serde_json::json!({"inspected": true})),
            None,
            "t-inspect-applied",
        )
        .expect("the operator settles the ambiguous command");
    let settled = current(&store, &ambiguous.id);
    let resumed = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                "rr-ambiguous-resumed",
                settled.version,
            ),
            &settled.id,
            AgentSessionStatus::Idle,
            "t-resumed-after-settle",
        )
        .expect("once the command is settled the reconciled lane resumes")
        .projection;
    assert_eq!(resumed.lifecycle, AgentSessionStatus::Idle);

    // Reconciled after an open-cycle failure: detached, idle, disarmed, no
    // turn, no queued input, no ambiguous command — the ordinary lane reopens
    // on the same runtime generation, and the native locator is retained.
    let resumable = mark_recovery_required(&store, &active_lane(&store, "resumable"));
    let resumable = detach(&store, &resumable, "rr-resumable-detach");
    let resumed = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                "rr-resumable-idle",
                resumable.version,
            ),
            &resumable.id,
            AgentSessionStatus::Idle,
            "t-resumed",
        )
        .expect("a reconciled RecoveryRequired lane resumes")
        .projection;
    assert_eq!(resumed.lifecycle, AgentSessionStatus::Idle);
    assert_eq!(resumed.runtime_generation, resumable.runtime_generation);
    assert!(resumed.current_turn_id.is_none());
    assert!(resumed.native_session_ref.is_some());
    // From Idle the ordinary paths apply again, e.g. an ordinary Close.
    let closed = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                "rr-resumable-closed",
                resumed.version,
            ),
            &resumed.id,
            AgentSessionStatus::Closed,
            "t-closed",
        )
        .expect("the resumed lane closes through the ordinary Idle -> Closed edge")
        .projection;
    assert_eq!(closed.lifecycle, AgentSessionStatus::Closed);
    fs::remove_dir_all(root).unwrap();
}

fn successor_context(daemon_id: &str, command: &str, key: &str, expected: u64) -> MutationContext {
    MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: daemon_id.into(),
        },
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version: expected,
        request_fingerprint: None,
    }
}

/// A reconciled `RecoveryRequired` lane (detached, turn-free) has nothing left
/// for a drain to settle, so both drain settlements leave it as it stands and
/// it outlives its NodeDaemon generation. The successor generation must
/// reattach it — the same terminated-lane proof plus the released predecessor
/// lease — and the one exit must then hold under the successor; otherwise the
/// lane has no writer left in any generation (round-3 review P1).
#[test]
fn reconciled_recovery_required_lane_is_reattached_by_the_successor_daemon() {
    let (store, root) = fabric_store();
    let lane = mark_recovery_required(&store, &active_lane(&store, "successor"));
    let lane = detach(&store, &lane, "rr-successor-detach");

    // The exact daemon drains: the reconciled lane is skipped, not rewritten.
    store
        .settle_node_daemon_shutdown_sessions(
            &service_context(
                "node_daemon.shutdown.settle_sessions",
                "rr-successor-drain-settle",
                1,
            ),
            &lane.node_id,
            "daemon-1",
            1,
            "instance-1",
            true,
            "t-drain",
        )
        .expect("the exact daemon settles its own sessions");
    let survived = current(&store, &lane.id);
    assert_eq!(survived.lifecycle, AgentSessionStatus::RecoveryRequired);
    assert_eq!(
        survived.version, lane.version,
        "a reconciled lane has nothing left to settle, so the drain leaves it as it stands"
    );

    // Predecessor settlement, then the successor generation reattaches it.
    let drain_time = current_unix_ms();
    store
        .release_node_daemon_lease(&lane.node_id, "daemon-1", 1, "instance-1", drain_time)
        .expect("the drained daemon releases its own settled lease");
    let successor = store
        .acquire_node_daemon_lease(
            &lane.node_id,
            "daemon-2",
            "instance-2",
            drain_time + 1,
            60_000,
        )
        .expect("successor NodeDaemon generation");
    store
        .reattach_agent_session_to_node_daemon(
            &successor_context(
                &successor.daemon_id,
                "runtime_fabric.session.reattach_node_daemon",
                "rr-successor-reattach",
                survived.version,
            ),
            &lane.id,
            survived.runtime_generation,
            1,
            &successor.daemon_id,
            successor.generation,
            "t-reattach",
        )
        .expect("the successor reattaches a reconciled RecoveryRequired lane");
    let reattached = current(&store, &lane.id);
    assert_eq!(reattached.lifecycle, AgentSessionStatus::RecoveryRequired);
    assert_eq!(reattached.node_daemon_id, successor.daemon_id);
    assert_eq!(reattached.node_daemon_generation, successor.generation);
    assert_eq!(reattached.runtime_generation, lane.runtime_generation);

    // The one exit holds under the successor, on the same runtime generation.
    let resumed = store
        .transition_agent_session(
            &successor_context(
                &successor.daemon_id,
                "node_daemon.agent_session.provider_state",
                "rr-successor-idle",
                reattached.version,
            ),
            &lane.id,
            AgentSessionStatus::Idle,
            "t-resumed-under-successor",
        )
        .expect("the reconciled lane resumes under the successor generation")
        .projection;
    assert_eq!(resumed.lifecycle, AgentSessionStatus::Idle);
    assert_eq!(resumed.node_daemon_generation, successor.generation);
    assert_eq!(resumed.runtime_generation, lane.runtime_generation);
    assert!(resumed.native_session_ref.is_some());
    fs::remove_dir_all(root).unwrap();
}

/// The same lane after a hard crash of its daemon: the predecessor recovery
/// (#837) records the reconciled lane as already settled instead of rewriting
/// it, the successor reattaches it, and the one exit holds there too.
#[test]
fn reconciled_recovery_required_lane_is_reattached_after_a_predecessor_crash() {
    let (store, root) = fabric_store();
    let lane = mark_recovery_required(&store, &active_lane(&store, "crash"));
    let lane = detach(&store, &lane, "rr-crash-detach");

    let recovery_time = current_unix_ms() + 61_000;
    let mut operator = context(
        "host",
        "node_daemon.predecessor_recover",
        "rr-crash-recover",
        0,
    );
    operator.authenticated_actor = ActorRef {
        kind: ActorKind::Service,
        id: lane.node_id.clone(),
    };
    let recovered = store
        .recover_node_daemon_predecessor(
            &operator,
            &lane.node_id,
            "daemon-1",
            1,
            "instance-1",
            true,
            true,
            "operator-check:pid-absent+process-groups-esrch",
            recovery_time,
            "t-crash",
        )
        .expect("exact Operator evidence settles the crashed predecessor");
    assert_eq!(
        recovered.lease.status,
        firm_core::NodeDaemonLeaseStatus::Released
    );
    assert_eq!(recovered.sessions_already_settled, vec![lane.id.clone()]);
    assert!(recovered.sessions_detached.is_empty());
    let survived = current(&store, &lane.id);
    assert_eq!(survived.lifecycle, AgentSessionStatus::RecoveryRequired);
    assert_eq!(
        survived.version, lane.version,
        "recovery never rewrites a settled Session"
    );

    let successor = store
        .acquire_node_daemon_lease(
            &lane.node_id,
            "daemon-2",
            "instance-2",
            recovery_time + 1,
            60_000,
        )
        .expect("successor NodeDaemon generation");
    store
        .reattach_agent_session_to_node_daemon(
            &successor_context(
                &successor.daemon_id,
                "runtime_fabric.session.reattach_node_daemon",
                "rr-crash-reattach",
                survived.version,
            ),
            &lane.id,
            survived.runtime_generation,
            1,
            &successor.daemon_id,
            successor.generation,
            "t-reattach",
        )
        .expect("the successor reattaches the reconciled lane after a crash");
    let reattached = current(&store, &lane.id);
    let resumed = store
        .transition_agent_session(
            &successor_context(
                &successor.daemon_id,
                "node_daemon.agent_session.provider_state",
                "rr-crash-idle",
                reattached.version,
            ),
            &lane.id,
            AgentSessionStatus::Idle,
            "t-resumed-after-crash",
        )
        .expect("the reconciled lane resumes under the successor generation")
        .projection;
    assert_eq!(resumed.lifecycle, AgentSessionStatus::Idle);
    assert_eq!(resumed.node_daemon_generation, successor.generation);
    fs::remove_dir_all(root).unwrap();
}
