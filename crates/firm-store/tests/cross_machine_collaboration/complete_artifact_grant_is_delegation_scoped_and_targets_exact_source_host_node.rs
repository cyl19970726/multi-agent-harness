use super::*;

#[test]
fn complete_artifact_grant_is_delegation_scoped_and_targets_exact_source_host_node() {
    let central = TestStore::new("artifact-grant-central");
    install_policy(&central.store);
    let auth = authority();
    let mut attestation = source_attestation();
    attestation.id = "artifact-source-attestation".into();
    attestation.source_work_ref.node_id = SOURCE_NODE_UUID.into();
    attestation.source_work_ref.execution_space_id = "space-node-a".into();
    attestation.source_work_ref.team_revision = 1;
    attestation.attestation_digest = canonical_json_fingerprint(&serde_json::json!({
        "id": attestation.id,
        "company_id": attestation.company_id,
        "source_work_ref": attestation.source_work_ref,
        "source_owner_ref": attestation.source_owner_ref,
        "source_host_ref": attestation.source_host_ref,
        "work_application_service_ref": attestation.work_application_service_ref,
        "source_gateway_generation": attestation.source_gateway_generation,
        "issued_at": attestation.issued_at,
    }));
    central
        .store
        .put_source_work_attestation(
            &context(
                attestation.work_application_service_ref.clone(),
                "source_work.attest",
                "artifact-source-attest",
                0,
            ),
            &attestation,
            &attestation.work_application_service_ref,
            8,
        )
        .unwrap();
    let mut artifact_proposal = proposal();
    artifact_proposal.delegation_id = "artifact-delegation".into();
    artifact_proposal.source_work_attestation_id = attestation.id.clone();
    central
        .store
        .propose_collaboration_delegation(
            &context(
                auth.source_host.clone(),
                "delegation.propose",
                "artifact-propose",
                0,
            ),
            &artifact_proposal,
            &auth,
            &policy(),
        )
        .unwrap();
    central
        .store
        .decide_collaboration_delegation(
            &context(
                auth.target_host.clone(),
                "delegation.decide",
                "artifact-accept",
                1,
            ),
            &artifact_proposal.delegation_id,
            &DelegationDecision {
                id: "artifact-accept".into(),
                delegation_id: artifact_proposal.delegation_id.clone(),
                expected_delegation_revision: 1,
                decision: DelegationDecisionKind::Accept,
                decided_by_target_host: auth.target_host.clone(),
                reason: "accept artifact-producing Work".into(),
                created_at: "unix-ms:1".into(),
            },
            &auth,
            &placement(13),
        )
        .unwrap();
    let control_plane = actor(ActorKind::Service, "fabric-control-plane");
    central
        .store
        .apply_target_work_created(
            &context(
                control_plane.clone(),
                "target_work.applied",
                "artifact-target-work",
                2,
            ),
            &artifact_proposal.delegation_id,
            &work_ref("node-b", "team-b", "work-b", 1),
            &placement(13),
            "route-artifact-target-work",
            &control_plane,
        )
        .unwrap();
    let source = TestStore::new("artifact-grant-source");
    seed_team(
        &source.store,
        SOURCE_NODE_UUID,
        "Node A",
        "space-node-a",
        "project-a",
        "mission-a",
        "team-a",
        "Team A",
        "host-a",
        "run-a",
    );
    let manifest = RemoteArtifactManifest {
        id: "artifact-delegation-1".into(),
        company_id: "company-1".into(),
        source_node_id: "node-b".into(),
        source_team_id: Some("team-b".into()),
        source_work_id: Some("work-b".into()),
        operation_id: Some("publication-1".into()),
        media_type: "text/plain".into(),
        size_bytes: 8,
        sha256: "a".repeat(64),
        classification: ArtifactClassification::CompanyInternal,
        initiator: "host-b".into(),
        authorized_readers: BTreeSet::from(["host-a".into()]),
        created_by: "host-b".into(),
        created_at_unix_ms: 100,
        expires_at_unix_ms: None,
        completed_at_unix_ms: Some(101),
        deleted_at_unix_ms: None,
        revision: 3,
        schema_version: "agentfirm.remote-fabric.v1".into(),
    };
    let capability = ArtifactCapability {
        token: "signed-token".into(),
        company_id: "company-1".into(),
        node_id: SOURCE_NODE_UUID.into(),
        artifact_id: manifest.id.clone(),
        artifact_digest: manifest.sha256.clone(),
        purpose: ArtifactCapabilityPurpose::Download,
        issued_to: "host-a".into(),
        issued_at_unix_ms: 102,
        expires_at_unix_ms: 10_000,
        one_use: true,
    };
    let business = central
        .store
        .artifact_grant_operation(
            &context(
                actor(ActorKind::AgentMember, "host-b"),
                "artifact_grant",
                "artifact-grant-1",
                3,
            ),
            &artifact_proposal.delegation_id,
            &manifest,
            &capability,
        )
        .expect("target Host grants exact completed artifact to source Host");
    let route = route_collaboration_business_operation(
        &business,
        &CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "control-plane:3".into(),
                actor_kind: FabricActorKind::Service,
                role_bindings: BTreeSet::from(["company_control_plane".into()]),
                session_id: "control-plane:3".into(),
                issued_at_unix_ms: 102,
                expires_at_unix_ms: 10_000,
            },
            resolved_business_actor: actor(ActorKind::AgentMember, "host-b"),
            source: CollaborationFabricSource::ControlPlane,
            control_plane_generation: 3,
            target_execution_space_id: Some("space-node-a".into()),
            created_at_unix_ms: 102,
            expires_at_unix_ms: 5_000,
        },
    )
    .expect("artifact grant uses Wave5 route");
    let validated = apply_collaboration_target_operation(&source.store, &route, "unix-ms:103")
        .expect("source Node validates exact source Team Host capability");
    assert_eq!(
        validated.0,
        "agentfirm.collaboration.artifact_grant_validated.v1"
    );

    let before = source.store.collaboration_operations().unwrap();
    let mut hostile = route;
    hostile.body["payload"]["read_capability"]["issued_to"] = serde_json::json!("member-a");
    hostile.body_digest = json_digest(&hostile.body).unwrap();
    assert!(apply_collaboration_target_operation(&source.store, &hostile, "unix-ms:104").is_err());
    assert_eq!(source.store.collaboration_operations().unwrap(), before);

    let mut hostile_snapshot = hostile;
    hostile_snapshot.body["payload"]["read_capability"]["issued_to"] = serde_json::json!("host-a");
    hostile_snapshot.body["payload"]["delegation"]["source_node_id"] =
        serde_json::json!("node-hostile");
    hostile_snapshot.body_digest = json_digest(&hostile_snapshot.body).unwrap();
    assert!(
        apply_collaboration_target_operation(&source.store, &hostile_snapshot, "unix-ms:105")
            .is_err()
    );
    assert_eq!(source.store.collaboration_operations().unwrap(), before);
}
