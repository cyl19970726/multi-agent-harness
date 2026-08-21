use super::*;

#[test]
fn sha256_matches_standard_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn latest_projection_retains_last_row_bytes() {
    let bytes = b"{\"id\":\"a\",\"v\":1}\n{\"id\":\"b\",\"v\":1}\n{\"id\":\"a\",\"v\":2}";
    assert_eq!(
        latest_projection(bytes, "test.jsonl").unwrap(),
        b"{\"id\":\"b\",\"v\":1}\n{\"id\":\"a\",\"v\":2}"
    );
}

#[test]
fn only_contract_paths_become_edges() {
    let value = serde_json::json!({
        "id": "x",
        "goal_id": "g1",
        "phase_runs": [{"phase_id": "p1"}],
        "result": {"task_id": "dynamic-must-not-scan"}
    });
    let mut links = Vec::new();
    collect_link_values("goal_orchestration_runs.jsonl", &value, &mut links);
    assert_eq!(links.len(), 2);
    assert!(links
        .iter()
        .any(|(path, _, id)| path == "/goal_id" && id == "g1"));
    assert!(links
        .iter()
        .any(|(path, _, id)| path == "/phase_runs/0/phase_id" && id == "p1"));
    assert!(!links.iter().any(|(_, _, id)| id == "dynamic-must-not-scan"));
}

#[test]
fn snapshot_detects_file_set_size_and_hash_changes() {
    let root = std::env::temp_dir().join(format!(
        "legacy-export-snapshot-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("ledger.jsonl"), b"one\n").unwrap();
    let before = snapshot_directory(&root).unwrap();

    fs::write(root.join("ledger.jsonl"), b"two\n").unwrap();
    let hash_changed = snapshot_directory(&root).unwrap();
    assert_ne!(before, hash_changed, "same size but new hash must differ");

    fs::write(root.join("new.jsonl"), b"{}\n").unwrap();
    let set_changed = snapshot_directory(&root).unwrap();
    assert_ne!(hash_changed, set_changed, "new file set must differ");

    fs::write(root.join("ledger.jsonl"), b"longer\n").unwrap();
    let size_changed = snapshot_directory(&root).unwrap();
    assert_ne!(set_changed, size_changed, "new size must differ");

    let source = SourceSpec {
        id: "test".into(),
        kind: "test".into(),
        root: root.clone(),
        before: set_changed,
    };
    let error = ensure_source_unchanged(&source).unwrap_err();
    assert!(error.contains("refusing mixed snapshot"));
    fs::remove_dir_all(root).unwrap();
}
