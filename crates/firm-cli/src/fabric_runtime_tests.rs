
use super::*;

fn current_remote_fact_fixture() -> (
    harness_core::collaboration::SourceWorkAttestation,
    harness_core::collaboration::WorkDelegationV1,
    harness_core::collaboration::DelegationInboundPolicy,
    harness_core::collaboration::RemoteFactPublication,
    harness_fabric::RoutedOperation,
) {
    let mut attestation: harness_core::collaboration::SourceWorkAttestation =
            serde_json::from_str(include_str!(
                "../../../schemas/collaboration/fixtures/source-work-attestation/valid/server-authored.json"
            ))
            .unwrap();
    let mut delegation: harness_core::collaboration::WorkDelegationV1 =
        serde_json::from_str(include_str!(
            "../../../schemas/collaboration/fixtures/work-delegation-v1/valid/awaiting.json"
        ))
        .unwrap();
    let mut policy: harness_core::collaboration::DelegationInboundPolicy =
            serde_json::from_str(include_str!(
                "../../../schemas/collaboration/fixtures/delegation-inbound-policy/valid/host-approval.json"
            ))
            .unwrap();
    let publication: harness_core::collaboration::RemoteFactPublication =
        serde_json::from_str(include_str!(
            "../../../schemas/collaboration/fixtures/remote-fact-publication/valid/report.json"
        ))
        .unwrap();
    attestation.id = delegation.source_work_attestation_id.clone();
    attestation.source_work_ref = delegation.source_work_ref.clone();
    attestation.source_owner_ref = delegation.source_owner_ref.clone();
    policy.revision = 3;
    delegation.inbound_policy_snapshot.policy_revision = policy.revision;
    delegation.inbound_policy_snapshot.policy_digest =
        harness_store::canonical_json_fingerprint(&serde_json::json!({
            "policy_id": policy.id,
            "policy_revision": policy.revision,
            "mode": policy.mode,
            "allowed_outcome_classes": policy.allowed_outcome_classes,
            "max_active_delegations": policy.max_active_delegations,
        }));
    delegation.state = harness_core::collaboration::DelegationState::Active;
    delegation.revision = 3;
    delegation.target_work_ref = Some(publication.fact_work_ref.clone());
    let payload = serde_json::json!({
        "publication": publication,
        "source_team_placement": {
            "team_id": delegation.source_team_id,
            "team_revision": delegation.source_work_ref.team_revision,
            "node_id": delegation.source_node_id,
            "placement_generation": delegation.source_work_ref.placement_generation,
        },
    });
    let business = harness_core::collaboration::RoutedBusinessOperation {
        id: "route-publication-authority-fence".into(),
        protocol_version: "agentfirm.fabric.v1".into(),
        company_id: delegation.company_id.clone(),
        kind: harness_core::collaboration::RoutedBusinessKind::RemoteFactPublish,
        authenticated_actor: publication.created_by.clone(),
        source_node_id: publication.origin_node_id.clone(),
        target_placement: harness_core::collaboration::TargetPlacementRef {
            team_id: delegation.source_team_id.clone(),
            team_revision: delegation.source_work_ref.team_revision,
            node_id: delegation.source_node_id.clone(),
            placement_generation: delegation.source_work_ref.placement_generation,
        },
        expected_revision: delegation.revision,
        idempotency_key: "publish-authority-fence".into(),
        payload_digest: harness_store::canonical_json_fingerprint(&payload),
        payload,
        required_capability: harness_core::collaboration::RoutedBusinessKind::RemoteFactPublish
            .required_capability(),
        ordering_key: format!("delegation:{}", delegation.id),
        created_at: "unix-ms:10".into(),
    };
    let operation = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: AuthenticatedActor {
                company_id: delegation.company_id.clone(),
                actor_id: publication.origin_node_id.clone(),
                actor_kind: harness_fabric::ActorKind::Service,
                role_bindings: BTreeSet::from(["fabric_submit".into()]),
                session_id: "daemon-b:1".into(),
                issued_at_unix_ms: 10,
                expires_at_unix_ms: 1_000,
            },
            resolved_business_actor: publication.created_by.clone(),
            source: harness_store::CollaborationFabricSource::Node {
                source_execution_space_id: "space-b".into(),
                source_gateway_generation: 9,
                source_node_daemon_id: "daemon-b".into(),
                source_node_daemon_generation: 1,
            },
            control_plane_generation: 3,
            target_execution_space_id: Some("space-a".into()),
            created_at_unix_ms: 10,
            expires_at_unix_ms: 1_000,
        },
    )
    .unwrap();
    (attestation, delegation, policy, publication, operation)
}

