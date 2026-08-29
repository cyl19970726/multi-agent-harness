use super::work_responsibility_execution_admission_is_exact_and_idempotent::{
    admit_member_run, assign_responsibility, canonical_member_run, execution_binding,
};
use super::*;

#[test]
fn closed_process_admission_settles_claimed_delivery_but_cannot_claim_another() {
    let (store, _root) = fabric_store();
    const DAEMON_ID: &str = "daemon-process-admission-work-test";
    const INSTANCE_ID: &str = "instance-process-admission-work-test";
    let now = current_unix_ms();
    store
        .release_node_daemon_lease(
            "11111111-1111-4111-8111-111111111111",
            "daemon-1",
            1,
            "instance-1",
            now,
        )
        .unwrap();
    let daemon_lease = store
        .acquire_node_daemon_lease(
            "11111111-1111-4111-8111-111111111111",
            DAEMON_ID,
            INSTANCE_ID,
            now,
            60_000,
        )
        .unwrap();
    let daemon_context = |command: &str, key: &str, expected_version: u64| MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: DAEMON_ID.into(),
        },
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version,
        request_fingerprint: None,
    };
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context("operator", "identity.create", "identity-drain-worker", 0),
            identity("drain-worker"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-drain-worker",
        "team-admission",
        "drain-worker",
        TeamMembershipRole::Member,
    );
    let mut target = session("session-drain-worker", "drain-worker");
    target.node_daemon_id = DAEMON_ID.into();
    target.node_daemon_generation = daemon_lease.generation;
    target.control_state.driver_ref = firm_core::agentfirm_api::RuntimeDriverRef::NodeDaemon {
        node_daemon_id: DAEMON_ID.into(),
        node_daemon_generation: daemon_lease.generation,
    };
    store
        .create_agent_session(
            &daemon_context("session.create", "session-drain-worker", 0),
            target.clone(),
        )
        .unwrap();
    admit_member_run(
        &store,
        canonical_member_run("member-run-drain-worker", "drain-worker", "run-admission"),
    );
    let mut runtime_binding = runtime_command_fixture(
        "runtime-drain-worker",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-drain-worker".into());
    runtime_binding.target_member_run_generation = Some(1);

    let claimed_work = assign_responsibility(&store, "work-drain-claimed", &membership.id);
    let claimed_binding =
        execution_binding(&claimed_work, &membership, &target, "binding-drain-claimed");
    store
        .bind_responsible_work_execution(
            &daemon_context("work.bind", "binding-drain-claimed", 0),
            &runtime_binding,
            claimed_binding.clone(),
        )
        .unwrap();
    let queued_work = assign_responsibility(&store, "work-drain-queued", &membership.id);
    let queued_binding =
        execution_binding(&queued_work, &membership, &target, "binding-drain-queued");
    store
        .bind_responsible_work_execution(
            &daemon_context("work.bind", "binding-drain-queued", 0),
            &runtime_binding,
            queued_binding.clone(),
        )
        .unwrap();
    store
        .claim_work_for_provider(
            &daemon_context("work.claim", "claim-drain-worker", 0),
            &claimed_binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-drain-worker",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim-drain-worker",
        )
        .unwrap();

    crate::close_process_node_daemon_admission(DAEMON_ID, INSTANCE_ID);
    assert_eq!(
        store
            .latest_node_daemon_lease(&target.node_id)
            .unwrap()
            .unwrap()
            .status,
        firm_core::NodeDaemonLeaseStatus::Active,
        "the process gate must close before durable draining"
    );
    let receipt = store
        .record_work_provider_receipt(
            &daemon_context("work.receipt", "receipt-drain-worker", 0),
            &claimed_binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-drain-worker",
            "provider-receipt-drain-worker",
            "t-receipt-drain-worker",
        )
        .expect("draining predecessor must settle the exact claimed delivery");
    assert_eq!(
        receipt.projection.status,
        firm_core::agentfirm_api::WorkDeliveryStatus::ProviderReceived
    );

    let operations_before_rejection = store.canonical_operations().unwrap();
    let claim_error = store
        .claim_work_for_provider(
            &daemon_context("work.claim", "claim-after-drain", 0),
            &queued_binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-after-drain",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim-after-drain",
        )
        .expect_err("draining predecessor cannot claim another delivery");
    assert!(
        claim_error
            .to_string()
            .contains("SUPERVISOR_GENERATION_FENCED"),
        "{claim_error}"
    );
    assert_eq!(
        store.canonical_operations().unwrap(),
        operations_before_rejection,
        "rejected provider claim must have zero durable effect"
    );
}

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
