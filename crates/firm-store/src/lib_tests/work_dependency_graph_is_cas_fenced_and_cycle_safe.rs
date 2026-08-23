use super::*;

fn create_work(store: &HarnessStore, run_id: &str, id: &str, at: u64) -> Work {
    store
        .insert_work(
            unassigned_test_work(run_id, id),
            host_work_context(
                &format!("event-create-{id}"),
                &format!("key-create-{id}"),
                &format!("unix-ms:{at}"),
            ),
        )
        .expect("create Work")
}

#[test]
fn work_dependency_graph_is_cas_fenced_idempotent_and_cycle_safe() {
    let (_root, store, run, _, _) = work_test_fixture("dependency-graph");
    let a = create_work(&store, &run.id, "work-a", 2);
    let b = create_work(&store, &run.id, "work-b", 3);
    let c = create_work(&store, &run.id, "work-c", 4);

    let b = store
        .replace_work_dependencies(
            &b.id,
            b.version,
            vec![a.id.clone()],
            host_work_context("event-b-a", "key-b-a", "unix-ms:5"),
        )
        .expect("B depends on A");
    let replay = store
        .replace_work_dependencies(
            &b.id,
            1,
            vec![a.id.clone()],
            host_work_context("different-envelope-id", "key-b-a", "unix-ms:99"),
        )
        .expect("envelope id and timestamp do not change semantic retry identity");
    assert_eq!(replay, b);
    let drift = store
        .replace_work_dependencies(
            &b.id,
            1,
            Vec::new(),
            host_work_context("drift-envelope-id", "key-b-a", "unix-ms:100"),
        )
        .expect_err("same key cannot name a different dependency set");
    assert!(drift
        .to_string()
        .contains("idempotency key was already used"));

    let c = store
        .replace_work_dependencies(
            &c.id,
            c.version,
            vec![b.id.clone()],
            host_work_context("event-c-b", "key-c-b", "unix-ms:6"),
        )
        .expect("C depends on B");
    let cycle = store
        .replace_work_dependencies(
            &a.id,
            a.version,
            vec![c.id.clone()],
            host_work_context("event-a-c", "key-a-c", "unix-ms:7"),
        )
        .expect_err("transitive cycle rejected");
    assert!(cycle.to_string().contains("cycle"));

    let graph = store.work_graph(&run.agent_team_id).expect("derive graph");
    assert_eq!(graph.nodes.len(), 3);
    let a_node = graph
        .nodes
        .iter()
        .find(|node| node.work.id == a.id)
        .unwrap();
    assert_eq!(a_node.successor_work_ids, vec![b.id.clone()]);
    assert!(a_node.readiness.ready);
    let c_node = graph
        .nodes
        .iter()
        .find(|node| node.work.id == c.id)
        .unwrap();
    assert!(!c_node.readiness.ready);
}

