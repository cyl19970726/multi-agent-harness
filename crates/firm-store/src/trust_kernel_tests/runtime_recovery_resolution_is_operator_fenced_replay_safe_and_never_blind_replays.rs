use super::*;

#[test]
fn operator_cannot_race_an_active_prepared_command() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-abandoned-prepared", 0),
            identity("abandoned-prepared"),
        )
        .unwrap();
    let target_session = session("session-abandoned-prepared", "abandoned-prepared");
    store
        .create_agent_session(
            &service_context("session.create", "session-abandoned-prepared", 0),
            target_session.clone(),
        )
        .unwrap();
    let (command, admission_context) = runtime_command_fixture(
        "runtime-abandoned-prepared",
        RuntimeCommandKind::ResumeNativeSession,
        &target_session,
        "runtime.native_session.resume",
    );
    store
        .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t-prepare")
        .unwrap();

    let mut resolve_context = service_context(
        "operator.runtime.resolve",
        "runtime-abandoned-prepared:confirm-not-applied",
        1,
    );
    resolve_context.authority_actor = Some(ActorRef {
        kind: ActorKind::Service,
        id: target_session.node_id.clone(),
    });
    let error = store
        .resolve_runtime_command_recovery(
            &resolve_context,
            &command.id,
            &target_session.node_id,
            &target_session.node_daemon_id,
            target_session.node_daemon_generation,
            RuntimeRecoveryResolution::ConfirmNotApplied,
            "evidence:premature",
            "t-rejected",
        )
        .expect_err("Operator recovery cannot race an actively owned Prepared command");
    assert!(error
        .to_string()
        .contains("only an Unknown RecoveryRequired"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runtime_recovery_resolution_is_operator_fenced_replay_safe_and_never_blind_replays() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-recovery-agent", 0),
            identity("recovery-agent"),
        )
        .unwrap();
    let target_session = session("session-recovery-agent", "recovery-agent");
    store
        .create_agent_session(
            &service_context("session.create", "session-recovery-agent", 0),
            target_session.clone(),
        )
        .unwrap();
    let (command, admission_context) = runtime_command_fixture(
        "runtime-recovery-command",
        RuntimeCommandKind::StopSession,
        &target_session,
        "stop_session",
    );
    store
        .prepare_runtime_command(&admission_context, &command, current_unix_ms(), "t-prepare")
        .unwrap();
    let mut settle_context = service_context(
        "node_daemon.runtime.settle",
        "runtime-recovery-command:settle",
        1,
    );
    settle_context.authority_actor = Some(command.authenticated_actor.clone());
    store
        .settle_runtime_command(
            &settle_context,
            &command.id,
            RuntimeCommandStatus::RecoveryRequired,
            RuntimeEffectCertainty::Unknown,
            None,
            Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
            "t-ambiguous",
        )
        .unwrap();

    let operations_before_hostile = store.canonical_operations().unwrap();
    let mut hostile = service_context(
        "operator.runtime.resolve",
        "runtime-recovery-command:hostile",
        2,
    );
    hostile.authority_actor = Some(ActorRef {
        kind: ActorKind::Service,
        id: "sibling-node".into(),
    });
    let rejected = store
        .resolve_runtime_command_recovery(
            &hostile,
            &command.id,
            &target_session.node_id,
            &target_session.node_daemon_id,
            target_session.node_daemon_generation,
            RuntimeRecoveryResolution::ConfirmApplied,
            "evidence:hostile",
            "t-hostile",
        )
        .expect_err("a sibling Operator cannot resolve another Node's effect");
    assert!(rejected
        .to_string()
        .contains("exact Execution Node Operator"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_hostile
    );

    let mut resolve_context = service_context(
        "operator.runtime.resolve",
        "runtime-recovery-command:resolve",
        2,
    );
    resolve_context.authority_actor = Some(ActorRef {
        kind: ActorKind::Service,
        id: target_session.node_id.clone(),
    });
    let resolved = store
        .resolve_runtime_command_recovery(
            &resolve_context,
            &command.id,
            &target_session.node_id,
            &target_session.node_daemon_id,
            target_session.node_daemon_generation,
            RuntimeRecoveryResolution::ConfirmNotApplied,
            "evidence:provider-process-absent",
            "t-resolved",
        )
        .unwrap();
    assert_eq!(resolved.projection.status, RuntimeCommandStatus::Failed);
    assert_eq!(
        resolved.projection.effect_certainty,
        RuntimeEffectCertainty::NotApplied
    );
    assert_eq!(
        resolved.projection.result.as_ref().unwrap()["blind_replay"],
        false
    );
    let operations_after_resolution = store.canonical_operations().unwrap();
    let replay = store
        .resolve_runtime_command_recovery(
            &resolve_context,
            &command.id,
            &target_session.node_id,
            &target_session.node_daemon_id,
            target_session.node_daemon_generation,
            RuntimeRecoveryResolution::ConfirmNotApplied,
            "evidence:provider-process-absent",
            "t-replay",
        )
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_after_resolution
    );

    let conflict = store
        .resolve_runtime_command_recovery(
            &resolve_context,
            &command.id,
            &target_session.node_id,
            &target_session.node_daemon_id,
            target_session.node_daemon_generation,
            RuntimeRecoveryResolution::ConfirmApplied,
            "evidence:different-semantics",
            "t-conflict",
        )
        .expect_err("same key with changed resolution must conflict");
    assert!(conflict.to_string().contains("IDEMPOTENCY_KEY_REUSED"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_after_resolution
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_runtime_recovery_resolution_projects_exact_success_and_replays_without_delta() {
    let (store, root) = fabric_store();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "identity-recovery-outcomes", 0),
            identity("recovery-outcomes"),
        )
        .unwrap();
    let target_session = session("session-recovery-outcomes", "recovery-outcomes");
    store
        .create_agent_session(
            &service_context("session.create", "session-recovery-outcomes", 0),
            target_session.clone(),
        )
        .unwrap();

    for (suffix, resolution, status, phase, certainty, failure_code) in [
        (
            "applied",
            RuntimeRecoveryResolution::ConfirmApplied,
            RuntimeCommandStatus::Applied,
            RuntimeCommandPhase::Settled,
            RuntimeEffectCertainty::Applied,
            None,
        ),
        (
            "not-applied",
            RuntimeRecoveryResolution::ConfirmNotApplied,
            RuntimeCommandStatus::Failed,
            RuntimeCommandPhase::Rejected,
            RuntimeEffectCertainty::NotApplied,
            Some("RECOVERY_CONFIRMED_NOT_APPLIED"),
        ),
        (
            "keep-required",
            RuntimeRecoveryResolution::KeepRecoveryRequired,
            RuntimeCommandStatus::RecoveryRequired,
            RuntimeCommandPhase::RecoveryRequired,
            RuntimeEffectCertainty::Unknown,
            Some("RECOVERY_EVIDENCE_INSUFFICIENT"),
        ),
    ] {
        let command_id = format!("runtime-recovery-outcome-{suffix}");
        let (command, admission_context) = runtime_command_fixture(
            &command_id,
            RuntimeCommandKind::StopSession,
            &target_session,
            "stop_session",
        );
        store
            .prepare_runtime_command(
                &admission_context,
                &command,
                current_unix_ms(),
                &format!("t-{suffix}-prepare"),
            )
            .unwrap();
        let mut settle_context = service_context(
            "node_daemon.runtime.settle",
            &format!("{command_id}:settle"),
            1,
        );
        settle_context.authority_actor = Some(command.authenticated_actor.clone());
        store
            .settle_runtime_command(
                &settle_context,
                &command.id,
                RuntimeCommandStatus::RecoveryRequired,
                RuntimeEffectCertainty::Unknown,
                None,
                Some("PROVIDER_EFFECT_AMBIGUOUS".into()),
                &format!("t-{suffix}-ambiguous"),
            )
            .unwrap();

        let mut resolve_context = service_context(
            "operator.runtime.resolve",
            &format!("{command_id}:resolve"),
            2,
        );
        resolve_context.authority_actor = Some(ActorRef {
            kind: ActorKind::Service,
            id: target_session.node_id.clone(),
        });
        let evidence_ref = format!("evidence:{suffix}");
        let resolved = store
            .resolve_runtime_command_recovery(
                &resolve_context,
                &command.id,
                &target_session.node_id,
                &target_session.node_daemon_id,
                target_session.node_daemon_generation,
                resolution,
                &evidence_ref,
                &format!("t-{suffix}-resolved"),
            )
            .unwrap();
        assert_eq!(resolved.projection.status, status, "{suffix}");
        assert_eq!(resolved.projection.phase, phase, "{suffix}");
        assert_eq!(resolved.projection.effect_certainty, certainty, "{suffix}");
        assert_eq!(
            resolved.projection.failure_code.as_deref(),
            failure_code,
            "{suffix}"
        );
        assert_eq!(resolved.projection.version, 3, "{suffix}");
        assert_eq!(
            resolved.projection.result.as_ref().unwrap(),
            &serde_json::json!({
                "resolution": resolution,
                "evidence_ref": evidence_ref,
                "blind_replay": false,
            }),
            "{suffix}"
        );

        let operations_after_resolution = store.canonical_operations().unwrap();
        let replay = store
            .resolve_runtime_command_recovery(
                &resolve_context,
                &command.id,
                &target_session.node_id,
                &target_session.node_daemon_id,
                target_session.node_daemon_generation,
                resolution,
                &format!("evidence:{suffix}"),
                &format!("t-{suffix}-replay"),
            )
            .unwrap();
        assert!(replay.replayed, "{suffix}");
        assert_eq!(replay.projection, resolved.projection, "{suffix}");
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_after_resolution,
            "{suffix} replay must have zero durable delta"
        );

        let changed_evidence = store
            .resolve_runtime_command_recovery(
                &resolve_context,
                &command.id,
                &target_session.node_id,
                &target_session.node_daemon_id,
                target_session.node_daemon_generation,
                resolution,
                &format!("evidence:{suffix}:changed"),
                &format!("t-{suffix}-conflict"),
            )
            .expect_err("same key with changed evidence must conflict");
        assert!(
            changed_evidence
                .to_string()
                .contains("IDEMPOTENCY_KEY_REUSED"),
            "{suffix}: {changed_evidence}"
        );
        assert_eq!(
            store.canonical_operations().unwrap(),
            operations_after_resolution,
            "{suffix} semantic conflict must have zero durable delta"
        );
    }

    fs::remove_dir_all(root).unwrap();
}
