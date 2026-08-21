use super::*;

#[test]
fn commit_failure_and_torn_final_frame_recover_without_partial_state() {
    let root = TestRoot::new("recovery");
    let journal;
    let durable;
    {
        let store = FabricStore::open(root.path()).expect("open store");
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(COMPANY, [7; 32]);
        let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
        let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
        durable = store.snapshot().expect("durable snapshot");
        store.fail_next_commit_for_test();
        let error = control
            .create_enrollment(
                &actor("host", &["company_host"]),
                lease.control_plane_generation,
                "must-not-commit",
                TOKEN_A,
                "node-a",
                BTreeSet::new(),
                1000,
                2,
            )
            .expect_err("forced commit failure");
        assert_eq!(error.code, FabricErrorCode::StoreUnavailable);
        assert_eq!(store.snapshot().expect("snapshot"), durable);
        journal = store.journal_path().to_path_buf();
    }
    OpenOptions::new()
        .append(true)
        .open(&journal)
        .expect("open journal")
        .write_all(b"{\"transaction_sequence\":")
        .expect("append torn frame");
    let reopened = FabricStore::open(root.path()).expect("ignore torn final frame");
    assert_eq!(reopened.snapshot().expect("snapshot"), durable);

    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &reopened, &keys, [9; 32]);
    let lease = reopened.snapshot().expect("snapshot").control_plane_leases[COMPANY].clone();
    reopened.fail_after_append_for_test();
    let unknown = control
        .create_enrollment(
            &actor("host", &["company_host"]),
            lease.control_plane_generation,
            "commit-ack-lost",
            TOKEN_B,
            "node-b",
            BTreeSet::new(),
            1000,
            3,
        )
        .expect_err("lost commit acknowledgement is unknown, never effect-none");
    assert_eq!(unknown.code, FabricErrorCode::RecoveryRequired);
    assert_eq!(unknown.effect, EffectCertainty::Unknown);
    assert_eq!(
        reopened
            .snapshot()
            .expect_err("store latches unavailable")
            .code,
        FabricErrorCode::StoreUnavailable
    );
    drop(control);
    drop(reopened);
    let recovered = FabricStore::open(root.path()).expect("reopen complete committed frame");
    assert!(recovered
        .snapshot()
        .expect("snapshot")
        .enrollments
        .contains_key("commit-ack-lost"));
}