#[test]
fn dependency_writer_requires_exact_team_run_host_before_replay_or_append() {
    let (_root, store, run, _, _) = work_test_fixture("dependency-exact-host");
    let prerequisite = create_work(&store, &run.id, "work-prerequisite", 2);
    let dependent = create_work(&store, &run.id, "work-dependent", 3);

    let committed = store
        .replace_work_dependencies(
            &dependent.id,
            dependent.version,
            vec![prerequisite.id.clone()],
            run_host_work_context(&run, "event-exact-host", "key-exact-host", "unix-ms:4"),
        )
        .expect("exact TeamRun Host may replace dependencies");
    let canonical_before = store
        .canonical_operations()
        .expect("canonical operations before hostile attempts")
        .len();

    let hostile_actors = [
        (TeamActorKind::Host, "forged-host"),
        (TeamActorKind::Host, "host"),
        (TeamActorKind::Operator, "operator"),
        (TeamActorKind::Service, "service"),
    ];
    for (index, (kind, id)) in hostile_actors.into_iter().enumerate() {
        let idempotency_key = if index == 0 {
            // Prove exact replay cannot bypass Host authorization.
            "key-exact-host".to_string()
        } else {
            format!("key-hostile-{index}")
        };
        let mut context = host_work_context(
            &format!("event-hostile-{index}"),
            &idempotency_key,
            &format!("unix-ms:{}", 5 + index),
        );
        context.performed_by_actor.kind = kind;
        context.performed_by_actor.id = id.into();
        let error = store
            .replace_work_dependencies(&dependent.id, committed.version, Vec::new(), context)
            .expect_err("non-exact Host authority must fail closed");
        assert!(
            error.to_string().contains("Host authority")
                || error
                    .to_string()
                    .contains("TEAM_RUN_HOST_AUTHORITY_MISMATCH")
        );
        assert_eq!(
            store
                .canonical_operations()
                .expect("canonical operations after hostile attempt")
                .len(),
            canonical_before,
            "hostile actor {kind:?}:{id} must append no canonical operation"
        );
        assert_eq!(
            store
                .latest_works()
                .expect("latest Works after hostile attempt")
                .into_iter()
                .find(|work| work.id == dependent.id)
                .expect("dependent Work remains present"),
            committed,
            "hostile actor {kind:?}:{id} must not change the Work projection"
        );
    }
}

#[test]
fn concurrent_opposite_edges_linearize_without_creating_a_cycle() {
    let (_root, store, run, _, _) = work_test_fixture("dependency-concurrent-cycle");
    let a = create_work(&store, &run.id, "work-a", 2);
    let b = create_work(&store, &run.id, "work-b", 3);
    let store = Arc::new(store);
    let barrier = Arc::new(Barrier::new(3));

    let left = {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let a = a.clone();
        let b = b.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.replace_work_dependencies(
                &a.id,
                a.version,
                vec![b.id],
                host_work_context("event-a-b", "key-a-b", "unix-ms:4"),
            )
        })
    };
    let right = {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let a = a.clone();
        let b = b.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.replace_work_dependencies(
                &b.id,
                b.version,
                vec![a.id],
                host_work_context("event-b-a", "key-b-a", "unix-ms:5"),
            )
        })
    };
    barrier.wait();
    let outcomes = [left.join().unwrap(), right.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
}

#[test]
fn failed_prerequisite_commits_replayable_cross_team_run_reconciliation_outbox() {
    let (_root, store, run, _, _) = work_test_fixture("dependency-reconciliation");
    let prerequisite = create_work(&store, &run.id, "work-prerequisite", 2);
    let mut successor_run = run.clone();
    successor_run.id = "tr-dependency-reconciliation-successor".into();
    successor_run.previous_run_id = Some(run.id.clone());
    let mut successor_host = store
        .member_runs()
        .expect("fixture runtime projections")
        .into_iter()
        .find(|member| member.agent_member_id == "agent-host" && member.team_run_id == run.id)
        .expect("source exact Host MemberRun");
    successor_host.id = "mr-dependency-reconciliation-successor-host".into();
    successor_host.team_run_id = successor_run.id.clone();
    successor_run.member_run_ids = vec![successor_host.id.clone()];
    successor_run.created_at = "unix-ms:3".into();
    successor_run.updated_at = "unix-ms:3".into();
    let execution_space_id = "unit-test-space";
    store
        .create_team_run_with_member_runs_from_agent_team(
            &successor_run,
            execution_space_id,
            std::slice::from_ref(&successor_host),
            &[canonical_member_admission_for_test(
                execution_space_id,
                &successor_host,
            )],
        )
        .expect("create successor TeamRun with exact Host MemberRun");
    let dependent = create_work(&store, &successor_run.id, "work-dependent", 4);
    let dependent = store
        .replace_work_dependencies(
            &dependent.id,
            dependent.version,
            vec![prerequisite.id.clone()],
            host_work_context("event-dependent-edge", "key-dependent-edge", "unix-ms:5"),
        )
        .expect("cross-TeamRun dependency in one accountable Team");

    let failed = store
        .fail_work(
            &prerequisite.id,
            prerequisite.version,
            "Host determined the prerequisite cannot complete",
            "failure-analysis-prerequisite",
            host_work_context(
                "event-prerequisite-failed",
                "key-prerequisite-failed",
                "unix-ms:6",
            ),
        )
        .expect("fail prerequisite");
    assert_eq!(failed.resolution, Some(WorkResolution::Failed));

    let operation = store
        .canonical_operations()
        .expect("read canonical operations")
        .into_iter()
        .find(|operation| {
            operation.event.aggregate_id == prerequisite.id
                && operation.event.transition == "failed"
        })
        .expect("failed operation");
    let outbox = &operation.initial_outbox_records;
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0]["work_id"], dependent.id);
    assert_eq!(outbox[0]["team_run_id"], successor_run.id);
    assert_eq!(outbox[0]["kind"], "work_prerequisite_needs_reconciliation");

    let first = store
        .host_attention_inbox_for_team_run(&successor_run.id, true)
        .expect("first replay-safe read");
    let second = store
        .host_attention_inbox_for_team_run(&successor_run.id, true)
        .expect("second replay-safe read");
    let matches = |rows: &[HostAttention]| {
        rows.iter()
            .filter(|attention| {
                attention.work_id == dependent.id
                    && attention.kind == HostAttentionKind::WorkPrerequisiteNeedsReconciliation
            })
            .count()
    };
    assert_eq!(matches(&first.attentions), 1);
    assert_eq!(matches(&second.attentions), 1);
}

