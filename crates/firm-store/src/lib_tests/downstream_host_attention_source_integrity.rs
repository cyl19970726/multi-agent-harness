use super::*;

fn fixture(label: &str) -> (PathBuf, HarnessStore, HostAttention) {
    let (root, store, run, _, _) = work_test_fixture(label);
    let prerequisite = store
        .insert_work(
            unassigned_test_work(&run.id, "prerequisite"),
            host_work_context("create-pre", "create-pre", "unix-ms:2"),
        )
        .unwrap();
    let dependent = store
        .insert_work(
            unassigned_test_work(&run.id, "dependent"),
            host_work_context("create-dependent", "create-dependent", "unix-ms:3"),
        )
        .unwrap();
    store
        .replace_work_dependencies(
            &dependent.id,
            dependent.version,
            vec![prerequisite.id.clone()],
            host_work_context("link", "link", "unix-ms:4"),
        )
        .unwrap();
    // Persist the supported ordinary WorkOperation source path. Current
    // cancel_work writes a canonical trust operation, which would exercise a
    // different source index and miss the reviewed regression.
    let mut cancelled = prerequisite.clone();
    cancelled.version += 1;
    cancelled.phase = WorkPhase::Closed;
    cancelled.resolution = Some(WorkResolution::Cancelled);
    cancelled.updated_at = "unix-ms:5".into();
    let payload = store
        .work_graph_outbox_payload_unlocked(
            &cancelled,
            WorkEventKind::Cancelled,
            serde_json::Value::Null,
        )
        .unwrap();
    let context = host_work_context("cancel-pre", "cancel-pre", "unix-ms:5");
    let operation = WorkOperation {
        event: WorkEvent {
            id: context.event_id,
            team_run_id: run.id.clone(),
            work_id: cancelled.id.clone(),
            sequence: 1,
            kind: WorkEventKind::Cancelled,
            expected_version: prerequisite.version,
            resulting_version: cancelled.version,
            performed_by_actor: context.performed_by_actor,
            authority_actor: None,
            causation_ref: None,
            idempotency_key: context.idempotency_key,
            payload,
            created_at: context.created_at,
        },
        work: cancelled,
        condition_records: vec![],
        reports: vec![],
        evidence_records: vec![],
        decisions: vec![],
        delegation_revisions: vec![],
    };
    {
        let _lock = store.acquire_write_lock().unwrap();
        store.append_work_operation_unlocked(&operation).unwrap();
    }
    let attention = store
        .host_attentions()
        .unwrap()
        .into_iter()
        .find(|row| row.kind == HostAttentionKind::WorkPrerequisiteNeedsReconciliation)
        .unwrap();
    assert_eq!(
        attention.kind,
        HostAttentionKind::WorkPrerequisiteNeedsReconciliation
    );
    (root, store, attention)
}

fn replace_rows<T: serde::Serialize>(root: &std::path::Path, name: &str, rows: &[T]) {
    // Fault injection in this isolated fixture only: external atomic replacement
    // exercises invalidation of an already-warm source/lifecycle snapshot.
    let mut bytes = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut bytes, row).unwrap();
        bytes.push(b'\n');
    }
    let temporary = root.join(format!("{name}.test-replacement"));
    std::fs::write(&temporary, bytes).unwrap();
    std::fs::rename(temporary, root.join(name)).unwrap();
}

#[test]
fn downstream_matching_existing_lifecycle_is_returned_on_warm_reconciliation() {
    let (root, store, mut attention) = fixture("downstream-matching-warm");
    attention.updated_at = "unix-ms:9".into();
    attention.validate().unwrap();
    replace_rows(&root, "host_attentions.jsonl", &[attention.clone()]);
    store.current_host_attention_projection().unwrap();
    for _ in 0..2 {
        let existing = store.reconcile_work_host_attentions().unwrap();
        assert_eq!(
            existing,
            vec![attention.clone()],
            "return the existing lifecycle, not a newly synthesized source row or an empty result"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn downstream_warm_lifecycle_replacement_with_conflicting_source_fact_is_rejected() {
    for change_kind in [false, true] {
        let (root, store, mut attention) = fixture(if change_kind {
            "downstream-kind-conflict"
        } else {
            "downstream-version-conflict"
        });
        store.current_work_sources().unwrap();
        store.current_host_attention_projection().unwrap();
        if change_kind {
            attention.kind = HostAttentionKind::WorkPrerequisiteCompleted;
        } else {
            attention.work_version += 1;
        }
        attention.validate().unwrap();
        replace_rows(&root, "host_attentions.jsonl", &[attention.clone()]);
        // This model has no ordinary Work sources, so the conflicting row is
        // structurally valid there; reconciliation must compare the two planes.
        assert_eq!(
            store
                .current_host_attention_projection()
                .unwrap()
                .get(&attention.id),
            Some(&attention)
        );
        let before = std::fs::read(root.join("host_attentions.jsonl")).unwrap();
        for _ in 0..2 {
            let error = store.reconcile_work_host_attentions().unwrap_err();
            assert!(
                matches!(error, StoreError::Conflict(ref text) if text == &format!("HostAttention id {} already names a different causal fact", attention.id)),
                "{error}"
            );
            assert_eq!(
                std::fs::read(root.join("host_attentions.jsonl")).unwrap(),
                before
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn downstream_warm_work_source_replacement_rechecks_existing_lifecycle_identity() {
    let (root, store, attention) = fixture("downstream-source-replacement");
    store.current_work_sources().unwrap();
    store.current_host_attention_projection().unwrap();
    let mut operations = store.work_operations().unwrap();
    let source = operations
        .iter_mut()
        .find(|op| op.event.payload.get("work_graph_outbox").is_some())
        .unwrap();
    source.event.payload["work_graph_outbox"][0]["dependent_work_version"] =
        serde_json::json!(attention.work_version + 1);
    replace_rows(&root, "work_operations.jsonl", &operations);
    let before = std::fs::read(root.join("host_attentions.jsonl")).unwrap();
    for _ in 0..2 {
        let error = store.reconcile_work_host_attentions().unwrap_err();
        assert!(
            matches!(error, StoreError::Conflict(ref text) if text == &format!("HostAttention id {} already names a different causal fact", attention.id)),
            "{error}"
        );
        assert_eq!(
            std::fs::read(root.join("host_attentions.jsonl")).unwrap(),
            before
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
