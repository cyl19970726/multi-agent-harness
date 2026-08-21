use super::*;

#[test]
fn stale_unknown_delivery_attempt_blocks_more_delivery() {
    let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("stale")));
    let store = HarnessStore::new(&root);
    append_test_delivery_attempt(
        &store,
        "agent-1",
        Some("task-1"),
        ProviderExecutionStatus::Stale,
        Some("thread-1"),
        Some("turn-1"),
    );

    assert!(has_unresolved_delivery_attempt(&store, "agent-1").expect("running check"));

    let _ = std::fs::remove_dir_all(root);
}
