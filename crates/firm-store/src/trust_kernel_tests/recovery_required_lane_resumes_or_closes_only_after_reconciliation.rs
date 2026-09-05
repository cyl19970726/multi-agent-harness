use super::*;

/// GitHub #755 (DEV-232): `RecoveryRequired` had no outgoing transition, so a
/// session that reached it from `Active` (an unrecoverable provider error on
/// an open cycle, written best-effort by the runner) was wedged one state over
/// from #748. It now exits to `Idle`, `Cold`, or `Closed` under exactly the
/// terminated-lane proof the drain exit uses — never on the lifecycle alone.
fn current(store: &HarnessStore, session_id: &str) -> AgentSession {
    store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|session| session.id == session_id)
        .expect("AgentSession")
}

/// An attached, mid-cycle lane: Active with an open turn.
fn active_lane(store: &HarnessStore, suffix: &str) -> AgentSession {
    let member = format!("recovery-{suffix}");
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", &format!("identity-{member}"), 0),
            identity(&member),
        )
        .unwrap();
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

/// The runner records an unrecoverable provider error on that lane.
fn mark_recovery_required(store: &HarnessStore, active: &AgentSession) -> AgentSession {
    let failed = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                &format!("recovery-required-{}", active.id),
                active.version,
            ),
            &active.id,
            AgentSessionStatus::RecoveryRequired,
            "t-recovery-required",
        )
        .expect("the runner records an unrecoverable provider error")
        .projection;
    assert_eq!(failed.lifecycle, AgentSessionStatus::RecoveryRequired);
    assert!(failed.current_turn_id.is_none());
    failed
}

/// An attached, mid-cycle lane that the runner just marked `RecoveryRequired`.
fn recovery_required_lane(store: &HarnessStore, suffix: &str) -> AgentSession {
    let active = active_lane(store, suffix);
    mark_recovery_required(store, &active)
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

fn refused(store: &HarnessStore, lane: &AgentSession, next: AgentSessionStatus, key: &str) {
    let operations_before = store.canonical_operations().unwrap();
    let error = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                key,
                lane.version,
            ),
            &lane.id,
            next,
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

#[test]
fn recovery_required_lane_resumes_or_closes_only_after_reconciliation() {
    let (store, root) = fabric_store();

    // Attached and mid-cycle: no exit yet, for any target.
    let attached = recovery_required_lane(&store, "attached");
    refused(
        &store,
        &attached,
        AgentSessionStatus::Idle,
        "rr-attached-idle",
    );
    refused(
        &store,
        &attached,
        AgentSessionStatus::Closed,
        "rr-attached-closed",
    );
    refused(
        &store,
        &attached,
        AgentSessionStatus::Cold,
        "rr-attached-cold",
    );

    // Detached and quiet, but a RuntimeCommand admitted on the lane is still
    // ambiguous: a resume could replay it, so the lane stays put.
    // The command is admitted while the cycle is still Active (a stop never
    // targets a terminal lane); the provider failure then lands on top of it.
    let ambiguous_active = active_lane(&store, "ambiguous");
    let command_target = ambiguous_active.clone();
    let (stop, stop_context) = runtime_command_fixture(
        "runtime-rr-ambiguous-stop",
        RuntimeCommandKind::StopSession,
        &command_target,
        "stop_session",
    );
    store
        .prepare_runtime_command(&stop_context, &stop, current_unix_ms(), "t-stop")
        .expect("an exact StopSession may be admitted on the active lane");
    let ambiguous = mark_recovery_required(&store, &ambiguous_active);
    // Reconciliation must come first: the control state cannot even record
    // the dropped handle while the command's effect is ambiguous, and the
    // lane's exits stay shut until it is settled.
    let mut detached = ambiguous.control_state.clone();
    detached.runtime_residency = RuntimeResidency::Detached;
    detached.activity = RuntimeActivity::Idle;
    let error = store
        .bind_agent_session_control_state(
            &service_context(
                "node_daemon.session.control",
                "rr-ambiguous-detach",
                ambiguous.version,
            ),
            &ambiguous.id,
            ambiguous.runtime_generation,
            detached,
            "t-detach",
        )
        .expect_err("an ambiguous command blocks reconciliation of the control state");
    assert!(
        error
            .to_string()
            .contains("reconciliation of every ambiguous RuntimeCommand"),
        "{error}"
    );
    let ambiguous = current(&store, &ambiguous.id);
    refused(
        &store,
        &ambiguous,
        AgentSessionStatus::Idle,
        "rr-ambiguous-idle",
    );
    refused(
        &store,
        &ambiguous,
        AgentSessionStatus::Closed,
        "rr-ambiguous-closed",
    );

    // Reconciled: detached, idle, disarmed, no turn, no queued input, no
    // ambiguous command — the ordinary lane reopens.
    let resumable = recovery_required_lane(&store, "resumable");
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

    // ...or closes, retaining the native session locator as history.
    let closable = recovery_required_lane(&store, "closable");
    let closable = detach(&store, &closable, "rr-closable-detach");
    let closed = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                "rr-closable-closed",
                closable.version,
            ),
            &closable.id,
            AgentSessionStatus::Closed,
            "t-closed",
        )
        .expect("a reconciled RecoveryRequired lane closes")
        .projection;
    assert_eq!(closed.lifecycle, AgentSessionStatus::Closed);
    assert!(closed.closed_at.is_some());
    assert!(closed.native_session_ref.is_some());

    // ...or goes Cold for an explicit ResumeSession.
    let coldable = recovery_required_lane(&store, "coldable");
    let coldable = detach(&store, &coldable, "rr-coldable-detach");
    let cold = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                "rr-coldable-cold",
                coldable.version,
            ),
            &coldable.id,
            AgentSessionStatus::Cold,
            "t-cold",
        )
        .expect("a reconciled RecoveryRequired lane may go Cold")
        .projection;
    assert_eq!(cold.lifecycle, AgentSessionStatus::Cold);

    // The exits are exactly Idle / Cold / Closed: nothing else opens up.
    let stuck = recovery_required_lane(&store, "stuck");
    let stuck = detach(&store, &stuck, "rr-stuck-detach");
    let error = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                "rr-stuck-active",
                stuck.version,
            ),
            &stuck.id,
            AgentSessionStatus::Active,
            "t-active-again",
        )
        .expect_err("RecoveryRequired never jumps straight back into a cycle");
    assert!(
        error
            .to_string()
            .contains("invalid AgentSession transition RecoveryRequired->Active"),
        "{error}"
    );
    fs::remove_dir_all(root).unwrap();
}
