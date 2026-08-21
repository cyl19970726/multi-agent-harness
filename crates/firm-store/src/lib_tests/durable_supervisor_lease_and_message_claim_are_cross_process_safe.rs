use super::*;

#[test]
#[cfg(any())] // Wave 4C: historical Wave 4A writer contract; canonical trust-kernel coverage replaces it.
fn durable_supervisor_lease_and_message_claim_are_cross_process_safe() {
    let root = team_test_root("supervisor-claim");
    let store = Arc::new(HarnessStore::new(&root));
    let run = AgentTeamRun {
        id: "tr-claim".into(),
        agent_team_id: "team-claim".into(),
        execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
        project_binding_id: "project-test".into(),
        previous_run_id: None,
        host_surface: "codex-app".into(),
        host_thread_id: Some("thread-claim".into()),
        host_actor: None,
        host_control_mode: Default::default(),
        objective: "claim exactly once".into(),
        execution_root: None,
        status: TeamRunStatus::Running,
        member_run_ids: vec!["mr-claim".into()],
        budget_limit_usd: None,
        created_at: "unix-ms:1".into(),
        updated_at: "unix-ms:1".into(),
        completed_at: None,
    };
    store
        .legacy_import_append_team_run_projection(&run)
        .expect("append run");

    let first = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-a", 101, "test:a", 100, 1_000)
        .expect("first Supervisor");
    assert_eq!(first.generation, 1);
    let conflict = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-b", 202, "test:b", 101, 1_000)
        .expect_err("second active Supervisor must be rejected");
    assert!(conflict.to_string().contains("supervisor-a"));
    let second = store
        .acquire_test_supervisor_lease(&run.id, "supervisor-b", 202, "test:b", 1_101, 1_000)
        .expect("expired lease may be replaced");
    assert_eq!(second.generation, 2);

    let message = TeamMessageProjection {
        id: "tm-claim".into(),
        team_run_id: run.id.clone(),
        work_id: None,
        source_plan_ref: None,
        sender: None,
        sender_runtime_id: "host".into(),
        recipients: Vec::new(),
        recipient_runtime_ids: vec!["mr-claim".into()],
        kind: ProviderDispatchIntent::Message,
        body: "only once".into(),
        correlation_id: "corr-claim".into(),
        causation_id: None,
        response_intent: None,
        evidence_refs: Vec::new(),
        deliveries: vec![ProviderDispatchAttempt {
            member_id: "mr-claim".into(),
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
    let early_ack = store
        .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:2")
        .expect_err("queued delivery cannot be acknowledged");
    assert!(early_ack.to_string().contains("has not been delivered"));

    let barrier = Arc::new(Barrier::new(2));
    let handles = ["claim-a", "claim-b"].map(|claim_id| {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let run_id = run.id.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store
                .claim_team_message_delivery(
                    &run_id,
                    "tm-claim",
                    "mr-claim",
                    "supervisor-b",
                    2,
                    claim_id,
                    1_102,
                    1_000,
                    "unix-ms:3",
                )
                .expect("claim call")
        })
    });
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("claim thread"))
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, TeamMessageDeliveryClaimResult::Claimed(_)))
            .count(),
        1
    );
    let claimed = results
        .into_iter()
        .find_map(|result| match result {
            TeamMessageDeliveryClaimResult::Claimed(message) => Some(*message),
            TeamMessageDeliveryClaimResult::NotQueued => None,
        })
        .expect("one claim");
    let claim_id = claimed.deliveries[0].claim_id.clone().expect("claim id");
    let stale_completion = store
        .complete_team_message_delivery_claim(
            &run.id,
            &message.id,
            "mr-claim",
            "supervisor-a",
            1,
            &claim_id,
            "native-turn-stale",
            1_103,
            "unix-ms:4",
        )
        .expect_err("a stale Supervisor generation cannot complete another lease's claim");
    assert!(stale_completion
        .to_string()
        .contains("Supervisor lease is not owned"));
    let delivered = store
        .complete_team_message_delivery_claim(
            &run.id,
            &message.id,
            "mr-claim",
            "supervisor-b",
            2,
            &claim_id,
            "native-turn-1",
            1_103,
            "unix-ms:4",
        )
        .expect("complete claim");
    assert_eq!(
        delivered.deliveries[0].status,
        TeamDeliveryStatus::Delivered
    );
    assert_eq!(
        delivered.deliveries[0].provider_receipt_id.as_deref(),
        Some("native-turn-1")
    );
    store
        .complete_team_message_delivery_claim(
            &run.id,
            &message.id,
            "mr-claim",
            "supervisor-b",
            2,
            &claim_id,
            "native-turn-1",
            1_104,
            "unix-ms:4",
        )
        .expect("exact completion receipt is idempotent");
    let different_receipt = store
        .complete_team_message_delivery_claim(
            &run.id,
            &message.id,
            "mr-claim",
            "supervisor-b",
            2,
            &claim_id,
            "native-turn-different",
            1_104,
            "unix-ms:4",
        )
        .expect_err("completed claim cannot change provider receipt");
    assert!(different_receipt
        .to_string()
        .contains("different provider receipt"));
    let acknowledged = store
        .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:5")
        .expect("acknowledge delivered message");
    assert_eq!(
        acknowledged.deliveries[0].status,
        TeamDeliveryStatus::Acknowledged
    );
    let acknowledged_again = store
        .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:6")
        .expect("ACK is idempotent");
    assert_eq!(
        acknowledged_again.deliveries[0].status,
        TeamDeliveryStatus::Acknowledged
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
