use super::*;

/// The exact r5 dogfood sequence (#748, follows #746): a NodeDaemon drain kills
/// the owned provider process groups and settles every mid-turn AgentSession as
/// `Interrupted`; the predecessor lease is released; a successor generation
/// reattaches the Session and must be able to resume it.
///
/// Before this regression, `Interrupted` had no exit into the ordinary lane, so
/// one drain wedged every mid-turn member forever.
#[test]
fn drained_session_resumes_under_the_next_daemon_generation() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "drain-mid-turn", 0),
            identity("drain-mid-turn"),
        )
        .unwrap();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "drain-idle", 0),
            identity("drain-idle"),
        )
        .unwrap();

    let mut mid_turn = session("session-drain-mid-turn", "drain-mid-turn");
    mid_turn.native_session_ref = Some(drain_native_session("native-drain-mid-turn"));
    store
        .create_agent_session(
            &service_context("session.create", "session-drain-mid-turn", 0),
            mid_turn.clone(),
        )
        .unwrap();
    let mut idle = session("session-drain-idle", "drain-idle");
    idle.control_state.runtime_residency = RuntimeResidency::Detached;
    idle.control_state.activity = RuntimeActivity::Idle;
    idle.native_session_ref = Some(drain_native_session("native-drain-idle"));
    store
        .create_agent_session(
            &service_context("session.create", "session-drain-idle", 0),
            idle.clone(),
        )
        .unwrap();

    // The mid-turn member is executing one StartCycle when the drain arrives.
    let activated = store
        .transition_agent_session(
            &service_context("session.activate", "drain-mid-turn-active", 1),
            &mid_turn.id,
            AgentSessionStatus::Active,
            "t-active",
        )
        .unwrap()
        .projection;
    assert!(activated.current_turn_id.is_some());
    let (mut start, mut start_context) = runtime_command_fixture(
        "runtime-drain-start-cycle",
        RuntimeCommandKind::StartCycle,
        &mid_turn,
        "start_cycle",
    );
    start.payload["provider_attempt"] = serde_json::json!(1);
    start.payload_fingerprint = canonical_json_fingerprint(&start.payload);
    start_context.request_fingerprint = Some(runtime_command_envelope_fingerprint(&start).unwrap());
    let accepted = store
        .prepare_runtime_command(&start_context, &start, current_unix_ms(), "t-start")
        .expect("StartCycle admission");
    // The daemon refuses to detach a Session whose provider effect is still
    // ambiguous, so the killed cycle reaches the drain already settled: a
    // terminal, non-replayable fact rather than a resumable instruction.
    store
        .settle_runtime_command_with_postcondition(
            &service_context(
                "node_daemon.provider_effect.settle",
                "runtime-drain-start-cycle:settle",
                accepted.projection.version,
            ),
            &start.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            RuntimePostconditionStatus::Satisfied,
            Some(serde_json::json!({
                "phase": "input_accepted",
                "provider_receipt": {
                    "command": "deliver",
                    "response_id": "provider-receipt:drain",
                    "success": true,
                },
            })),
            None,
            "t-start-settled",
        )
        .unwrap();

    // The drain: owned provider process groups terminated, then session detach.
    store
        .settle_node_daemon_shutdown_sessions(
            &service_context("node_daemon.shutdown.settle_sessions", "drain-settle", 1),
            &mid_turn.node_id,
            "daemon-1",
            1,
            "instance-1",
            true,
            "t-drain",
        )
        .expect("the exact daemon settles its own sessions after killing its process groups");

    let drained = drain_session(&store, &mid_turn.id);
    assert_eq!(drained.lifecycle, AgentSessionStatus::Interrupted);
    assert_eq!(
        drained.control_state.runtime_residency,
        RuntimeResidency::Detached
    );
    assert!(drained.current_turn_id.is_none());
    let untouched_idle = drain_session(&store, &idle.id);
    assert_eq!(
        untouched_idle.lifecycle,
        AgentSessionStatus::Idle,
        "a member idle at drain time keeps its resumable lifecycle"
    );
    assert_eq!(untouched_idle.version, idle.version);

    // Predecessor settlement, then the successor generation.
    let drain_time = current_unix_ms();
    store
        .release_node_daemon_lease(&mid_turn.node_id, "daemon-1", 1, "instance-1", drain_time)
        .expect("the drained daemon releases its own settled lease");
    let successor = store
        .acquire_node_daemon_lease(
            &mid_turn.node_id,
            "daemon-2",
            "instance-2",
            drain_time + 1,
            60_000,
        )
        .expect("successor NodeDaemon generation");
    for session_id in [mid_turn.id.as_str(), idle.id.as_str()] {
        let current = drain_session(&store, session_id);
        store
            .reattach_agent_session_to_node_daemon(
                &successor_context(
                    &successor.daemon_id,
                    "runtime_fabric.session.reattach_node_daemon",
                    &format!("reattach:{session_id}"),
                    current.version,
                ),
                session_id,
                current.runtime_generation,
                1,
                &successor.daemon_id,
                successor.generation,
                "t-reattach",
            )
            .expect("explicit predecessor release permits exact successor reattach");
    }

    // The resume the r5 run could never reach: `Interrupted -> Idle`.
    let reattached = drain_session(&store, &mid_turn.id);
    let resumed = store
        .transition_agent_session(
            &successor_context(
                &successor.daemon_id,
                "node_daemon.agent_session.provider_state",
                "drain-mid-turn-resume",
                reattached.version,
            ),
            &mid_turn.id,
            AgentSessionStatus::Idle,
            "t-resume",
        )
        .expect("a drained Session with a provably terminated runtime must resume")
        .projection;
    assert_eq!(resumed.lifecycle, AgentSessionStatus::Idle);
    assert!(resumed.current_turn_id.is_none());
    assert_eq!(
        resumed
            .native_session_ref
            .map(|native| native.native_session_id),
        Some("native-drain-mid-turn".to_string()),
        "resume keeps the provider-native session identity"
    );

    // The idle member resumes on the ordinary path and is unaffected.
    let idle_after = drain_session(&store, &idle.id);
    store
        .transition_agent_session(
            &successor_context(
                &successor.daemon_id,
                "node_daemon.agent_session.provider_state",
                "drain-idle-resume",
                idle_after.version,
            ),
            &idle.id,
            AgentSessionStatus::Active,
            "t-idle-resume",
        )
        .expect("an idle member is unaffected by the drain");

    // The killed cycle stays exactly one settled, non-replayed command.
    let start_cycles = store
        .runtime_commands("space-test")
        .unwrap()
        .into_iter()
        .filter(|command| command.command == RuntimeCommandKind::StartCycle)
        .collect::<Vec<_>>();
    assert_eq!(
        start_cycles.len(),
        1,
        "resume must open a new cycle, never replay the killed one"
    );
    assert_eq!(start_cycles[0].status, RuntimeCommandStatus::Applied);
    assert_eq!(
        start_cycles[0].effect_certainty,
        RuntimeEffectCertainty::Applied
    );
    assert_eq!(start_cycles[0].phase, RuntimeCommandPhase::Settled);
    assert_eq!(start_cycles[0].target_node_daemon_generation, 1);
    fs::remove_dir_all(root).unwrap();
}