fn seed_current_remote_fact_authority(
    root: &Path,
    attestation: &harness_core::collaboration::SourceWorkAttestation,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
) {
    std::fs::create_dir_all(root).unwrap();
    let actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: "fixture".into(),
    };
    let rows = [
        (
            "source_work_attestation",
            attestation.id.as_str(),
            serde_json::to_value(attestation).unwrap(),
        ),
        (
            "delegation_inbound_policy",
            policy.id.as_str(),
            serde_json::to_value(policy).unwrap(),
        ),
        (
            "work_delegation_v1",
            delegation.id.as_str(),
            serde_json::to_value(delegation).unwrap(),
        ),
    ];
    let body = rows
        .into_iter()
        .enumerate()
        .map(|(index, (kind, id, projection))| {
            serde_json::to_string(&harness_store::CollaborationOperation {
                store_version: harness_core::collaboration::COLLABORATION_STORE_VERSION.into(),
                company_id: delegation.company_id.clone(),
                command_name: "fixture".into(),
                authenticated_actor: actor.clone(),
                idempotency_key: format!("fixture-{index}"),
                request_fingerprint: format!("sha256:{:064x}", index + 1),
                aggregate_kind: kind.into(),
                aggregate_id: id.into(),
                store_sequence: (index + 1) as u64,
                resulting_revision: if kind == "work_delegation_v1" {
                    delegation.revision
                } else if kind == "delegation_inbound_policy" {
                    policy.revision
                } else {
                    1
                },
                resulting_projection: projection,
                immutable_side_records: Vec::new(),
                created_at: "unix-ms:1".into(),
            })
            .unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(root.join("agentfirm_collaboration_operations.jsonl"), body).unwrap();
}

#[path = "fabric_runtime_tests/artifact_complete_body_limit_matches_the_frozen_64_mib_contract_only.rs"]
mod artifact_complete_body_limit_matches_the_frozen_64_mib_contract_only;
#[path = "fabric_runtime_tests/artifact_grant_replay_uses_frozen_authority_before_mutable_business_state.rs"]
mod artifact_grant_replay_uses_frozen_authority_before_mutable_business_state;
#[path = "fabric_runtime_tests/artifact_retention_http_shape_rejects_caller_authored_terminal_times.rs"]
mod artifact_retention_http_shape_rejects_caller_authored_terminal_times;
#[path = "fabric_runtime_tests/collaboration_read_scope_is_closed_to_exact_participants.rs"]
mod collaboration_read_scope_is_closed_to_exact_participants;
#[path = "fabric_runtime_tests/concurrent_cancellation_cannot_cross_remote_fact_admission_fence.rs"]
mod concurrent_cancellation_cannot_cross_remote_fact_admission_fence;
#[path = "fabric_runtime_tests/concurrent_exact_artifact_grants_commit_one_route_capability_and_receipt.rs"]
mod concurrent_exact_artifact_grants_commit_one_route_capability_and_receipt;
#[path = "fabric_runtime_tests/control_plane_message_authority_rejects_every_non_active_or_widened_route.rs"]
mod control_plane_message_authority_rejects_every_non_active_or_widened_route;
#[path = "fabric_runtime_tests/current_artifact_grant_authority_rejects_revocation_and_cancellation.rs"]
mod current_artifact_grant_authority_rejects_revocation_and_cancellation;
#[path = "fabric_runtime_tests/delegation_proposal_admission_rejects_revoked_policy_before_fabric_commit.rs"]
mod delegation_proposal_admission_rejects_revoked_policy_before_fabric_commit;
#[path = "fabric_runtime_tests/execution_space_derives_the_exact_firm_home_without_escaping_to_user_home.rs"]
mod execution_space_derives_the_exact_firm_home_without_escaping_to_user_home;
#[path = "fabric_runtime_tests/host_rest_enrollment_uses_csr_possession_and_one_atomic_consumption.rs"]
mod host_rest_enrollment_uses_csr_possession_and_one_atomic_consumption;
#[path = "fabric_runtime_tests/host_rest_secret_and_mutation_shapes_fail_closed.rs"]
mod host_rest_secret_and_mutation_shapes_fail_closed;
#[path = "fabric_runtime_tests/macos_keychain_credentials_require_public_identity_before_acl_access.rs"]
mod macos_keychain_credentials_require_public_identity_before_acl_access;
#[path = "fabric_runtime_tests/real_gateway_frame_timeout_is_bounded_within_the_lease.rs"]
mod real_gateway_frame_timeout_is_bounded_within_the_lease;
#[path = "fabric_runtime_tests/remote_fact_admission_rejects_stale_or_revoked_authority_before_fabric_commit.rs"]
mod remote_fact_admission_rejects_stale_or_revoked_authority_before_fabric_commit;
#[path = "fabric_runtime_tests/route_queue_replay_uses_semantic_intent_not_regenerated_timestamps.rs"]
mod route_queue_replay_uses_semantic_intent_not_regenerated_timestamps;
#[cfg(target_os = "macos")]
#[path = "fabric_runtime_tests/source_work_attestation_identity_is_stable_per_proposal_not_per_work_revision.rs"]
mod source_work_attestation_identity_is_stable_per_proposal_not_per_work_revision;
