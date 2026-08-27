use super::*;

#[test]
fn runtime_command_team_supervisor_generation_is_live_fenced_at_prepare_and_settle() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-supervisor", "run-supervisor");
    let lease = store
        .acquire_team_supervisor_under_node_lease(
            "run-supervisor",
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "space-test",
            "project-1",
            "supervisor-1",
            std::process::id(),
            "loopback://supervisor-1",
            current_unix_ms(),
            60_000,
        )
        .unwrap();
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "supervised-agent", 0),
            identity("supervised-agent"),
        )
        .unwrap();
    join_runtime_membership(
        &store,
        "membership-supervised-agent",
        "team-supervisor",
        "supervised-agent",
        TeamMembershipRole::Member,
    );
    let mut target = session("session-supervised", "supervised-agent");
    target.control_state.driver_ref = RuntimeDriverRef::TeamSupervisor {
        team_run_id: "run-supervisor".into(),
        team_supervisor_id: lease.supervisor_id.clone(),
        team_supervisor_generation: lease.generation,
    };
    store
        .create_agent_session(
            &service_context("session.create", "session-supervised", 0),
            target.clone(),
        )
        .unwrap();
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-supervised", 0),
            MemberRun {
                id: "member-run-supervised".into(),
                agent_member_id: "supervised-agent".into(),
                team_run_id: "run-supervisor".into(),
                role_snapshot: "member".into(),
                provider_profile_snapshot: None,
                requested_controls: serde_json::json!({}),
                effective_controls: serde_json::json!({}),
                coordination_status: MemberCoordinationStatus::Active,
                runtime_status: MemberRuntimeStatus::Idle,
                runtime_generation: 1,
                workspace_binding_id: None,
                native_session: None,
                version: 1,
                started_at: "t1".into(),
                last_event_at: None,
                finished_at: None,
            },
        )
        .unwrap();

    let (mut command, mut admission) = runtime_command_fixture(
        "supervisor-live-command",
        RuntimeCommandKind::OpenRuntime,
        &target,
        "open_runtime",
    );
    command.binding.target_member_run_id = Some("member-run-supervised".into());
    command.binding.target_member_run_generation = Some(1);
    admission.request_fingerprint = Some(runtime_command_envelope_fingerprint(&command).unwrap());
    let mut unbound = command.clone();
    unbound.binding.target_member_run_id = None;
    unbound.binding.target_member_run_generation = None;
    let mut unbound_admission = admission.clone();
    unbound_admission.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&unbound).unwrap());
    let before = store.canonical_operations().unwrap();
    let error = store
        .prepare_runtime_command(&unbound_admission, &unbound, current_unix_ms(), "t-unbound")
        .expect_err("TeamSupervisor RuntimeCommand without MemberRun binding must fail closed");
    assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before);
    for hostile_binding in [("member-run-foreign", 1), ("member-run-supervised", 2)] {
        let mut hostile = command.clone();
        hostile.binding.target_member_run_id = Some(hostile_binding.0.into());
        hostile.binding.target_member_run_generation = Some(hostile_binding.1);
        let mut hostile_admission = admission.clone();
        hostile_admission.request_fingerprint =
            Some(runtime_command_envelope_fingerprint(&hostile).unwrap());
        let before = store.canonical_operations().unwrap();
        let error = store
            .prepare_runtime_command(&hostile_admission, &hostile, current_unix_ms(), "t-hostile")
            .expect_err("foreign or stale MemberRun binding must fail before admission");
        assert!(error.to_string().contains("MEMBER_RUN_GENERATION_FENCED"));
        assert_eq!(store.canonical_operations().unwrap(), before);
    }
    let accepted = store
        .prepare_runtime_command(&admission, &command, current_unix_ms(), "t-accepted")
        .unwrap();
    assert_eq!(
        accepted.projection.request_fingerprint,
        runtime_command_envelope_fingerprint(&command).unwrap(),
        "RuntimeCommandRecord must snapshot the full command, not only payload"
    );
    store
        .release_team_supervisor_lease(
            "run-supervisor",
            &lease.supervisor_id,
            lease.generation,
            current_unix_ms(),
        )
        .unwrap();
    let before_settle = store.canonical_operations().unwrap();
    let error = store
        .settle_runtime_command(
            &service_context(
                "node_daemon.runtime.settle",
                "supervisor-live-command:settle",
                accepted.projection.version,
            ),
            &command.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            Some(serde_json::json!({"provider_receipt": "must-not-land"})),
            None,
            "t-settle",
        )
        .expect_err("a released Supervisor cannot settle Applied");
    assert!(error.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_settle);

    assert!(store
        .runtime_command_is_publicly_recoverable(&accepted.projection, current_unix_ms())
        .unwrap());
    let mut confirm_applied = service_context(
        "operator.runtime.resolve",
        "supervisor-live-command:confirm-applied",
        accepted.projection.version,
    );
    confirm_applied.authority_actor = Some(ActorRef {
        kind: ActorKind::Service,
        id: target.node_id.clone(),
    });
    let error = store
        .resolve_runtime_command_recovery(
            &confirm_applied,
            &command.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            RuntimeRecoveryResolution::ConfirmApplied,
            "evidence:no-provider-receipt",
            "t-confirm-applied-rejected",
        )
        .expect_err("an abandoned Prepared command cannot fabricate Applied");
    assert!(error.to_string().contains("cannot be confirmed Applied"));

    let mut confirm_not_applied = service_context(
        "operator.runtime.resolve",
        "supervisor-live-command:confirm-not-applied",
        accepted.projection.version,
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
            "evidence:released-supervisor-and-provider-process-absent",
            "t-confirmed-not-applied",
        )
        .expect("exact Operator resolves the abandoned prepared command");
    assert_eq!(resolved.projection.status, RuntimeCommandStatus::Failed);
    assert_eq!(
        resolved.projection.effect_certainty,
        RuntimeEffectCertainty::NotApplied
    );
    assert_eq!(
        resolved.projection.result.as_ref().unwrap()["blind_replay"],
        false
    );

    let successor = store
        .acquire_team_supervisor_under_node_lease(
            "run-supervisor",
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "space-test",
            "project-1",
            "supervisor-2",
            std::process::id(),
            "loopback://supervisor-2",
            current_unix_ms(),
            60_000,
        )
        .unwrap();

    let mut stale = session("session-stale-supervisor", "supervised-agent");
    stale.id = "session-stale-supervisor".into();
    stale.agent_member_id = "another-supervised-agent".into();
    stale.control_state.driver_ref = RuntimeDriverRef::TeamSupervisor {
        team_run_id: "run-supervisor".into(),
        team_supervisor_id: successor.supervisor_id,
        team_supervisor_generation: successor.generation.saturating_add(1),
    };
    store
        .migrate_legacy_agent_identity_same_id(
            &context("host", "identity.create", "another-supervised-agent", 0),
            identity("another-supervised-agent"),
        )
        .unwrap();
    join_runtime_membership(
        &store,
        "membership-another-supervised-agent",
        "team-supervisor",
        "another-supervised-agent",
        TeamMembershipRole::Member,
    );
    store
        .create_agent_session(
            &service_context("session.create", "session-stale-supervisor", 0),
            stale.clone(),
        )
        .unwrap();
    store
        .legacy_import_create_trust_member_run_projection(
            &context("host", "member_run.create", "member-run-stale", 0),
            MemberRun {
                id: "member-run-stale".into(),
                agent_member_id: "another-supervised-agent".into(),
                team_run_id: "run-supervisor".into(),
                role_snapshot: "member".into(),
                provider_profile_snapshot: None,
                requested_controls: serde_json::json!({}),
                effective_controls: serde_json::json!({}),
                coordination_status: MemberCoordinationStatus::Active,
                runtime_status: MemberRuntimeStatus::Idle,
                runtime_generation: 1,
                workspace_binding_id: None,
                native_session: None,
                version: 1,
                started_at: "t1".into(),
                last_event_at: None,
                finished_at: None,
            },
        )
        .unwrap();
    let (mut stale_command, mut stale_admission) = runtime_command_fixture(
        "supervisor-stale-command",
        RuntimeCommandKind::OpenRuntime,
        &stale,
        "open_runtime",
    );
    stale_command.binding.target_member_run_id = Some("member-run-stale".into());
    stale_command.binding.target_member_run_generation = Some(1);
    stale_admission.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&stale_command).unwrap());
    let before_prepare = store.canonical_operations().unwrap();
    let error = store
        .prepare_runtime_command(
            &stale_admission,
            &stale_command,
            current_unix_ms(),
            "t-stale",
        )
        .expect_err("a stale Supervisor generation must not reach Accepted");
    assert!(error.to_string().contains("SUPERVISOR_GENERATION_FENCED"));
    assert_eq!(store.canonical_operations().unwrap(), before_prepare);
    fs::remove_dir_all(root).unwrap();
}
