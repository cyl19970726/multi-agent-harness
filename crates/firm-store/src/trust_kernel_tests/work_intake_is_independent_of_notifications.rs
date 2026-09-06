use super::*;

fn command(store: &HarnessStore, run: &str, key: &str) -> WorkCommandContext {
    WorkCommandContext {
        event_id: key.into(),
        performed_by_actor: store.exact_team_run_host_actor(run).unwrap(),
        authority_actor: None,
        causation_ref: None,
        idempotency_key: key.into(),
        created_at: "t-retarget".into(),
        duplicate_ok: false,
    }
}

fn attention(work: &Work, kind: HostAttentionKind) -> HostAttention {
    serde_json::from_value(serde_json::json!({
        "id": "attention-intake", "team_run_id": work.team_run_id,
        "kind": kind, "work_id": work.id, "work_version": work.version,
        "source_event_ref": "source-intake", "status": "actionable",
        "attempt": 0, "created_at": "t-attention", "updated_at": "t-attention"
    }))
    .unwrap()
}

#[test]
fn retarget_is_a_versioned_host_decision_independent_of_notification_intake() {
    let (store, _root) = fabric_store();
    append_runtime_team(&store, "team-intake", "run-source");
    append_runtime_team(&store, "team-intake", "run-successor");
    append_runtime_team(&store, "team-foreign", "run-foreign");
    let work = insert_runtime_work(&store, "work-intake", "team-intake", "run-source");
    store
        .ensure_host_attention(&attention(&work, HostAttentionKind::WorkChanged))
        .unwrap();
    let before = store.work_operations().unwrap();
    let mut foreign = command(&store, "run-source", "wrong-host");
    foreign.performed_by_actor.id = "wrong-host".into();
    assert!(store
        .retarget_work_execution(&work.id, work.version, "run-successor", foreign)
        .is_err());
    assert!(store
        .retarget_work_execution(
            &work.id,
            work.version + 1,
            "run-successor",
            command(&store, "run-source", "stale")
        )
        .is_err());
    assert!(store
        .retarget_work_execution(
            &work.id,
            work.version,
            "run-foreign",
            command(&store, "run-source", "foreign-team")
        )
        .is_err());
    assert_eq!(store.work_operations().unwrap(), before);
    let context = command(&store, "run-source", "retarget");
    let next = store
        .retarget_work_execution(&work.id, work.version, "run-successor", context.clone())
        .unwrap();
    assert_eq!(next.team_run_id, "run-successor");
    assert_eq!(next.version, work.version + 1);
    assert_eq!(
        store
            .retarget_work_execution(&work.id, work.version, "run-successor", context.clone())
            .unwrap(),
        next
    );
    let mut wrong_replay = context.clone();
    wrong_replay.performed_by_actor.id = "wrong-host".into();
    assert!(store
        .retarget_work_execution(&work.id, work.version, "run-successor", wrong_replay)
        .is_err());
    assert!(store
        .retarget_work_execution(&work.id, work.version, "run-foreign", context)
        .is_err());
    assert_eq!(
        store.latest_host_attentions_unlocked().unwrap()["attention-intake"].status,
        HostAttentionStatus::Actionable
    );
}

#[test]
fn historical_attention_kinds_are_readable_but_have_no_current_creation_path() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-intake", "run-source");
    let work = insert_runtime_work(&store, "work-intake", "team-intake", "run-source");
    for kind in [
        HostAttentionKind::WorkDeliveryFailed,
        HostAttentionKind::MemberStoppedWithOwnedReadyWork,
        HostAttentionKind::MemberFailedWithOwnedReadyWork,
    ] {
        let row = attention(&work, kind);
        assert!(store
            .ensure_host_attention(&row)
            .unwrap_err()
            .to_string()
            .contains("LEGACY_HOST_ATTENTION_KIND"));
        let mut historic = row;
        historic.status = HostAttentionStatus::EscalationRequired;
        std::fs::write(
            root.join("host_attentions.jsonl"),
            format!("{}\n", serde_json::to_string(&historic).unwrap()),
        )
        .unwrap();
        assert_eq!(
            store.latest_host_attentions_unlocked().unwrap()[&historic.id],
            historic
        );
    }
}