#[test]
fn cancelled_prerequisite_uses_only_canonical_work_authority_and_replays_outbox() {
    let (_root, store, run, _, _) = work_test_fixture("dependency-cancel-canonical");
    let prerequisite = create_work(&store, &run.id, "work-prerequisite", 2);
    let dependent = create_work(&store, &run.id, "work-dependent", 3);
    let dependent = store
        .replace_work_dependencies(
            &dependent.id,
            dependent.version,
            vec![prerequisite.id.clone()],
            host_work_context("event-dependent-edge", "key-dependent-edge", "unix-ms:4"),
        )
        .expect("dependent edge");
    let legacy_before = store.work_operations().expect("legacy operations").len();

    let cancelled = store
        .cancel_work(
            &prerequisite.id,
            prerequisite.version,
            "obsolete prerequisite",
            host_work_context("native-cancel-event", "canonical-cancel-key", "unix-ms:5"),
        )
        .expect("canonical cancellation");
    let replay = store
        .cancel_work(
            &prerequisite.id,
            prerequisite.version,
            "obsolete prerequisite",
            host_work_context(
                "different-envelope-event",
                "canonical-cancel-key",
                "unix-ms:99",
            ),
        )
        .expect("exact canonical replay");
    assert_eq!(cancelled, replay);
    assert_eq!(
        store.work_operations().expect("legacy operations").len(),
        legacy_before,
        "current cancellation must not append the legacy WorkOperation ledger"
    );

    let operations = store.canonical_operations().expect("canonical operations");
    let cancellations = operations
        .iter()
        .filter(|operation| {
            operation.event.aggregate_id == prerequisite.id
                && operation.event.transition == "cancelled"
        })
        .collect::<Vec<_>>();
    assert_eq!(cancellations.len(), 1);
    assert!(cancellations[0]
        .initial_outbox_records
        .iter()
        .any(|record| {
            record["work_id"] == dependent.id
                && record["kind"] == "work_prerequisite_needs_reconciliation"
        }));
    assert!(store
        .work_events()
        .expect("compatibility events")
        .iter()
        .any(|event| {
            event.id == "native-cancel-event"
                && event.work_id == prerequisite.id
                && event.kind == WorkEventKind::Cancelled
        }));

    let first = store
        .host_attention_inbox_for_team_run(&run.id, true)
        .expect("first replay-safe outbox read");
    let second = store
        .host_attention_inbox_for_team_run(&run.id, true)
        .expect("second replay-safe outbox read");
    let count = |rows: &[HostAttention]| {
        rows.iter()
            .filter(|attention| {
                attention.work_id == dependent.id
                    && attention.kind == HostAttentionKind::WorkPrerequisiteNeedsReconciliation
            })
            .count()
    };
    assert_eq!(count(&first.attentions), 1);
    assert_eq!(count(&second.attentions), 1);
}

