use super::*;

#[test]
fn artifact_digest_scope_encryption_and_one_use_capability_fail_closed() {
    let root = TestRoot::new("artifact");
    let store = FabricStore::open(root.path()).expect("open store");
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(COMPANY, [7; 32]);
    let control = ControlPlane::new(COMPANY, "control-1", &store, &keys, [9; 32]);
    let lease = control.acquire_lease("cp-lease", 0, 1).expect("lease");
    enroll_nodes(&control, lease.control_plane_generation);
    let bytes = b"bounded deterministic artifact";
    let writer = actor("writer", &["artifact_write"]);
    let (manifest, upload) = control
        .initiate_artifact(
            &writer,
            lease.control_plane_generation,
            "artifact-1",
            "node-a",
            None,
            "text/plain",
            bytes.len() as u64,
            &sha256_hex(bytes),
            ArtifactClassification::CompanyInternal,
            BTreeSet::from(["reader".into()]),
            100,
        )
        .expect("initiate artifact");
    let scoped = control
        .bind_collaboration_artifact_scope(
            &writer,
            lease.control_plane_generation,
            &manifest.id,
            manifest.revision,
            "team-b",
            "work-b",
            BTreeSet::from(["reader".into()]),
            100,
        )
        .expect("server binds exact collaboration Team/Work/readers before upload");
    assert_eq!(scoped.source_team_id.as_deref(), Some("team-b"));
    assert_eq!(scoped.source_work_id.as_deref(), Some("work-b"));
    assert_eq!(
        control.artifact_manifest(&manifest.id).unwrap(),
        Some(scoped.clone())
    );
    let before_rebind = store.snapshot().unwrap();
    assert!(control
        .bind_collaboration_artifact_scope(
            &writer,
            lease.control_plane_generation,
            &manifest.id,
            scoped.revision,
            "team-c",
            "work-c",
            BTreeSet::from(["attacker".into()]),
            100,
        )
        .is_err());
    assert_eq!(store.snapshot().unwrap(), before_rebind);
    let completed = control
        .complete_artifact(lease.control_plane_generation, &upload, bytes, 101)
        .expect("complete artifact");
    assert_eq!(completed.id, manifest.id);
    let retained = control
        .schedule_collaboration_artifact_retention(
            &writer,
            lease.control_plane_generation,
            &manifest.id,
            completed.revision,
            1_000_000,
            102,
        )
        .expect("safe collaboration retention anchor schedules future expiry");
    assert_eq!(retained.expires_at_unix_ms, Some(1_000_000));
    let delegated = control
        .issue_delegated_download_capability(
            &writer,
            lease.control_plane_generation,
            &manifest.id,
            "reader",
            "node-b",
            102,
        )
        .expect("exact artifact initiator grants one-use download to frozen reader and Node");
    assert_eq!(delegated.issued_to, "reader");
    assert_eq!(delegated.node_id, "node-b");
    let before_hostile_grant = store.snapshot().unwrap();
    let hostile_grantor = actor("other-writer", &["artifact_write"]);
    assert!(control
        .issue_delegated_download_capability(
            &hostile_grantor,
            lease.control_plane_generation,
            &manifest.id,
            "reader",
            "node-b",
            102,
        )
        .is_err());
    assert_eq!(store.snapshot().unwrap(), before_hostile_grant);
    let replay = control
        .complete_artifact(lease.control_plane_generation, &upload, bytes, 102)
        .expect_err("upload capability is one-use");
    assert_eq!(replay.code, FabricErrorCode::CapabilityConsumed);
    let reader = actor("reader", &["artifact_read"]);
    let first_hello = hello("node-b", "gateway-b-1", "cert-b", &fingerprint("node-b"));
    let first_gateway = connect_node(
        &control,
        lease.control_plane_generation,
        &first_hello,
        &signing_key("node-b"),
        103,
    )
    .expect("connect first target gateway");
    control
        .issue_gateway_download_capability(
            &reader,
            lease.control_plane_generation,
            first_gateway.gateway_generation,
            &first_gateway.node_daemon_id,
            first_gateway.node_daemon_generation,
            &manifest.id,
            "node-b",
            104,
        )
        .expect("current target gateway can request exact artifact capability");
    control
        .heartbeat_lease(lease.control_plane_generation, lease.revision, 29_000)
        .expect("keep Control Plane authority current");
    let successor_hello = rotate_gateway_certificate(
        &control,
        &store,
        lease.control_plane_generation,
        &first_gateway,
        "node-b",
        "cert-b",
        "cert-b-successor",
        29_500,
    );
    connect_node(
        &control,
        lease.control_plane_generation,
        &successor_hello,
        &signing_key("node-b"),
        30_104,
    )
    .expect("successor target gateway binds after predecessor expiry");
    let before_stale_capability = store.snapshot().expect("snapshot");
    let stale_capability = control
        .issue_gateway_download_capability(
            &reader,
            lease.control_plane_generation,
            first_gateway.gateway_generation,
            &first_gateway.node_daemon_id,
            first_gateway.node_daemon_generation,
            &manifest.id,
            "node-b",
            30_105,
        )
        .expect_err("predecessor gateway cannot mint artifact capability");
    assert_eq!(stale_capability.code, FabricErrorCode::NodeStaleGeneration);
    assert_eq!(stale_capability.effect, EffectCertainty::None);
    assert_eq!(store.snapshot().expect("snapshot"), before_stale_capability);
    let download = control
        .issue_download_capability(
            &reader,
            lease.control_plane_generation,
            &manifest.id,
            "node-b",
            30_106,
        )
        .expect("issue download capability");
    assert_eq!(
        control
            .download_artifact(lease.control_plane_generation, &download, 30_107)
            .expect("decrypt artifact"),
        bytes
    );
    let consumed = control
        .download_artifact(lease.control_plane_generation, &download, 30_108)
        .expect_err("download capability is one-use");
    assert_eq!(consumed.code, FabricErrorCode::CapabilityConsumed);
    let journal_bytes = std::fs::read(store.journal_path()).expect("read journal");
    assert!(!journal_bytes
        .windows(bytes.len())
        .any(|window| window == bytes));

    let secret = b"-----BEGIN PRIVATE KEY-----\nnot-real";
    let (_, secret_upload) = control
        .initiate_artifact(
            &writer,
            lease.control_plane_generation,
            "artifact-secret",
            "node-a",
            None,
            "text/plain",
            secret.len() as u64,
            &sha256_hex(secret),
            ArtifactClassification::Sensitive,
            BTreeSet::from(["reader".into()]),
            30_109,
        )
        .expect("manifest can precede content inspection");
    let before = store.snapshot().expect("snapshot");
    let rejected = control
        .complete_artifact(
            lease.control_plane_generation,
            &secret_upload,
            secret,
            30_110,
        )
        .expect_err("secret-like payload must fail closed");
    assert_eq!(rejected.code, FabricErrorCode::ArtifactTampered);
    assert_eq!(store.snapshot().expect("snapshot"), before);
}
