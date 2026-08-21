use super::*;

#[test]
fn concurrent_one_use_enrollment_has_exactly_one_winner() {
    let root = TestRoot::new("concurrent-enrollment");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    control
        .create_enrollment(
            &actor("host", &["company_host"]),
            lease.control_plane_generation,
            "enroll-a",
            TOKEN_A,
            "node-a",
            BTreeSet::new(),
            1000,
            2,
        )
        .expect("create enrollment");
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let client = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
            client.consume_enrollment(
                lease.control_plane_generation,
                TOKEN_A,
                "node-a",
                "Node A",
                &enrollment_proof("enroll-a", "node-a", "cert-a"),
                "cert-a",
                10_000,
                SCHEMA_DIGEST,
                3,
            )
        });
        let second = scope.spawn(|| {
            let client = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
            client.consume_enrollment(
                lease.control_plane_generation,
                TOKEN_A,
                "node-a-duplicate",
                "Node A duplicate",
                &enrollment_proof("enroll-a", "node-a-duplicate", "cert-a-duplicate"),
                "cert-a-duplicate",
                10_000,
                SCHEMA_DIGEST,
                3,
            )
        });
        [
            first.join().expect("first thread"),
            second.join().expect("second thread"),
        ]
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .filter(|error| error.code == FabricErrorCode::EnrollmentConsumed)
            .count(),
        1
    );
    let state = store.snapshot().expect("snapshot");
    assert_eq!(state.nodes.len(), 1);
    assert_eq!(state.certificates.len(), 1);
}
