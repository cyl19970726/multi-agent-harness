use super::*;

#[test]
fn source_artifact_import_is_digest_bound_durable_and_exactly_replayed() {
    let test = TestStore::new("artifact-import");
    active_delegation(&test.store);
    let bytes = b"source-owned-import";
    let digest = firm_fabric::sha256_hex(bytes);
    let import = ArtifactImport {
        id: "artifact-import:artifact-1".into(),
        company_id: "company-1".into(),
        delegation_id: "delegation-1".into(),
        artifact_id: "artifact-1".into(),
        artifact_digest: digest.clone(),
        size_bytes: bytes.len() as u64,
        source_node_id: "node-a".into(),
        source_node_daemon_id: "daemon-a".into(),
        source_node_daemon_generation: 9,
        source_team_id: "team-a".into(),
        source_host_ref: actor(ActorKind::AgentMember, "host-a"),
        source_work_ref: source_attestation().source_work_ref,
        operation_id: "artifact-grant-route-1".into(),
        imported_at_unix_ms: 500,
        revision: 1,
    };
    let ctx = context(
        actor(ActorKind::Service, "daemon-a"),
        "artifact_import.persist",
        "artifact-grant-route-1",
        0,
    );
    let committed = test
        .store
        .persist_collaboration_artifact_import(&ctx, &import, bytes)
        .expect("verified bytes become canonical source import");
    assert!(!committed.replayed);
    assert_eq!(
        test.store
            .read_collaboration_artifact_import_bytes("company-1", "artifact-1")
            .unwrap(),
        bytes,
    );
    let rows = test.store.collaboration_operations().unwrap().len();
    let replay = test
        .store
        .persist_collaboration_artifact_import(&ctx, &import, bytes)
        .expect("exact replay does not download or import twice");
    assert!(replay.replayed);
    assert_eq!(test.store.collaboration_operations().unwrap().len(), rows);

    let mut tampered = import;
    tampered.artifact_digest = firm_fabric::sha256_hex(b"different");
    assert!(test
        .store
        .persist_collaboration_artifact_import(&ctx, &tampered, bytes)
        .is_err());
    assert_eq!(test.store.collaboration_operations().unwrap().len(), rows);
}
