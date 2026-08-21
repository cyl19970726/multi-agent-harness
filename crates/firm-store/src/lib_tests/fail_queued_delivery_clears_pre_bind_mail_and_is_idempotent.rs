use super::*;

/// When a member fails before binding (pre-bind), queued TeamMessageProjection deliveries
/// transition to Failed so they do not stay permanently actionable in the inbox.
#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn fail_queued_delivery_clears_pre_bind_mail_and_is_idempotent() {
    let root = team_test_root("pre-bind-mail-fail");
    let store = HarnessStore::new(&root);
    let run = AgentTeamRun {
        id: "tr-fail-mail".into(),
        agent_team_id: "team-fail-mail".into(),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        project_binding_id: "project-test".into(),
        previous_run_id: None,
        host_surface: "codex-app".into(),
        host_thread_id: None,
        host_actor: None,
        host_control_mode: Default::default(),
        objective: "fail orphaned mail".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: vec!["mr-orphan".into()],
        budget_limit_usd: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    store
        .legacy_import_append_team_run_projection(&run)
        .expect("append run");

    let lease = store
        .acquire_test_supervisor_lease(
            &run.id,
            "supervisor-pre-bind",
            300,
            "test:pre-bind",
            100,
            5_000,
        )
        .expect("acquire Supervisor lease");

    let message = TeamMessageProjection {
        id: "tm-orphan".into(),
        team_run_id: run.id.clone(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "host".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec!["mr-orphan".into()],
        kind: ProviderDispatchIntent::Message,
        body: "orphaned work assignment".into(),
        correlation_id: "corr-orphan".into(),
        causation_id: None,
        response_intent: None,
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: "mr-orphan".into(),
            policy: TeamDeliveryPolicy::Queue,
            status: TeamDeliveryStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: "unix-ms:2".into(),
        }],
        created_at: "unix-ms:2".into(),
    };
    store
        .append_team_message_checked(&message)
        .expect("append queued message");

    // Pre-bind failure: member never bound, delivery is still Queued.
    let msgs = store.legacy_team_messages().expect("read legacy messages");
    let queued = msgs
        .iter()
        .find(|m| m.id == "tm-orphan")
        .expect("tm-orphan present");
    assert_eq!(
        queued.deliveries[0].status,
        TeamDeliveryStatus::Queued,
        "starts queued"
    );

    // Fail the delivery.
    let failed = store
        .fail_team_message_delivery(
            &run.id,
            &message.id,
            "mr-orphan",
            &lease.supervisor_id,
            lease.generation,
            "pre-bind member terminated",
            200,
            "unix-ms:3",
        )
        .expect("fail queued delivery");

    assert_eq!(failed.deliveries[0].status, TeamDeliveryStatus::Failed);
    assert_eq!(
        failed.deliveries[0].failure_reason.as_deref(),
        Some("pre-bind member terminated")
    );
    assert!(failed.deliveries[0].claim_id.is_none());
    assert!(failed.deliveries[0].provider_receipt_id.is_none());

    // Idempotent: same reason succeeds.
    let again = store
        .fail_team_message_delivery(
            &run.id,
            &message.id,
            "mr-orphan",
            &lease.supervisor_id,
            lease.generation,
            "pre-bind member terminated",
            201,
            "unix-ms:4",
        )
        .expect("idempotent fail with same reason");

    assert_eq!(again.deliveries[0].status, TeamDeliveryStatus::Failed);

    // Different reason is rejected.
    let conflict = store
        .fail_team_message_delivery(
            &run.id,
            &message.id,
            "mr-orphan",
            &lease.supervisor_id,
            lease.generation,
            "different reason",
            202,
            "unix-ms:5",
        )
        .expect_err("different failure reason must be rejected");
    assert!(conflict.to_string().contains("different reason"));

    // RegistryMessage survives store reopen.
    drop(store);
    let reopened = HarnessStore::new(&root);
    let msgs_after = reopened
        .legacy_team_messages()
        .expect("read legacy messages after reopen");
    let reloaded = latest_by_id(msgs_after, |m| m.id.clone())
        .remove("tm-orphan")
        .expect("tm-orphan survived reopen");
    assert_eq!(reloaded.deliveries[0].status, TeamDeliveryStatus::Failed);
    assert_eq!(
        reloaded.deliveries[0].failure_reason.as_deref(),
        Some("pre-bind member terminated")
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
