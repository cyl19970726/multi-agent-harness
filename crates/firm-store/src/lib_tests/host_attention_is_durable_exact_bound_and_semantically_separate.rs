use super::*;

#[test]
fn host_attention_is_durable_exact_bound_and_semantically_separate() {
    let root = team_test_root("host-attention");
    let store = HarnessStore::new(&root);
    let (run, member, work) = seed_host_attention_fixture(&store, "run-a", None);
    let attention = HostAttention {
        id: "host-attention-work-review-a".into(),
        team_run_id: run.id.clone(),
        kind: HostAttentionKind::WorkReviewRequested,
        work_id: work.id.clone(),
        work_version: work.version,
        source_event_ref: "work-event-review-a".into(),
        member_run_id: Some(member.id.clone()),
        status: HostAttentionStatus::Actionable,
        attempt: 0,
        claim_id: None,
        claimed_host_surface: None,
        claimed_host_thread_id: None,
        claimed_host_lease_id: None,
        claimed_host_lease_generation: None,
        claimed_host_lease_owner_id: None,
        claimed_recipient_member_run_id: None,
        claimed_recipient_session_id: None,
        claimed_recipient_session_generation: None,
        claimed_node_daemon_id: None,
        claimed_node_daemon_generation: None,
        provider_receipt_id: None,
        last_failure_reason: None,
        created_at: "unix-ms:3".into(),
        updated_at: "unix-ms:3".into(),
    };
    store
        .ensure_host_attention(&attention)
        .expect("append attention");
    assert!(
        store.legacy_team_messages().expect("messages").is_empty(),
        "Work state attention must not fabricate TeamMessageProjection conversation"
    );
    let unbound = store
        .host_attention_inbox_for_team_run(&run.id, false)
        .expect("unbound projection");
    assert_eq!(unbound.attentions.len(), 1);
    assert!(unbound.warning.as_deref().is_some_and(|warning| {
        warning.contains("EXTERNAL_HOST_PULL_ONLY") && warning.contains(&member.id)
    }));
    assert!(store
        .host_attention_inboxes_for_native_thread("codex-app", "other-task", false)
        .expect("other task")
        .is_empty());

    let mut bound = run.clone();
    bound.host_thread_id = Some("codex-task-a".into());
    bound.updated_at = "unix-ms:4".into();
    store
        .compare_and_append_team_run(&run, &bound)
        .expect("bind exact Host task");
    let exact = store
        .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false)
        .expect("exact Host inbox");
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].attentions[0].id, attention.id);
    assert!(store
        .host_attention_inboxes_for_native_thread("codex-app", "codex-task-b", false)
        .expect("other exact task")
        .is_empty());

    let claimed = store
        .claim_host_attention(
            &attention.id,
            "codex-app",
            "codex-task-a",
            "claim-a",
            "unix-ms:5",
        )
        .expect("claim attention");
    assert!(matches!(claimed, HostAttentionClaimResult::Claimed(_)));
    assert!(matches!(
        store
            .claim_host_attention(
                &attention.id,
                "codex-app",
                "codex-task-a",
                "claim-a",
                "unix-ms:5",
            )
            .expect("idempotent claim"),
        HostAttentionClaimResult::Claimed(_)
    ));
    assert!(store
        .claim_host_attention(
            &attention.id,
            "codex-app",
            "codex-task-a",
            "claim-b",
            "unix-ms:5",
        )
        .is_ok_and(|result| result == HostAttentionClaimResult::NotActionable));

    let delivered = store
        .complete_host_attention_claim(&attention.id, "claim-a", "codex-turn-start-1", "unix-ms:6")
        .expect("record provider receipt");
    assert_eq!(delivered.status, HostAttentionStatus::Delivered);
    assert!(delivered.needs_host_action());
    assert_eq!(
        store
            .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false,)
            .expect("delivered still actionable")[0]
            .attentions
            .len(),
        1
    );

    let acknowledged = store
        .acknowledge_host_attention(&attention.id, "codex-app", "codex-task-a", "unix-ms:7")
        .expect("Host intake ACK");
    assert_eq!(acknowledged.status, HostAttentionStatus::Acknowledged);
    assert!(store
        .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false)
        .expect("actionable inbox after ACK")
        .is_empty());
    assert_eq!(
        store.latest_works().expect("Work remains")[0].phase,
        WorkPhase::Open,
        "attention ACK must not accept or request changes on Work"
    );
    assert_eq!(
        store
            .ensure_host_attention(&attention)
            .expect("causal replay remains idempotent")
            .status,
        HostAttentionStatus::Acknowledged,
        "replaying projection must not reset Host intake"
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
