use super::*;

#[test]
fn legacy_team_message_delivery_mutators_reject_with_zero_store_side_effects() {
    let root = team_test_root("retired-team-message-delivery-writers");
    let store = HarnessStore::new(&root);
    store.init().expect("init legacy-history store");
    append_sparse_row(
        &root,
        "team_messages.jsonl",
        r#"{"id":"tm-history","team_run_id":"tr-history","sender_runtime_id":"host","kind":"message","body":"history","correlation_id":"corr-history","created_at":"unix-ms:1"}"#,
    );
    let ledger_path = root.join("team_messages.jsonl");
    let before = std::fs::read(&ledger_path).expect("read historical ledger before rejection");

    let acknowledge_error = store
        .acknowledge_team_message_delivery(
            "tr-history",
            "tm-history",
            "member-history",
            "unix-ms:2",
        )
        .expect_err("legacy acknowledgement must fail closed");
    assert!(acknowledge_error
        .to_string()
        .contains("RETIRED_RUNTIME_WRITER"));
    assert_eq!(
        std::fs::read(&ledger_path).expect("read ledger after acknowledgement rejection"),
        before,
        "rejected legacy acknowledgement must not append or rewrite history"
    );

    let reconcile_error = store
        .reconcile_team_message_delivery_claim(
            "tr-history",
            "tm-history",
            "member-history",
            "claim-history",
            true,
            Some("provider-receipt-history"),
            "unix-ms:3",
        )
        .expect_err("legacy reconciliation must fail closed");
    assert!(reconcile_error
        .to_string()
        .contains("RETIRED_RUNTIME_WRITER"));
    assert_eq!(
        std::fs::read(&ledger_path).expect("read ledger after reconciliation rejection"),
        before,
        "rejected legacy reconciliation must not append or rewrite history"
    );

    assert!(
        !root.join(".store.lock").exists(),
        "retired seams must reject before acquiring a writer lock"
    );
    std::fs::remove_dir_all(root).expect("remove temp store");
}
