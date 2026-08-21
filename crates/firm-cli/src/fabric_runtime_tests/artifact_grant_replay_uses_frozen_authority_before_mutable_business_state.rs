use super::*;

#[test]
fn artifact_grant_replay_uses_frozen_authority_before_mutable_business_state() {
    let delegation: harness_core::collaboration::WorkDelegationV1 =
        serde_json::from_str(include_str!(
            "../../../../schemas/collaboration/fixtures/work-delegation-v1/valid/awaiting.json"
        ))
        .expect("Delegation fixture");
    let attestation: harness_core::collaboration::SourceWorkAttestation =
            serde_json::from_str(include_str!(
                "../../../../schemas/collaboration/fixtures/source-work-attestation/valid/server-authored.json"
            ))
            .expect("attestation fixture");
    let manifest = harness_fabric::RemoteArtifactManifest {
        id: "artifact-replay".into(),
        company_id: delegation.company_id.clone(),
        source_node_id: delegation.target_placement.node_id.clone(),
        source_team_id: Some(delegation.target_placement.team_id.clone()),
        source_work_id: delegation
            .target_work_ref
            .as_ref()
            .map(|work| work.work_id.clone()),
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
    let capability = harness_fabric::ArtifactCapability {
        token: "redacted-test-capability".into(),
        company_id: delegation.company_id.clone(),
        artifact_id: manifest.id.clone(),
        artifact_digest: manifest.sha256.clone(),
        purpose: harness_fabric::ArtifactCapabilityPurpose::Download,
        node_id: delegation.source_node_id.clone(),
        issued_to: attestation.source_host_ref.id.clone(),
        issued_at_unix_ms: 12,
        expires_at_unix_ms: 1_000,
        one_use: true,
    };
    let source_placement = harness_core::collaboration::TargetPlacementRef {
        team_id: delegation.source_team_id.clone(),
        team_revision: delegation.source_work_ref.team_revision,
        node_id: delegation.source_node_id.clone(),
        placement_generation: delegation.source_work_ref.placement_generation,
    };
    let payload = serde_json::to_value(CollaborationArtifactGrantEnvelope {
        delegation_id: delegation.id.clone(),
        delegation: delegation.clone(),
        source_work_attestation: attestation,
        manifest: manifest.clone(),
        read_capability: capability,
        source_placement,
    })
    .expect("closed grant payload");
    let operation_id = format!("route-artifact-grant:{}:{}", delegation.id, manifest.id);
    let business = harness_core::collaboration::RoutedBusinessOperation {
        id: operation_id.clone(),
        protocol_version: "agentfirm.fabric.v1".into(),
        company_id: delegation.company_id.clone(),
        kind: harness_core::collaboration::RoutedBusinessKind::ArtifactGrant,
        authenticated_actor: delegation.target_host_ref.clone(),
        source_node_id: delegation.target_placement.node_id.clone(),
        target_placement: harness_core::collaboration::TargetPlacementRef {
            team_id: delegation.source_team_id.clone(),
            team_revision: delegation.source_work_ref.team_revision,
            node_id: delegation.source_node_id.clone(),
            placement_generation: delegation.source_work_ref.placement_generation,
        },
        expected_revision: delegation.revision,
        idempotency_key: "grant-replay-key".into(),
        payload_digest: harness_store::canonical_json_fingerprint(&payload),
        payload,
        required_capability: harness_core::collaboration::RoutedBusinessKind::ArtifactGrant
            .required_capability(),
        ordering_key: format!("delegation:{}", delegation.id),
        created_at: "unix-ms:12".into(),
    };
    let operation = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: delegation.company_id.clone(),
                actor_id: "control-plane:3".into(),
                actor_kind: harness_fabric::ActorKind::Service,
                role_bindings: BTreeSet::from(["company_control_plane".into()]),
                session_id: "control-plane:3".into(),
                issued_at_unix_ms: 12,
                expires_at_unix_ms: 1_000,
            },
            resolved_business_actor: delegation.target_host_ref.clone(),
            source: harness_store::CollaborationFabricSource::ControlPlane,
            control_plane_generation: 3,
            target_execution_space_id: Some("space-source".into()),
            created_at_unix_ms: 12,
            expires_at_unix_ms: 1_000,
        },
    )
    .expect("canonical artifact grant route");
    operation.validate_digest().expect("exact operation digest");

    let receipt = harness_fabric::RouteReceipt {
        id: "receipt-grant-accepted".into(),
        company_id: delegation.company_id.clone(),
        operation_id: operation_id.clone(),
        target_node_id: delegation.source_node_id.clone(),
        target_gateway_generation: 9,
        control_plane_generation: 3,
        route_seq: 1,
        kind: ReceiptKind::ControlPlaneAccepted,
        application_effect: None,
        result_schema: None,
        result: None,
        result_digest: None,
        error: None,
        created_at_unix_ms: 20,
        schema_version: harness_fabric::FABRIC_SCHEMA_VERSION.into(),
    };
    let operations = std::collections::BTreeMap::from([(operation_id, operation)]);
    let receipts = std::collections::BTreeMap::from([(receipt.id.clone(), receipt.clone())]);

    assert_eq!(
        resolve_frozen_artifact_grant_replay(
            &operations,
            &receipts,
            &delegation.company_id,
            &delegation.id,
            &manifest.id,
            "grant-replay-key",
            delegation.revision,
            "space-source",
            1_000,
            &delegation.target_host_ref,
        )
        .expect("exact frozen replay"),
        Some(receipt)
    );
    assert_eq!(
        resolve_frozen_artifact_grant_replay(
            &operations,
            &receipts,
            &delegation.company_id,
            &delegation.id,
            &manifest.id,
            "grant-replay-key",
            delegation.revision,
            "space-other",
            1_000,
            &delegation.target_host_ref,
        )
        .expect_err("changed scope conflicts")
        .code,
        FabricErrorCode::IdempotencyConflict
    );
    let hostile = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::AgentMember,
        id: "sibling-host".into(),
    };
    assert_eq!(
        resolve_frozen_artifact_grant_replay(
            &operations,
            &receipts,
            &delegation.company_id,
            &delegation.id,
            &manifest.id,
            "grant-replay-key",
            delegation.revision,
            "space-source",
            1_000,
            &hostile,
        )
        .expect_err("wrong actor conflicts")
        .code,
        FabricErrorCode::IdempotencyConflict
    );
}
