use super::*;

/// The exact r5c dogfood trap (#837, after the self-stop in #836).
///
/// One Node is recovered twice. The first recovery detaches the member Session
/// of the dead generation. A successor generation then reattaches that Session,
/// settles a `ResumeNativeSession` command as applied and binds the live
/// handle — and afterwards loses machine authority without a drain, leaving its
/// own lease `Active` past expiry.
///
/// The operator runs the same `daemon recover-predecessor` command again, so
/// the caller idempotency prefix is byte-identical to the first recovery's.
/// Before the fix, the per-Session detach key was derived from that prefix
/// alone, so the second recovery collided with the first one's row under a
/// different payload: `IDEMPOTENCY_KEY_REUSED`, the lease could never be
/// released, and every successor `daemon start` failed
/// `NODE_DAEMON_MACHINE_AUTHORITY_LOST` forever.
#[test]
fn predecessor_recovery_is_idempotent_across_daemon_generations() {
    let (store, root) = fabric_store();
    let node_id = "11111111-1111-4111-8111-111111111111";
    for member_id in ["r5c-kimi", "r5c-codex"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context("host", "identity.create", member_id, 0),
                identity(member_id),
            )
            .unwrap();
    }

    // `attached` is mid-lane when authority is lost; `partially_drained` models
    // the Session the dying generation's own incomplete drain already settled.
    let mut attached = session("session-r5c-kimi", "r5c-kimi");
    attached.control_state.runtime_residency = RuntimeResidency::Attached;
    attached.control_state.activity = RuntimeActivity::Idle;
    attached.native_session_ref = Some(recovery_native_session("native-r5c-kimi"));
    let mut partially_drained = session("session-r5c-codex", "r5c-codex");
    partially_drained.control_state.runtime_residency = RuntimeResidency::Attached;
    partially_drained.control_state.activity = RuntimeActivity::Idle;
    partially_drained.native_session_ref = Some(recovery_native_session("native-r5c-codex"));
    for target in [&attached, &partially_drained] {
        store
            .create_agent_session(
                &service_context("session.create", &target.id, 0),
                target.clone(),
            )
            .unwrap();
    }

    // One operator identity for both recoveries: exactly what the CLI passes,
    // one constant prefix for every recovery this Node ever runs.
    let operator = MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: node_id.into(),
        },
        authority_actor: None,
        command_name: "node_daemon.predecessor_recover".into(),
        idempotency_key: format!("cli-daemon-recover-predecessor:{node_id}:space:space-test"),
        expected_version: 1,
        request_fingerprint: None,
    };

    // Recovery 1: the earlier crash of generation 1.
    let first_recovery_at = current_unix_ms() + 61_000;
    let first = store
        .recover_node_daemon_predecessor(
            &operator,
            node_id,
            "daemon-1",
            1,
            "instance-1",
            true,
            true,
            "operator: pid 12406 absent (ps), provider process groups terminated",
            first_recovery_at,
            "t-recover-1",
        )
        .expect("the first predecessor recovery settles generation 1");
    assert_eq!(
        first.lease.status,
        firm_core::NodeDaemonLeaseStatus::Released
    );
    assert_eq!(
        first.sessions_detached,
        vec![
            "session-r5c-codex".to_string(),
            "session-r5c-kimi".to_string()
        ]
    );

    // The successor generation adopts both Sessions.
    let successor = store
        .acquire_node_daemon_lease(
            node_id,
            "daemon-2",
            "instance-2",
            first_recovery_at + 1,
            60_000,
        )
        .expect("successor NodeDaemon generation");
    assert_eq!(successor.generation, 2);
    for session_id in ["session-r5c-kimi", "session-r5c-codex"] {
        let current = recovery_session(&store, session_id);
        store
            .reattach_agent_session_to_node_daemon(
                &daemon_context(
                    "daemon-2",
                    "runtime_fabric.session.reattach_node_daemon",
                    &format!("reattach:{session_id}"),
                    current.version,
                ),
                session_id,
                current.runtime_generation,
                1,
                "daemon-2",
                successor.generation,
                "t-reattach",
            )
            .expect("the released predecessor permits the exact successor reattach");
    }

    // Generation 2 resumes the native session and records the live handle: the
    // exact ledger shape the dogfood daemon left behind (settled
    // `ResumeNativeSession`, then `control_state_bound` with residency
    // attached) before it lost the store lock and self-stopped.
    let reattached = recovery_session(&store, "session-r5c-kimi");
    let (resume, mut resume_context) = runtime_command_fixture(
        "runtime-r5c-resume-native-session",
        RuntimeCommandKind::ResumeNativeSession,
        &reattached,
        "resume_native_session",
    );
    resume_context.authenticated_actor = ActorRef {
        kind: ActorKind::Service,
        id: "daemon-2".into(),
    };
    let accepted = store
        .prepare_runtime_command(&resume_context, &resume, current_unix_ms(), "t-resume")
        .expect("ResumeNativeSession admission under the successor generation");
    store
        .settle_runtime_command_with_postcondition(
            &daemon_context(
                "daemon-2",
                "node_daemon.provider_effect.settle",
                "runtime-r5c-resume-native-session:settle",
                accepted.projection.version,
            ),
            &resume.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            RuntimePostconditionStatus::Satisfied,
            Some(serde_json::json!({
                "phase": "resumed",
                "provider_receipt": {
                    "command": "resume",
                    "response_id": "provider-receipt:r5c",
                    "success": true,
                },
            })),
            None,
            "t-resume-settled",
        )
        .expect("the applied provider effect is settled by its own generation");
    let resumed = recovery_session(&store, "session-r5c-kimi");
    let mut live_handle = resumed.control_state.clone();
    live_handle.runtime_residency = RuntimeResidency::Attached;
    store
        .bind_agent_session_control_state(
            &daemon_context(
                "daemon-2",
                "node_daemon.session.control",
                "r5c-kimi-control-bound",
                resumed.version,
            ),
            &resumed.id,
            resumed.runtime_generation,
            live_handle,
            "t-control-bound",
        )
        .expect("record the observed live provider handle");

    // Generation 2 loses machine authority; its drain cannot write, so its
    // lease stays Active past expiry and one Session stays attached while the
    // other keeps the detached state the partial drain reached.
    assert_eq!(
        recovery_session(&store, "session-r5c-kimi")
            .control_state
            .runtime_residency,
        RuntimeResidency::Attached
    );
    assert_eq!(
        recovery_session(&store, "session-r5c-codex")
            .control_state
            .runtime_residency,
        RuntimeResidency::Detached
    );

    // Recovery 2: same operator command, same caller idempotency prefix, a
    // different generation and evidence ref. It must complete.
    let second_recovery_at = first_recovery_at + 200_000;
    let second = store
        .recover_node_daemon_predecessor(
            &operator,
            node_id,
            "daemon-2",
            successor.generation,
            "instance-2",
            true,
            true,
            "https://github.com/cyl19970726/multi-agent-harness/issues/836",
            second_recovery_at,
            "t-recover-2",
        )
        .expect("a second predecessor recovery of the same Node must not collide with the first");
    assert_eq!(
        second.lease.status,
        firm_core::NodeDaemonLeaseStatus::Released
    );
    assert_eq!(second.sessions_detached, vec!["session-r5c-kimi"]);
    assert_eq!(
        second.sessions_already_settled,
        vec!["session-r5c-codex"],
        "recovery records what the incomplete drain already settled"
    );
    assert_eq!(
        recovery_session(&store, "session-r5c-kimi")
            .control_state
            .runtime_residency,
        RuntimeResidency::Detached
    );

    // The settled ResumeNativeSession is left exactly as its own generation
    // settled it: recovery never re-settles a terminal provider effect.
    let resume_commands = store
        .runtime_commands("space-test")
        .unwrap()
        .into_iter()
        .filter(|command| command.command == RuntimeCommandKind::ResumeNativeSession)
        .collect::<Vec<_>>();
    assert_eq!(resume_commands.len(), 1);
    assert_eq!(resume_commands[0].status, RuntimeCommandStatus::Applied);
    assert_eq!(
        resume_commands[0].effect_certainty,
        RuntimeEffectCertainty::Applied
    );
    assert_eq!(resume_commands[0].phase, RuntimeCommandPhase::Settled);

    // A re-run of the completed recovery stays a no-op.
    let operations_before = store.canonical_operations().unwrap();
    let replay = store
        .recover_node_daemon_predecessor(
            &operator,
            node_id,
            "daemon-2",
            successor.generation,
            "instance-2",
            true,
            true,
            "https://github.com/cyl19970726/multi-agent-harness/issues/836",
            second_recovery_at + 1,
            "t-recover-2-replay",
        )
        .expect("re-running a completed recovery reports the released lease");
    assert_eq!(
        replay.lease.status,
        firm_core::NodeDaemonLeaseStatus::Released
    );
    assert!(replay.sessions_detached.is_empty());
    assert_eq!(store.canonical_operations().unwrap(), operations_before);

    // The successor generation can take machine authority again.
    store
        .acquire_node_daemon_lease(
            node_id,
            "daemon-3",
            "instance-3",
            second_recovery_at + 2,
            60_000,
        )
        .expect("a released predecessor lets the next daemon generation start");
    fs::remove_dir_all(root).unwrap();
}

fn recovery_native_session(native_session_id: &str) -> NativeSessionRef {
    NativeSessionRef {
        provider: "kimi".into(),
        execution_mode: "kimi_acp".into(),
        native_session_id: native_session_id.into(),
        native_locator_kind: "kimi_session".into(),
        provider_version: None,
        adapter_contract_version: "kimi-acp-v1".into(),
        availability: firm_core::agentfirm_api::NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("t1".into()),
        parent_native_session_id: None,
    }
}

fn recovery_session(store: &HarnessStore, session_id: &str) -> AgentSession {
    store
        .fabric_agent_sessions("space-test")
        .unwrap()
        .into_iter()
        .find(|session| session.id == session_id)
        .unwrap_or_else(|| panic!("AgentSession {session_id}"))
}

fn daemon_context(daemon_id: &str, command: &str, key: &str, expected: u64) -> MutationContext {
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
