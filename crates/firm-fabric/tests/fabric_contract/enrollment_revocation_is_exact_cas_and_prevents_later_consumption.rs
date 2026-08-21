use super::*;

#[test]
fn enrollment_revocation_is_exact_cas_and_prevents_later_consumption() {
    let root = TestRoot::new("enrollment-revoke");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    let host = actor("host", &["company_host"]);
    let enrollment = control
        .create_enrollment(
            &host,
            lease.control_plane_generation,
            "enroll-revoked",
            TOKEN_A,
            "Node revoked before join",
            BTreeSet::new(),
            1000,
            2,
        )
        .expect("create enrollment");
    let before_stale = store.snapshot().expect("snapshot");
    let stale = control
        .revoke_enrollment(
            &host,
            lease.control_plane_generation,
            &enrollment.id,
            enrollment.revision + 1,
            3,
        )
        .expect_err("stale CAS cannot revoke enrollment");
    assert_eq!(stale.code, FabricErrorCode::ExpectedRevisionConflict);
    assert_eq!(store.snapshot().expect("snapshot"), before_stale);
    let revoked = control
        .revoke_enrollment(
            &host,
            lease.control_plane_generation,
            &enrollment.id,
            enrollment.revision,
            3,
        )
        .expect("revoke pending enrollment");
    assert_eq!(revoked.status, EnrollmentStatus::Revoked);
    assert_eq!(revoked.revision, enrollment.revision + 1);
    let before_consume = store.snapshot().expect("snapshot");
    let rejected = control
        .consume_enrollment(
            lease.control_plane_generation,
            TOKEN_A,
            "node-a",
            "Node A",
            &enrollment_proof("enroll-revoked", "node-a", "cert-a"),
            "cert-a",
            10_000,
            SCHEMA_DIGEST,
            4,
        )
        .expect_err("revoked token cannot be consumed");
    assert_eq!(rejected.code, FabricErrorCode::EnrollmentRevoked);
    assert_eq!(store.snapshot().expect("snapshot"), before_consume);
}
