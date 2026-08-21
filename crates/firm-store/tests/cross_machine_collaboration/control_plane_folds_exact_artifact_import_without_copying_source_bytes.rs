use super::*;

#[test]
fn control_plane_folds_exact_artifact_import_without_copying_source_bytes() {
    let central = TestStore::new("central-artifact-import");
    active_delegation(&central.store);
    let bytes = b"source-only-bytes";
    let import = ArtifactImport {
        id: "artifact-import:artifact-2".into(),
        company_id: "company-1".into(),
        delegation_id: "delegation-1".into(),
        artifact_id: "artifact-2".into(),
        artifact_digest: firm_fabric::sha256_hex(bytes),
        size_bytes: bytes.len() as u64,
        source_node_id: "node-a".into(),
        source_node_daemon_id: "daemon-a".into(),
        source_node_daemon_generation: 9,
        source_team_id: "team-a".into(),
        source_host_ref: actor(ActorKind::AgentMember, "host-a"),
        source_work_ref: source_attestation().source_work_ref,
        operation_id: "artifact-route-2".into(),
        imported_at_unix_ms: 501,
        revision: 1,
    };
    let cp = actor(ActorKind::Service, "control-plane:1");
    let folded = central
        .store
        .record_collaboration_artifact_import(
            &context(cp.clone(), "artifact_import.fold", "fold-artifact-2", 0),
            &import,
            "artifact-route-2",
            &cp,
        )
        .unwrap();
    assert_eq!(folded.projection, import);
    assert!(
        central
            .store
            .read_collaboration_artifact_import_bytes("company-1", "artifact-2")
            .is_err(),
        "central projection must not become artifact byte authority"
    );
    let rows = central.store.collaboration_operations().unwrap().len();
    let mut hostile = folded.projection;
    hostile.source_node_daemon_generation += 1;
    assert!(central
        .store
        .record_collaboration_artifact_import(
            &context(cp.clone(), "artifact_import.fold", "fold-artifact-2", 0),
            &hostile,
            "artifact-route-2",
            &cp,
        )
        .is_err());
    assert_eq!(
        central.store.collaboration_operations().unwrap().len(),
        rows
    );
}