#[test]
fn historical_submission_provenance_uses_work_operations_and_rejects_ambiguity() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-intake", "run-source");
    let work = insert_runtime_work(&store, "work-intake", "team-intake", "run-source");
    let mut submission = store
        .work_operations()
        .unwrap()
        .into_iter()
        .find(|op| op.work.id == work.id)
        .unwrap();
    submission.event.kind = firm_core::WorkEventKind::Submitted;
    submission.event.performed_by_actor.kind = TeamActorKind::ProviderRuntimeProjection;
    submission.event.performed_by_actor.id = "exact-historical-member-run".into();
    submission.work.phase = firm_core::WorkPhase::Review;
    std::fs::write(
        root.join("work_operations.jsonl"),
        format!("{}\n", serde_json::to_string(&submission).unwrap()),
    )
    .unwrap();
    // An unreadable notification file must not participate in Work provenance.
    std::fs::write(root.join("host_attentions.jsonl"), "not-json\n").unwrap();
    let mut terminal = submission.work.clone();
    terminal.phase = firm_core::WorkPhase::Closed;
    terminal.version += 1;
    assert_eq!(
        store
            .terminal_work_member_run_provenance_unlocked(&terminal)
            .unwrap(),
        "exact-historical-member-run"
    );
    let mut refreshed = submission.clone();
    refreshed.event.id = "refresh".into();
    refreshed.event.kind = firm_core::WorkEventKind::Updated;
    refreshed.event.expected_version = submission.work.version;
    refreshed.event.resulting_version = submission.work.version + 1;
    refreshed.event.payload = serde_json::json!({"reason": "github_evidence_refresh"});
    refreshed.work.version += 1;
    let original = serde_json::to_string(&submission).unwrap();
    std::fs::write(
        root.join("work_operations.jsonl"),
        format!(
            "{original}\n{}\n",
            serde_json::to_string(&refreshed).unwrap()
        ),
    )
    .unwrap();
    terminal.version += 1;
    assert_eq!(
        store
            .terminal_work_member_run_provenance_unlocked(&terminal)
            .unwrap(),
        "exact-historical-member-run"
    );
    refreshed.event.payload = serde_json::json!({"reason": "arbitrary_update"});
    std::fs::write(
        root.join("work_operations.jsonl"),
        format!(
            "{original}\n{}\n",
            serde_json::to_string(&refreshed).unwrap()
        ),
    )
    .unwrap();
    assert!(store
        .terminal_work_member_run_provenance_unlocked(&terminal)
        .is_err());
    terminal.version -= 1;
    let serialized = serde_json::to_string(&submission).unwrap();
    std::fs::write(
        root.join("work_operations.jsonl"),
        format!("{serialized}\n{serialized}\n"),
    )
    .unwrap();
    assert!(store
        .terminal_work_member_run_provenance_unlocked(&terminal)
        .unwrap_err()
        .to_string()
        .contains("ambiguous"));
}

#[test]
fn retarget_allows_blocked_recovery_but_rejects_terminal_work() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-intake", "run-source");
    append_runtime_team(&store, "team-intake", "run-successor");
    let work = insert_runtime_work(&store, "work-intake", "team-intake", "run-source");
    let mut op = store
        .work_operations()
        .unwrap()
        .into_iter()
        .find(|op| op.work.id == work.id)
        .unwrap();
    op.work.condition = firm_core::WorkCondition::Blocked;
    let mut delivered = attention(&work, HostAttentionKind::WorkBlocked);
    delivered.status = HostAttentionStatus::Delivered;
    store
        .append_jsonl_unlocked("host_attentions.jsonl", &delivered)
        .unwrap();

    std::fs::write(
        root.join("work_operations.jsonl"),
        format!("{}\n", serde_json::to_string(&op).unwrap()),
    )
    .unwrap();
    let next = store
        .retarget_work_execution(
            &work.id,
            work.version,
            "run-successor",
            command(&store, "run-source", "blocked-recovery"),
        )
        .unwrap();
    assert_eq!(next.condition, firm_core::WorkCondition::Blocked);
    let cancelled = store
        .cancel_work(
            &next.id,
            next.version,
            "superseded",
            command(&store, "run-successor", "cancel"),
        )
        .unwrap();
    let before = store.work_operations().unwrap();
    assert!(store
        .retarget_work_execution(
            &cancelled.id,
            cancelled.version,
            "run-source",
            command(&store, "run-successor", "terminal")
        )
        .unwrap_err()
        .to_string()
        .contains("terminal"));
    assert_eq!(store.work_operations().unwrap(), before);
}
