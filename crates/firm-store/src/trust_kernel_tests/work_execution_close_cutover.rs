use super::work_responsibility_execution_admission_is_exact_and_idempotent::{
    admit_member_run, assign_responsibility, canonical_member_run, execution_binding,
};
use super::*;

#[test]
fn member_close_releases_old_binding_but_preserves_provider_received_and_fences_replay() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context("operator", "identity.create", "identity-close-worker", 0),
            identity("close-worker"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-close-worker",
        "team-admission",
        "close-worker",
        TeamMembershipRole::Member,
    );
    let target = session("session-close-worker", "close-worker");
    store
        .create_agent_session(
            &service_context("session.create", "session-close-worker", 0),
            target.clone(),
        )
        .unwrap();
    admit_member_run(
        &store,
        canonical_member_run("member-run-close-worker", "close-worker", "run-admission"),
    );
    let work = assign_responsibility(&store, "work-close-worker", &membership.id);
    let mut runtime_binding = runtime_command_fixture(
        "runtime-close-worker",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-close-worker".into());
    runtime_binding.target_member_run_generation = Some(1);
    let binding = execution_binding(&work, &membership, &target, "binding-close-worker");
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-close-worker", 0),
            &runtime_binding,
            binding.clone(),
        )
        .unwrap();
    store
        .claim_work_for_provider(
            &service_context("work.claim", "claim-close-worker", 0),
            &binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-close-worker",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim-close-worker",
        )
        .unwrap();
    store
        .record_work_provider_receipt(
            &service_context("work.receipt", "receipt-close-worker", 0),
            &binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-close-worker",
            "provider-receipt-close-worker",
            "t-receipt-close-worker",
        )
        .unwrap();
    let started = store
        .start_work(
            &work.id,
            work.version,
            "member-run-close-worker",
            firm_core::WorkCommandContext {
                event_id: "event-start-close-worker".into(),
                performed_by_actor: firm_core::TeamActorRef {
                    kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
                    id: "member-run-close-worker".into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "command-start-close-worker".into(),
                created_at: "t-start-close-worker".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();
    let close = firm_core::TeamMemberCloseRequest {
        id: "close-worker-request".into(),
        team_run_id: "run-admission".into(),
        member_run_id: "member-run-close-worker".into(),
        requested_by: "host".into(),
        reason: "test exact generation cutover".into(),
        status: firm_core::TeamMemberCloseStatus::Pending,
        requested_at: "t-close-request".into(),
        applied_at: None,
        detached_recovery_fence: None,
    };
    store.latch_team_member_close(&close).unwrap();
    let missing_effect = store
        .release_work_execution_binding_for_member_close(
            &service_context(
                "work.release.close",
                "release-close-worker-missing-effect",
                binding.version,
            ),
            &binding.id,
            &close.id,
            "missing-close-worker-command",
            &close.member_run_id,
            1,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "t-close-release",
        )
        .expect_err("pending Close intent alone must not release provider-received authority");
    assert!(missing_effect
        .to_string()
        .contains("DELIVERY_RECOVERY_UNCERTAIN"));
    assert_eq!(
        store
            .fabric_work_execution_bindings("space-test")
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == binding.id)
            .unwrap()
            .status,
        WorkExecutionBindingStatus::Active
    );
    let (mut close_command, mut close_admission) = runtime_command_fixture(
        "close-worker-command",
        RuntimeCommandKind::CloseMember,
        &target,
        "close_member",
    );
    close_command.binding.target_member_run_id = Some(close.member_run_id.clone());
    close_command.binding.target_member_run_generation = Some(1);
    close_command.payload["delivery_id"] =
        serde_json::Value::String(format!("{}:idle:close-runtime", close.id));
    close_command.payload_fingerprint = canonical_json_fingerprint(&close_command.payload);
    close_command.postcondition.desired_ack_level =
        firm_core::agentfirm_api::RuntimeAcknowledgementLevel::ProviderReceipt;
    close_command.postcondition.desired_postcondition =
        firm_core::agentfirm_api::RuntimeDesiredPostcondition::RuntimeReleased;
    close_admission.request_fingerprint =
        Some(runtime_command_envelope_fingerprint(&close_command).unwrap());
    let prepared = store
        .prepare_runtime_command(
            &close_admission,
            &close_command,
            current_unix_ms(),
            "t-close-accepted",
        )
        .unwrap();
    store
        .settle_runtime_command_with_postcondition(
            &service_context(
                "runtime.closemember.settle",
                "close-worker-command:settle",
                prepared.projection.version,
            ),
            &close_command.id,
            RuntimeCommandStatus::Applied,
            RuntimeEffectCertainty::Applied,
            RuntimePostconditionStatus::Satisfied,
            Some(serde_json::json!({"closed": true})),
            None,
            "t-close-applied",
        )
        .unwrap();
    let released = store
        .release_work_execution_binding_for_member_close(
            &service_context(
                "work.release.close",
                "release-close-worker",
                binding.version,
            ),
            &binding.id,
            &close.id,
            &close_command.id,
            &close.member_run_id,
            1,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "t-close-release",
        )
        .unwrap();
    assert_eq!(
        released.projection.status,
        WorkExecutionBindingStatus::Released
    );
    let delivery = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.id == binding.delivery_id)
        .unwrap();
    assert_eq!(delivery.status, WorkDeliveryStatus::ProviderReceived);
    assert_eq!(
        delivery.provider_receipt_id.as_deref(),
        Some("provider-receipt-close-worker")
    );

    let mut replay_binding = binding.clone();
    replay_binding.id = "binding-close-worker-replay".into();
    replay_binding.delivery_id = "work-delivery:work-close-worker:2".into();
    replay_binding.binding_generation = 2;
    replay_binding.work_revision = started.version;
    let error = store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-close-worker-replay", 0),
            &runtime_binding,
            replay_binding,
        )
        .expect_err("same ProviderReceived Work revision must never replay");
    assert!(error.to_string().contains("DELIVERY_RECOVERY_UNCERTAIN"));
}
