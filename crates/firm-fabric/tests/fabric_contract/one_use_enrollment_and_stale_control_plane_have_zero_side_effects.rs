use super::*;

#[test]
fn one_use_enrollment_and_stale_control_plane_have_zero_side_effects() {
    let root = TestRoot::new("enrollment");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let old = ControlPlane::new(COMPANY, "control-old", &store, &keys, [9; 32]);
    let lease = old.acquire_lease("cp-lease-1", 0, 1).expect("lease");
    let host = actor("host", &["company_host"]);
    old.create_enrollment(
        &host,
        lease.control_plane_generation,
        "enrollment-1",
        TOKEN_A,
        "node-a",
        BTreeSet::new(),
        1000,
        2,
    )
    .expect("create enrollment");
    for invalid_expiry in [
        3,
        3 + firm_fabric::enrollment::NODE_CERTIFICATE_LIFETIME_MAX_MS + 1,
    ] {
        let before_invalid = store.snapshot().expect("snapshot");
        let invalid = old
            .consume_enrollment(
                lease.control_plane_generation,
                TOKEN_A,
                "node-a",
                "Node A",
                &enrollment_proof("enrollment-1", "node-a", "cert-a"),
                "cert-a",
                invalid_expiry,
                SCHEMA_DIGEST,
                3,
            )
            .expect_err("certificate lifetime must be bounded");
        assert_eq!(invalid.code, FabricErrorCode::EnrollmentInvalid);
        assert_eq!(store.snapshot().expect("snapshot"), before_invalid);
    }
    old.consume_enrollment(
        lease.control_plane_generation,
        TOKEN_A,
        "node-a",
        "Node A",
        &enrollment_proof("enrollment-1", "node-a", "cert-a"),
        "cert-a",
        10_000,
        SCHEMA_DIGEST,
        3,
    )
    .expect("consume once");
    let before = store.snapshot().expect("snapshot");
    let replay = old
        .consume_enrollment(
            lease.control_plane_generation,
            TOKEN_A,
            "node-a-replay",
            "Node Replay",
            &enrollment_proof("enrollment-1", "node-a-replay", "cert-replay"),
            "cert-replay",
            10_000,
            SCHEMA_DIGEST,
            4,
        )
        .expect_err("one-use enrollment must reject replay");
    assert_eq!(replay.code, FabricErrorCode::EnrollmentConsumed);
    assert_eq!(store.snapshot().expect("snapshot"), before);

    let successor = ControlPlane::new(COMPANY, "control-new", &store, &keys, [9; 32]);
    let next = successor
        .acquire_lease("cp-lease-2", lease.revision, 31_001)
        .expect("successor after expiry");
    let before_stale = store.snapshot().expect("snapshot");
    let stale = old
        .create_enrollment(
            &host,
            next.control_plane_generation,
            "stale-enrollment",
            "stale-enrollment-token-000000000000000000",
            "stale",
            BTreeSet::new(),
            31_100,
            31_010,
        )
        .expect_err("stale instance cannot borrow successor generation");
    assert_eq!(stale.code, FabricErrorCode::ControlPlaneStaleGeneration);
    assert_eq!(store.snapshot().expect("snapshot"), before_stale);
}
