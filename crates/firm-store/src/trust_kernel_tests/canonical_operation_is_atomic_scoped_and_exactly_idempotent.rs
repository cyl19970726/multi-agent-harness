use super::*;

#[test]
fn canonical_operation_is_atomic_scoped_and_exactly_idempotent() {
    let root = std::env::temp_dir().join(format!(
        "firm-trust-kernel-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = HarnessStore::new(&root);
    let first = store
        .create_trust_agent_member(
            &context("host", "agent_member.create", "same", 0),
            member("member-1"),
        )
        .expect("create");
    assert!(!first.replayed);
    let replay = store
        .create_trust_agent_member(
            &context("host", "agent_member.create", "same", 0),
            member("member-1"),
        )
        .expect("replay");
    assert!(replay.replayed);
    assert_eq!(first.event.id, replay.event.id);
    assert_eq!(store.canonical_operations().unwrap().len(), 1);

    let mut changed = member("member-1");
    changed.role = "reviewer".into();
    let error = store
        .create_trust_agent_member(&context("host", "agent_member.create", "same", 0), changed)
        .expect_err("payload drift conflicts")
        .to_string();
    assert!(error.contains("IDEMPOTENCY_KEY_REUSED"), "{error}");

    let mut other_member = member("member-2");
    other_member.created_by = actor("another");
    store
        .create_trust_agent_member(
            &context("another", "agent_member.create", "same", 0),
            other_member,
        )
        .expect("same key in another authenticated actor scope");
    assert_eq!(store.canonical_operations().unwrap().len(), 2);
    fs::remove_dir_all(root).unwrap();
}
