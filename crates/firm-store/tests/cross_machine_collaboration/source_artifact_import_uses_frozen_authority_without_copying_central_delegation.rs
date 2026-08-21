use super::*;

#[test]
fn source_artifact_import_uses_frozen_authority_without_copying_central_delegation() {
    let source = TestStore::new("artifact-import-frozen-authority");
    active_delegation(&source.store);
    let mut delegation = source
        .store
        .collaboration_delegation("company-1", "delegation-1")
        .unwrap()
        .unwrap();
    let attestation = source_attestation();
    delegation.source_work_attestation_id = attestation.id.clone();
    delegation.source_work_ref = attestation.source_work_ref.clone();
    delegation.source_owner_ref = attestation.source_owner_ref.clone();
    let isolated_source = TestStore::new("artifact-import-frozen-authority-empty");

    let bytes = b"source-owned-import-with-frozen-authority";
    let import = ArtifactImport {
        id: "artifact-import:artifact-frozen".into(),
        company_id: "company-1".into(),
        delegation_id: delegation.id.clone(),
        artifact_id: "artifact-frozen".into(),
        artifact_digest: firm_fabric::sha256_hex(bytes),
        size_bytes: bytes.len() as u64,
        source_node_id: delegation.source_node_id.clone(),
        source_node_daemon_id: "daemon-a".into(),
        source_node_daemon_generation: 9,
        source_team_id: delegation.source_team_id.clone(),
        source_host_ref: attestation.source_host_ref.clone(),
        source_work_ref: delegation.source_work_ref.clone(),
        operation_id: "artifact-grant-frozen".into(),
        imported_at_unix_ms: 600,
        revision: 1,
    };
    let ctx = context(
        actor(ActorKind::Service, "daemon-a"),
        "artifact_import.persist",
        "artifact-grant-frozen",
        0,
    );
    let before = isolated_source.store.collaboration_operations().unwrap();
    isolated_source
        .store
        .persist_collaboration_artifact_import_with_frozen_authority(
            &ctx,
            &import,
            bytes,
            &delegation,
            &attestation,
        )
        .expect("source persists bytes from frozen central authority");
    assert!(isolated_source
        .store
        .collaboration_delegation("company-1", &delegation.id)
        .unwrap()
        .is_none());
    assert_eq!(
        isolated_source
            .store
            .read_collaboration_artifact_import_bytes("company-1", "artifact-frozen")
            .unwrap(),
        bytes
    );

    let rows = isolated_source
        .store
        .collaboration_operations()
        .unwrap()
        .len();
    let mut hostile = delegation;
    hostile.source_node_id = "node-hostile".into();
    assert!(isolated_source
        .store
        .persist_collaboration_artifact_import_with_frozen_authority(
            &ctx,
            &import,
            bytes,
            &hostile,
            &attestation,
        )
        .is_err());
    assert_eq!(
        isolated_source
            .store
            .collaboration_operations()
            .unwrap()
            .len(),
        rows
    );
    assert_eq!(before.len() + 1, rows);
}
