use super::*;

#[test]
fn submitted_and_blocked_work_reconcile_exactly_one_host_attention_each() {
    let root = team_test_root("work-host-attention-reconciliation");
    let store = HarnessStore::new(&root);
    let (_review_run, review_member, review_work) =
        seed_host_attention_fixture(&store, "review-run", Some("review-host-task"));
    let started_review = store
        .start_work(
            &review_work.id,
            review_work.version,
            &review_member.id,
            member_work_context(
                &review_member.id,
                "work-event-review-started",
                "work-command-review-started",
                "unix-ms:3",
            ),
        )
        .expect("start review Work");
    let submitted = store
        .submit_work(
            &started_review.id,
            started_review.version,
            &review_member.id,
            "ready for exact Host review",
            Vec::new(),
            vec!["cargo:test".into()],
            member_work_context(
                &review_member.id,
                "work-event-review-submitted",
                "work-command-review-submitted",
                "unix-ms:4",
            ),
        )
        .expect("submit Work");

    let (_blocked_run, blocked_member, blocked_work) =
        seed_host_attention_fixture(&store, "blocked-run", Some("blocked-host-task"));
    let started_blocked = store
        .start_work(
            &blocked_work.id,
            blocked_work.version,
            &blocked_member.id,
            member_work_context(
                &blocked_member.id,
                "work-event-blocked-started",
                "work-command-blocked-started",
                "unix-ms:5",
            ),
        )
        .expect("start blocked Work");
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
    assert_eq!(attentions.len(), 2);
    let review_attention = attentions
        .iter()
        .find(|attention| attention.work_id == submitted.id)
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
        .find(|attention| attention.work_id == blocked.id)
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
    assert_eq!(reconciled.len(), 2);
    let repaired_bytes =
        std::fs::read(root.join("host_attentions.jsonl")).expect("repaired Host-attention ledger");
    assert_eq!(
        repaired_bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .count(),
        2
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