/// The relaxed transition is proof-gated, not a free exit from `Interrupted`.
#[test]
fn interrupted_session_resume_requires_a_provably_terminated_lane() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "drain-fenced", 0),
            identity("drain-fenced"),
        )
        .unwrap();
    let mut target = session("session-drain-fenced", "drain-fenced");
    target.native_session_ref = Some(drain_native_session("native-drain-fenced"));
    store
        .create_agent_session(
            &service_context("session.create", "session-drain-fenced", 0),
            target.clone(),
        )
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.activate", "drain-fenced-active", 1),
            &target.id,
            AgentSessionStatus::Active,
            "t-active",
        )
        .unwrap();
    store
        .settle_node_daemon_shutdown_sessions(
            &service_context(
                "node_daemon.shutdown.settle_sessions",
                "drain-fenced-settle",
                1,
            ),
            &target.node_id,
            "daemon-1",
            1,
            "instance-1",
            true,
            "t-drain",
        )
        .unwrap();
    let interrupted = drain_session(&store, &target.id);
    assert_eq!(interrupted.lifecycle, AgentSessionStatus::Interrupted);

    // A lane that claims a live provider handle is not proof that the killed
    // runtime is gone, so the resume stays refused and writes nothing.
    let mut attached_control = interrupted.control_state.clone();
    attached_control.runtime_residency = RuntimeResidency::Attached;
    let attached = store
        .bind_agent_session_control_state(
            &service_context(
                "node_daemon.session.control",
                "drain-fenced-attach",
                interrupted.version,
            ),
            &target.id,
            interrupted.runtime_generation,
            attached_control,
            "t-attach",
        )
        .expect("record the observed live handle")
        .projection;
    let operations_before = store.canonical_operations().unwrap();
    let error = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                "drain-fenced-resume",
                attached.version,
            ),
            &target.id,
            AgentSessionStatus::Idle,
            "t-resume",
        )
        .expect_err("an attached lane cannot prove the interrupted runtime is terminated");
    assert!(
        error.to_string().contains("detached, disarmed lane"),
        "unexpected error: {error}"
    );
    assert_eq!(store.canonical_operations().unwrap(), operations_before);
    assert_eq!(
        drain_session(&store, &target.id).lifecycle,
        AgentSessionStatus::Interrupted
    );

    // A resume can never step over an ambiguous provider effect either.
    let mut detached_control = attached.control_state.clone();
    detached_control.runtime_residency = RuntimeResidency::Detached;
    store
        .bind_agent_session_control_state(
            &service_context(
                "node_daemon.session.control",
                "drain-fenced-detach",
                attached.version,
            ),
            &target.id,
            attached.runtime_generation,
            detached_control,
            "t-detach",
        )
        .expect("record the dropped handle");
    let mut ambiguous_target = drain_session(&store, &target.id);
    ambiguous_target.lifecycle = AgentSessionStatus::Active;
    let (stop, stop_context) = runtime_command_fixture(
        "runtime-drain-fenced-stop",
        RuntimeCommandKind::StopSession,
        &ambiguous_target,
        "stop_session",
    );
    store
        .prepare_runtime_command(&stop_context, &stop, current_unix_ms(), "t-stop")
        .expect("an exact StopSession may be admitted on the interrupted lane");
    let operations_before = store.canonical_operations().unwrap();
    let refreshed = drain_session(&store, &target.id);
    let error = store
        .transition_agent_session(
            &service_context(
                "node_daemon.agent_session.provider_state",
                "drain-fenced-resume-ambiguous",
                refreshed.version,
            ),
            &target.id,
            AgentSessionStatus::Idle,
            "t-resume-ambiguous",
        )
        .expect_err("an ambiguous RuntimeCommand must be reconciled before any resume");
    assert!(
        error.to_string().contains("ambiguous RuntimeCommand"),
        "unexpected error: {error}"
    );
    assert_eq!(store.canonical_operations().unwrap(), operations_before);
    fs::remove_dir_all(root).unwrap();
}

fn drain_native_session(native_session_id: &str) -> NativeSessionRef {
    NativeSessionRef {
        provider: "codex".into(),
        execution_mode: "codex_app_server".into(),
        native_session_id: native_session_id.into(),
        native_locator_kind: "codex_thread".into(),
        provider_version: Some("0.148.0-alpha.9".into()),
        adapter_contract_version: "codex-app-server-v1".into(),
        availability: firm_core::agentfirm_api::NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("t1".into()),
        parent_native_session_id: None,
    }
}

fn drain_session(store: &HarnessStore, session_id: &str) -> AgentSession {
    store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|session| session.id == session_id)
        .unwrap_or_else(|| panic!("AgentSession {session_id}"))
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
