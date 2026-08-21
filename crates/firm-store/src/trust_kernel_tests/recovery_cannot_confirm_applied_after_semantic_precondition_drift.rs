use super::*;

#[test]
fn recovery_cannot_confirm_applied_after_semantic_precondition_drift() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "recovery-precondition", 0),
            identity("recovery-precondition"),
        )
        .unwrap();
    let target = session("session-recovery-precondition", "recovery-precondition");
    store
        .create_agent_session(
            &service_context("session.create", "session-recovery-precondition", 0),
            target.clone(),
        )
        .unwrap();
    let (mut command, mut admission) = runtime_command_fixture(
        "recovery-precondition-command",
        RuntimeCommandKind::StopSession,
        &target,
        "stop_session",
    );
    command.precondition.expected_session_version = Some(target.version);
    admission.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
    store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-accepted")
        .unwrap();
    store
        .settle_runtime_command(
            &service_context(
                "node_daemon.runtime.settle",
                "recovery-precondition:settle",
                1,
            ),
            &command.id,
            RuntimeCommandStatus::RecoveryRequired,
            RuntimeEffectCertainty::Unknown,
            None,
            Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
            "t-recovery",
        )
        .unwrap();
    store
        .transition_agent_session(
            &service_context("session.activate", "recovery-precondition:activate", 1),
            &target.id,
            AgentSessionStatus::Active,
            "t-active",
        )
        .unwrap();

    let mut confirm_applied = service_context(
        "operator.runtime.resolve",
        "recovery-precondition:confirm-applied",
        2,
    );
    confirm_applied.authority_actor = Some(ActorRef {
        kind: ActorKind::Service,
        id: target.node_id.clone(),
    });
    let before = store.canonical_operations().unwrap();
    let error = store
        .resolve_runtime_command_recovery(
            &confirm_applied,
            &command.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            RuntimeRecoveryResolution::ConfirmApplied,
            "evidence:provider-claimed-applied",
            "t-confirm-applied",
        )
        .expect_err("stale semantics cannot be promoted to Applied during recovery");
    assert!(error.to_string().contains("expected_session_version"));
    assert_eq!(store.canonical_operations().unwrap(), before);

    let mut confirm_not_applied = service_context(
        "operator.runtime.resolve",
        "recovery-precondition:confirm-not-applied",
        2,
    );
    confirm_not_applied.authority_actor = Some(ActorRef {
        kind: ActorKind::Service,
        id: target.node_id.clone(),
    });
    let resolved = store
        .resolve_runtime_command_recovery(
            &confirm_not_applied,
            &command.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            RuntimeRecoveryResolution::ConfirmNotApplied,
            "evidence:provider-absent",
            "t-confirm-not-applied",
        )
        .expect("stale work must remain safely resolvable as NotApplied");
    assert_eq!(resolved.projection.status, RuntimeCommandStatus::Failed);
    assert_eq!(
        resolved.projection.effect_certainty,
        RuntimeEffectCertainty::NotApplied
    );
    fs::remove_dir_all(root).unwrap();
}