#[test]
fn canonical_outbox_attention_preserves_claim_and_receipt_projection() {
    let (root, store, mut run, _, _) = work_test_fixture("canonical-attention-receipt");
    run.host_control_mode = firm_core::HostControlMode::ExternalInteractive;
    run.host_surface = "codex-app".into();
    run.host_thread_id = Some("host-thread".into());
    run.updated_at = "unix-ms:2".into();
    store
        .append_jsonl("team_runs.jsonl", &run)
        .expect("bind exact external Host fixture");

    let prerequisite = create_work(&store, &run.id, "work-prerequisite", 3);
    let dependent = create_work(&store, &run.id, "work-dependent", 4);
    let dependent = store
        .replace_work_dependencies(
            &dependent.id,
            dependent.version,
            vec![prerequisite.id.clone()],
            host_work_context("event-dependent-edge", "key-dependent-edge", "unix-ms:5"),
        )
        .expect("dependent edge");
    store
        .cancel_work(
            &prerequisite.id,
            prerequisite.version,
            "exercise canonical reconciliation outbox",
            host_work_context("event-cancel", "key-cancel", "unix-ms:6"),
        )
        .expect("canonical cancellation");

    let attention = store
        .host_attention_inbox_for_team_run(&run.id, true)
        .expect("canonical outbox inbox")
        .attentions
        .into_iter()
        .find(|attention| attention.work_id == dependent.id)
        .expect("dependent reconciliation attention");
    assert_eq!(attention.status, HostAttentionStatus::Actionable);

    let claimed = store
        .claim_host_attention(
            &attention.id,
            "codex-app",
            "host-thread",
            "claim-canonical-attention",
            "unix-ms:7",
        )
        .expect("claim canonical outbox attention");
    assert!(matches!(claimed, HostAttentionClaimResult::Claimed(_)));

    let delivered = store
        .complete_host_attention_claim(
            &attention.id,
            "claim-canonical-attention",
            "provider-receipt-1",
            "unix-ms:8",
        )
        .expect("settle the same projected claim");
    assert_eq!(delivered.status, HostAttentionStatus::Delivered);
    assert_eq!(
        delivered.provider_receipt_id.as_deref(),
        Some("provider-receipt-1")
    );

    let replay = store
        .complete_host_attention_claim(
            &attention.id,
            "claim-canonical-attention",
            "provider-receipt-1",
            "unix-ms:9",
        )
        .expect("receipt settlement is idempotent");
    assert_eq!(replay, delivered);
    let reopened = HarnessStore::new(root);
    assert_eq!(
        reopened
            .host_attention_inbox_for_team_run(&run.id, true)
            .expect("restart-safe lifecycle projection")
            .attentions
            .into_iter()
            .find(|row| row.id == attention.id)
            .expect("projected attention")
            .status,
        HostAttentionStatus::Delivered
    );

    let mut corrupted = delivered;
    corrupted.work_id = "forged-work".into();
    corrupted.updated_at = "unix-ms:10".into();
    reopened
        .append_jsonl("host_attentions.jsonl", &corrupted)
        .expect("append hostile lifecycle row fixture");
    let conflict = reopened
        .host_attention_inbox_for_team_run(&run.id, true)
        .expect_err("mutable lifecycle row cannot change canonical source identity");
    assert!(conflict
        .to_string()
        .contains("HOST_ATTENTION_SOURCE_FACT_CONFLICT"));
}
