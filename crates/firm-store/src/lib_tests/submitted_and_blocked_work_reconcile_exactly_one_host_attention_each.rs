use super::*;

#[test]
fn submitted_and_blocked_work_reconcile_exactly_one_host_attention_each() {
    let root = team_test_root("work-host-attention-reconciliation");
    let store = HarnessStore::new(&root);
    let (_review_run, review_member, review_work) =
        seed_host_attention_fixture(&store, "review-run", Some("review-host-task"));
    let started_review = start_claimed_work_for_test(
        &store,
        &review_work,
        &review_member,
        "work-event-review-started",
        "work-command-review-started",
        "unix-ms:3",
    );
    let submitted = submit_started_work_for_test(
        &store,
        &started_review,
        &review_member,
        "work-event-review-submitted",
        "ready for exact Host review",
        Vec::new(),
        vec!["cargo:test".into()],
        "unix-ms:4",
    );

    let (_blocked_run, blocked_member, blocked_work) =
        seed_host_attention_fixture(&store, "blocked-run", Some("blocked-host-task"));
    let started_blocked = start_claimed_work_for_test(
        &store,
        &blocked_work,
        &blocked_member,
        "work-event-blocked-started",
        "work-command-blocked-started",
        "unix-ms:5",
    );
    let blocked = store
        .block_work(
            &started_blocked.id,
            started_blocked.version,
            &blocked_member.id,
            "needs Host decision",
            member_work_context(
                &blocked_member.id,
                "work-event-blocked",
                "work-command-blocked",
                "unix-ms:6",
            ),
        )
        .expect("block Work");

    let attentions = store.host_attentions().expect("derived Host attentions");
    assert_eq!(
        attentions.len(),
        4,
        "each ordinary progress transition plus each urgent transition is durable"
    );
    let review_attention = attentions
        .iter()
        .find(|attention| {
            attention.work_id == submitted.id
                && attention.kind == HostAttentionKind::WorkReviewRequested
        })
        .expect("review attention");
    assert_eq!(
        review_attention.id,
        "host-attention-work-event-review-submitted"
    );
    assert_eq!(
        review_attention.kind,
        HostAttentionKind::WorkReviewRequested
    );
    assert_eq!(review_attention.work_version, submitted.version);
    let blocked_attention = attentions
        .iter()
        .find(|attention| {
            attention.work_id == blocked.id && attention.kind == HostAttentionKind::WorkBlocked
        })
        .expect("blocked attention");
    assert_eq!(blocked_attention.id, "host-attention-work-event-blocked");
    assert_eq!(blocked_attention.kind, HostAttentionKind::WorkBlocked);
    assert_eq!(blocked_attention.work_version, blocked.version);
    assert!(
        store
            .legacy_team_messages()
            .expect("Legacy TeamMessages")
            .is_empty(),
        "Work-state attention must not fabricate conversation"
    );

    // Simulate the process dying after work_operations.jsonl was fsynced
    // but before host_attentions.jsonl reached disk.
    std::fs::remove_file(root.join("host_attentions.jsonl"))
        .expect("remove derived ledger to simulate crash gap");
    let reconciled = store
        .reconcile_work_host_attentions()
        .expect("repair crash gap from WorkEvent truth");
    assert_eq!(reconciled.len(), 4);
    let repaired_bytes =
        std::fs::read(root.join("host_attentions.jsonl")).expect("repaired Host-attention ledger");
    assert_eq!(
        repaired_bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .count(),
        4
    );
    store
        .reconcile_work_host_attentions()
        .expect("idempotent second reconciliation");
    assert_eq!(
        std::fs::read(root.join("host_attentions.jsonl")).expect("stable ledger"),
        repaired_bytes,
        "reconciliation must not append duplicates"
    );

    std::fs::remove_dir_all(root).expect("remove temp store");
}
