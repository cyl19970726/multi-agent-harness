use super::*;

#[test]
fn durable_checkpoint_recovers_stale_or_torn_cache_and_never_hides_journal_tamper() {
    let root = TestRoot::new("durable-checkpoint");
    let checkpoint = root.path().join("fabric-checkpoint.json");
    let journal;
    let expected;
    let stale_checkpoint;
    {
        let store = FabricStore::open(root.path()).expect("open store");
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(COMPANY, [7; 32]);
        let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
        let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
        stale_checkpoint = std::fs::read(&checkpoint).expect("first durable checkpoint");
        control
            .heartbeat_lease(lease.control_plane_generation, lease.revision, 2)
            .expect("second durable transaction");
        expected = store.snapshot().expect("current snapshot");
        journal = store.journal_path().to_path_buf();
    }

    std::fs::write(&checkpoint, &stale_checkpoint).expect("restore stale checkpoint");
    let reopened = FabricStore::open(root.path()).expect("replay suffix after stale checkpoint");
    let checkpoint_modified = std::fs::metadata(&checkpoint)
        .expect("checkpoint metadata")
        .modified()
        .expect("checkpoint modified time");
    std::thread::sleep(std::time::Duration::from_millis(10));
    assert_eq!(reopened.snapshot().expect("snapshot"), expected);
    assert_eq!(
        std::fs::metadata(&checkpoint)
            .expect("checkpoint metadata after read")
            .modified()
            .expect("checkpoint modified time after read"),
        checkpoint_modified,
        "a current checkpoint is read-only on snapshot"
    );
    drop(reopened);

    std::fs::write(&checkpoint, b"{torn-checkpoint").expect("write torn checkpoint cache");
    let reopened = FabricStore::open(root.path()).expect("fall back to full journal validation");
    assert_eq!(reopened.snapshot().expect("snapshot"), expected);
    drop(reopened);

    let mut journal_bytes = std::fs::read(&journal).expect("read journal");
    let changed = journal_bytes
        .iter_mut()
        .find(|byte| **byte == b'c')
        .expect("journal has a byte to tamper");
    *changed = b'd';
    std::fs::write(&journal, journal_bytes).expect("tamper validated checkpoint prefix");
    assert_eq!(
        FabricStore::open(root.path())
            .err()
            .expect("checkpoint cannot hide journal tamper")
            .code,
        FabricErrorCode::StoreUnavailable
    );
}
