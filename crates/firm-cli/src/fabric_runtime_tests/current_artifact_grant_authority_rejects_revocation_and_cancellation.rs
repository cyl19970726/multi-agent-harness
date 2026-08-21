use super::*;

#[test]
fn current_artifact_grant_authority_rejects_revocation_and_cancellation() {
    let (attestation, mut delegation, mut policy, publication, _) = current_remote_fact_fixture();
    let manifest = harness_fabric::RemoteArtifactManifest {
        id: "artifact-fenced".into(),
        company_id: delegation.company_id.clone(),
        source_node_id: delegation.target_placement.node_id.clone(),
        source_team_id: Some(delegation.target_placement.team_id.clone()),
        source_work_id: Some(publication.fact_work_ref.work_id),
        operation_id: None,
        media_type: "text/plain".into(),
        size_bytes: 5,
        sha256: harness_fabric::sha256_hex(b"hello"),
        classification: harness_fabric::ArtifactClassification::CompanyInternal,
        initiator: delegation.target_host_ref.id.clone(),
        authorized_readers: BTreeSet::from([attestation.source_host_ref.id.clone()]),
        created_by: delegation.target_host_ref.id.clone(),
        revision: 1,
        created_at_unix_ms: 10,
        expires_at_unix_ms: None,
        completed_at_unix_ms: Some(11),
        deleted_at_unix_ms: None,
        schema_version: harness_fabric::FABRIC_SCHEMA_VERSION.into(),
    };
    validate_current_artifact_grant_authority(
        "company-1",
        &delegation,
        &attestation,
        &policy,
        &manifest,
        &delegation.target_host_ref,
        delegation.revision,
    )
    .unwrap();
    policy.revoked_at = Some("unix-ms:12".into());
    assert!(validate_current_artifact_grant_authority(
        "company-1",
        &delegation,
        &attestation,
        &policy,
        &manifest,
        &delegation.target_host_ref,
        delegation.revision,
    )
    .is_err());
    policy.revoked_at = None;
    delegation.state = harness_core::collaboration::DelegationState::CancellationRequested;
    assert!(validate_current_artifact_grant_authority(
        "company-1",
        &delegation,
        &attestation,
        &policy,
        &manifest,
        &delegation.target_host_ref,
        delegation.revision,
    )
    .is_err());
}
