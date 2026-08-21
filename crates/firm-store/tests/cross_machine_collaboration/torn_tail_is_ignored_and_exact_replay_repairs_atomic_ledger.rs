use super::*;

#[test]
fn torn_tail_is_ignored_and_exact_replay_repairs_atomic_ledger() {
    let test = TestStore::new("torn-tail");
    install_policy(&test.store);
    let auth = authority();
    let ctx = context(
        auth.source_host.clone(),
        "delegation.propose",
        "torn-propose",
        0,
    );
    test.store
        .propose_collaboration_delegation(&ctx, &proposal(), &auth, &policy())
        .expect("durable proposal");
    let ledger = test.root.join("agentfirm_collaboration_operations.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .unwrap();
    file.write_all(b"{\"torn\":").unwrap();
    file.sync_all().unwrap();

    let replay = test
        .store
        .propose_collaboration_delegation(&ctx, &proposal(), &auth, &policy())
        .expect("complete durable rows survive torn tail");
    assert!(replay.replayed);
    assert_eq!(
        test.store
            .collaboration_delegations("company-1")
            .unwrap()
            .len(),
        1
    );
}
