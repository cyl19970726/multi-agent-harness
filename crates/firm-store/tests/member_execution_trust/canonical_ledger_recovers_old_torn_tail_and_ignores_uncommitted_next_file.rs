use super::*;

#[test]
fn canonical_ledger_recovers_old_torn_tail_and_ignores_uncommitted_next_file() {
    let harness = TestStore::new("canonical-crash-recovery");
    let host = human("host");
    harness
        .store
        .create_trust_agent_member(
            &context(host.clone(), "member.create", "create-a", 0),
            member("member-a", &host),
        )
        .expect("commit first canonical operation");

    let ledger = harness.root.join("agentfirm_trust_operations.jsonl");
    let next = harness.root.join("agentfirm_trust_operations.jsonl.next");
    std::fs::write(&next, b"{\"uncommitted\":").expect("simulate crash before rename");
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 1);

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&ledger)
        .expect("open canonical ledger");
    file.write_all(b"{\"torn\":")
        .expect("simulate legacy append tear");
    file.sync_all().expect("persist torn tail");
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 1);

    harness
        .store
        .create_trust_agent_member(
            &context(host.clone(), "member.create", "create-b", 0),
            member("member-b", &host),
        )
        .expect("next commit atomically replaces torn ledger");
    assert_eq!(harness.store.canonical_operations().unwrap().len(), 2);
    let repaired = std::fs::read(&ledger).expect("read repaired ledger");
    assert!(repaired.ends_with(b"\n"));
    assert!(!next.exists(), "atomic rename consumes the next file");
    for row in repaired.split(|byte| *byte == b'\n') {
        if !row.is_empty() {
            serde_json::from_slice::<serde_json::Value>(row).expect("complete JSON frame");
        }
    }
}
