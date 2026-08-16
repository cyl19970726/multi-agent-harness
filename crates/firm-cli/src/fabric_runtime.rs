//! Process entrypoints for the Company Control Plane and outbound NodeGateway.

#![allow(clippy::result_large_err)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use harness_fabric::gateway_runtime::{
    now_unix_ms, serve_control_plane_session_with_application, ControlPlaneReceiptApplication,
    NodeApplication, NodeGatewayConnection, ProbeApplication,
};
use harness_fabric::transport::{
    accept_control_plane_mtls, ControlPlaneTlsFiles, NodeFabricConfig, NodeTlsIdentityFiles,
    NodeTlsIdentityMaterial,
};
use harness_fabric::{
    ArtifactClassification, AuthenticatedActor, ControlPlane, EffectCertainty, FabricError,
    FabricErrorCode, InMemoryArtifactKeyBackend, NodeAdministrativeStatus, NodeHello, ReceiptKind,
    RouteReceipt, RoutedOperation, TargetApplicationResult, COLLABORATION_BUSINESS_OPERATION_KIND,
    FABRIC_PROTOCOL_VERSION,
};
use harness_store::remote_fabric_store::RemoteFabricStoreLayout;
use harness_store::HarnessStore;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::{CliError, CliResult, ResolvedStore};

const GATEWAY_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(5);

struct Wave6ControlPlaneApplication {
    collaboration_root: PathBuf,
    company_id: String,
    actor_id: String,
}

fn current_inbound_policy_matches_delegation(
    company_id: &str,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
) -> bool {
    let policy_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "policy_id": policy.id,
        "policy_revision": policy.revision,
        "mode": policy.mode,
        "allowed_outcome_classes": policy.allowed_outcome_classes,
        "max_active_delegations": policy.max_active_delegations,
    }));
    policy.revoked_at.is_none()
        && policy.id == delegation.inbound_policy_snapshot.policy_id
        && policy.company_id == company_id
        && policy.target_team_id == delegation.target_placement.team_id
        && policy.source_team_id == delegation.source_team_id
        && policy.created_by_target_host == delegation.target_host_ref
        && policy.revision == delegation.inbound_policy_snapshot.policy_revision
        && policy_digest == delegation.inbound_policy_snapshot.policy_digest
}

fn validate_current_remote_fact_authority(
    company_id: &str,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
    publication: &harness_core::collaboration::RemoteFactPublication,
    reference: &harness_fabric::CollaborationBusinessReference,
    operation: &RoutedOperation,
) -> Result<(), FabricError> {
    let business_actor = harness_core::agentfirm_api::ActorRef {
        kind: match reference.business_actor_kind.as_str() {
            "human" => harness_core::agentfirm_api::ActorKind::Human,
            "agent_member" => harness_core::agentfirm_api::ActorKind::AgentMember,
            "service" => harness_core::agentfirm_api::ActorKind::Service,
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::UnauthorizedActor,
                    "remote fact business actor kind is not allowed",
                ))
            }
        },
        id: reference.business_actor_id.clone(),
    };
    if !matches!(
        delegation.state,
        harness_core::collaboration::DelegationState::Active
            | harness_core::collaboration::DelegationState::ResultAvailable
    ) || delegation.revision != reference.expected_revision
        || !current_inbound_policy_matches_delegation(company_id, delegation, policy)
        || publication.company_id != company_id
        || publication.delegation_id != delegation.id
        || publication.created_by != business_actor
        || publication.origin_node_id != operation.source_node_id.clone().unwrap_or_default()
        || publication.origin_node_id != delegation.target_placement.node_id
        || publication.origin_team_id != delegation.target_placement.team_id
        || delegation.target_work_ref.as_ref() != Some(&publication.fact_work_ref)
        || publication.delegation_source_work_ref != delegation.source_work_ref
        || reference.target_team_id != delegation.source_team_id
        || reference.target_team_revision != delegation.source_work_ref.team_revision
        || reference.placement_generation != delegation.source_work_ref.placement_generation
    {
        return Err(FabricError::none(
            FabricErrorCode::ExpectedRevisionConflict,
            "remote fact route disagrees with the current Delegation, inbound policy, Work, placement, or actor",
        ));
    }
    Ok(())
}

fn validate_current_delegation_proposal_authority(
    company_id: &str,
    store: &HarnessStore,
    request: &harness_store::ProposeDelegationRequest,
    attestation: &harness_core::collaboration::SourceWorkAttestation,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
    reference: &harness_fabric::CollaborationBusinessReference,
    operation: &RoutedOperation,
) -> Result<(), FabricError> {
    let attestation_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "id": attestation.id,
        "company_id": attestation.company_id,
        "source_work_ref": attestation.source_work_ref,
        "source_owner_ref": attestation.source_owner_ref,
        "source_host_ref": attestation.source_host_ref,
        "work_application_service_ref": attestation.work_application_service_ref,
        "source_gateway_generation": attestation.source_gateway_generation,
        "issued_at": attestation.issued_at,
    }));
    let exact_actor = reference.business_actor_kind == "agent_member"
        && (reference.business_actor_id == attestation.source_owner_ref.id
            || reference.business_actor_id == attestation.source_host_ref.id);
    let active_count = store
        .collaboration_delegations(company_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .filter(|delegation| {
            delegation.source_team_id == attestation.source_work_ref.team_id
                && delegation.target_placement.team_id == request.target_placement.team_id
                && delegation.state != harness_core::collaboration::DelegationState::Terminal
        })
        .count() as u64;
    if reference.expected_revision != 0
        || request.source_work_attestation_id != attestation.id
        || attestation.company_id != company_id
        || attestation.attestation_digest != attestation_digest
        || operation.source_node_id.as_deref() != Some(attestation.source_work_ref.node_id.as_str())
        || request.target_placement.team_id != reference.target_team_id
        || request.target_placement.team_revision != reference.target_team_revision
        || request.target_placement.placement_generation != reference.placement_generation
        || request.target_placement.node_id != operation.target_node_id
        || policy.company_id != company_id
        || policy.source_team_id != attestation.source_work_ref.team_id
        || policy.target_team_id != request.target_placement.team_id
        || policy.revoked_at.is_some()
        || !policy
            .allowed_outcome_classes
            .iter()
            .any(|class| class == &request.outcome_class)
        || active_count >= policy.max_active_delegations
        || !exact_actor
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "delegation proposal disagrees with the current inbound policy, source Work attestation, placement, actor, or active limit",
        ));
    }
    Ok(())
}

fn validate_current_artifact_grant_authority(
    company_id: &str,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    attestation: &harness_core::collaboration::SourceWorkAttestation,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
    manifest: &harness_fabric::RemoteArtifactManifest,
    actor: &harness_core::agentfirm_api::ActorRef,
    expected_revision: u64,
) -> Result<(), FabricError> {
    if !matches!(
        delegation.state,
        harness_core::collaboration::DelegationState::Active
            | harness_core::collaboration::DelegationState::ResultAvailable
    ) || delegation.revision != expected_revision
        || !current_inbound_policy_matches_delegation(company_id, delegation, policy)
        || actor != &delegation.target_host_ref
        || attestation.id != delegation.source_work_attestation_id
        || attestation.company_id != company_id
        || attestation.source_work_ref != delegation.source_work_ref
        || manifest.company_id != company_id
        || manifest.source_node_id != delegation.target_placement.node_id
        || manifest.source_team_id.as_deref() != Some(delegation.target_placement.team_id.as_str())
        || manifest.source_work_id.as_deref()
            != delegation
                .target_work_ref
                .as_ref()
                .map(|work| work.work_id.as_str())
        || manifest.completed_at_unix_ms.is_none()
        || manifest.deleted_at_unix_ms.is_some()
        || !manifest
            .authorized_readers
            .contains(&attestation.source_host_ref.id)
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "artifact grant disagrees with the current Delegation, inbound policy, Work, placement, or target Host",
        ));
    }
    Ok(())
}

fn validate_current_collaboration_message_authority(
    company_id: &str,
    delegation: &harness_core::collaboration::WorkDelegationV1,
    attestation: &harness_core::collaboration::SourceWorkAttestation,
    policy: &harness_core::collaboration::DelegationInboundPolicy,
    authority: &harness_core::collaboration::CollaborationMessageAuthority,
    reference: &harness_fabric::CollaborationBusinessReference,
) -> Result<(), FabricError> {
    let expected_authority_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "company_id": authority.company_id,
        "delegation_id": authority.delegation_id,
        "delegation_revision": authority.delegation_revision,
        "source_work_ref": authority.source_work_ref,
        "target_work_ref": authority.target_work_ref,
        "target_placement": authority.target_placement,
        "source_owner_ref": authority.source_owner_ref,
        "source_host_ref": authority.source_host_ref,
        "target_host_ref": authority.target_host_ref,
        "inbound_policy_snapshot": authority.inbound_policy_snapshot,
    }));
    let exact_actor = reference.business_actor_kind == "agent_member"
        && (reference.business_actor_id == attestation.source_owner_ref.id
            || reference.business_actor_id == attestation.source_host_ref.id);
    let policy_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "policy_id": policy.id,
        "policy_revision": policy.revision,
        "mode": policy.mode,
        "allowed_outcome_classes": policy.allowed_outcome_classes,
        "max_active_delegations": policy.max_active_delegations,
    }));
    if delegation.state != harness_core::collaboration::DelegationState::Active
        || delegation.revision != reference.expected_revision
        || authority.company_id != company_id
        || authority.delegation_id != delegation.id
        || authority.delegation_revision != delegation.revision
        || authority.source_work_ref != delegation.source_work_ref
        || delegation.target_work_ref.as_ref() != Some(&authority.target_work_ref)
        || authority.target_placement != delegation.target_placement
        || attestation.id != delegation.source_work_attestation_id
        || attestation.company_id != company_id
        || attestation.source_work_ref != delegation.source_work_ref
        || attestation.source_owner_ref != delegation.source_owner_ref
        || authority.source_owner_ref != attestation.source_owner_ref
        || authority.source_host_ref != attestation.source_host_ref
        || authority.target_host_ref != delegation.target_host_ref
        || authority.inbound_policy_snapshot != delegation.inbound_policy_snapshot
        || policy.revoked_at.is_some()
        || policy.id != delegation.inbound_policy_snapshot.policy_id
        || policy.company_id != company_id
        || policy.target_team_id != delegation.target_placement.team_id
        || policy.source_team_id != delegation.source_team_id
        || policy.created_by_target_host != delegation.target_host_ref
        || policy.revision != delegation.inbound_policy_snapshot.policy_revision
        || policy_digest != delegation.inbound_policy_snapshot.policy_digest
        || authority.authority_digest != expected_authority_digest
        || reference.target_team_id != delegation.target_placement.team_id
        || reference.target_team_revision != delegation.target_placement.team_revision
        || reference.placement_generation != delegation.target_placement.placement_generation
        || !exact_actor
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "team Message route disagrees with the current accepted Delegation, Work, placement, inbound policy, or source actor",
        ));
    }
    Ok(())
}

impl ControlPlaneReceiptApplication for Wave6ControlPlaneApplication {
    fn admit_and_accept_source(
        &self,
        operation: &RoutedOperation,
        _authenticated_actor: &AuthenticatedActor,
        accept: &mut dyn FnMut() -> Result<
            harness_fabric::gateway_runtime::AcceptedSourceOperation,
            FabricError,
        >,
    ) -> Result<harness_fabric::gateway_runtime::AcceptedSourceOperation, FabricError> {
        if operation.kind != COLLABORATION_BUSINESS_OPERATION_KIND {
            return accept();
        }
        let reference = match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => reference,
            _ => return accept(),
        };
        if reference.business_kind == "delegation_propose" {
            let request = serde_json::from_value::<harness_store::ProposeDelegationRequest>(
                reference.payload.get("request").cloned().ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        "delegation proposal route lacks its frozen request",
                    )
                })?,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
            let attestation =
                serde_json::from_value::<harness_core::collaboration::SourceWorkAttestation>(
                    reference
                        .payload
                        .get("source_work_attestation")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::InvalidPayload,
                                "delegation proposal route lacks source Work attestation",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
                })?;
            let policy_id = reference
                .payload
                .get("policy_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        "delegation proposal route lacks policy_id",
                    )
                })?;
            let store = HarnessStore::new(&self.collaboration_root);
            return store.with_collaboration_authority_fence(
                |locked_store| {
                    let policy = locked_store
                        .collaboration_inbound_policy(&self.company_id, policy_id)
                        .map_err(|error| {
                            FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "delegation proposal inbound policy is missing",
                            )
                        })?;
                    validate_current_delegation_proposal_authority(
                        &self.company_id,
                        locked_store,
                        &request,
                        &attestation,
                        &policy,
                        &reference,
                        operation,
                    )
                },
                accept,
            );
        }
        if reference.business_kind == "team_message_deliver" {
            let authority = serde_json::from_value::<
                harness_core::collaboration::CollaborationMessageAuthority,
            >(
                reference
                    .payload
                    .get("delegation_authority")
                    .cloned()
                    .ok_or_else(|| {
                        FabricError::none(
                            FabricErrorCode::UnauthorizedActor,
                            "team Message route lacks frozen Delegation authority",
                        )
                    })?,
            )
            .map_err(|error| {
                FabricError::none(
                    FabricErrorCode::UnauthorizedActor,
                    format!("team Message Delegation authority is invalid: {error}"),
                )
            })?;
            let store = HarnessStore::new(&self.collaboration_root);
            return store.with_collaboration_authority_fence(
                |locked_store| {
                    let delegation = locked_store
                        .collaboration_delegation(&self.company_id, &authority.delegation_id)
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "team Message references no central Delegation",
                            )
                        })?;
                    let attestation = locked_store
                        .collaboration_source_work_attestation(
                            &self.company_id,
                            &delegation.source_work_attestation_id,
                        )
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "team Message Delegation lacks source Work attestation",
                            )
                        })?;
                    let policy = locked_store
                        .collaboration_inbound_policy(
                            &self.company_id,
                            &delegation.inbound_policy_snapshot.policy_id,
                        )
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .filter(|policy| policy.revoked_at.is_none())
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "team Message inbound policy is missing or revoked",
                            )
                        })?;
                    validate_current_collaboration_message_authority(
                        &self.company_id,
                        &delegation,
                        &attestation,
                        &policy,
                        &authority,
                        &reference,
                    )
                },
                accept,
            );
        }
        if reference.business_kind == "remote_fact_publish" {
            let publication =
                serde_json::from_value::<harness_core::collaboration::RemoteFactPublication>(
                    reference
                        .payload
                        .get("publication")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::InvalidPayload,
                                "remote fact route lacks the immutable publication",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        format!("remote fact publication is invalid: {error}"),
                    )
                })?;
            let store = HarnessStore::new(&self.collaboration_root);
            return store.with_collaboration_authority_fence(
                |locked_store| {
                    let delegation = locked_store
                        .collaboration_delegation(&self.company_id, &publication.delegation_id)
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::ExpectedRevisionConflict,
                                "remote fact references no central Delegation",
                            )
                        })?;
                    let policy = locked_store
                        .collaboration_inbound_policy(
                            &self.company_id,
                            &delegation.inbound_policy_snapshot.policy_id,
                        )
                        .map_err(|error| {
                            FabricError::unknown(operation.id.clone(), error.to_string())
                        })?
                        .ok_or_else(|| {
                            FabricError::none(
                                FabricErrorCode::UnauthorizedActor,
                                "remote fact inbound policy is missing",
                            )
                        })?;
                    validate_current_remote_fact_authority(
                        &self.company_id,
                        &delegation,
                        &policy,
                        &publication,
                        &reference,
                        operation,
                    )
                },
                accept,
            );
        }
        accept()
    }

    fn fold_source_accepted(
        &self,
        operation: &RoutedOperation,
        receipt: &RouteReceipt,
        observed_at_unix_ms: u64,
    ) -> Result<Vec<harness_fabric::gateway_runtime::ControlPlaneFollowUp>, FabricError> {
        if operation.kind != COLLABORATION_BUSINESS_OPERATION_KIND
            || receipt.kind != ReceiptKind::ControlPlaneAccepted
        {
            return Ok(Vec::new());
        }
        let reference = match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => reference,
            _ => return Ok(Vec::new()),
        };
        if matches!(
            reference.business_kind.as_str(),
            "team_message_deliver" | "remote_fact_publish"
        ) {
            // Source authority was already resolved while the canonical
            // collaboration lock was held through the Fabric commit. Never
            // perform a second mutable-authority fold here: cancellation or a
            // policy revision immediately after that linearization point is a
            // later event, not grounds to reject an already accepted route or
            // strand a committed attempt without its response.
            if reference.business_kind == "team_message_deliver" {
                return Ok(Vec::new());
            }
        } else {
            return Ok(Vec::new());
        }
        let publication = serde_json::from_value::<
            harness_core::collaboration::RemoteFactPublication,
        >(reference.payload.get("publication").cloned().ok_or_else(
            || {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "remote fact route lacks the immutable publication",
                )
            },
        )?)
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("remote fact publication is invalid: {error}"),
            )
        })?;
        let store = HarnessStore::new(&self.collaboration_root);
        let delegation = store
            .collaboration_delegation(&self.company_id, &publication.delegation_id)
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("remote fact Delegation lookup failed: {error}"),
                )
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "remote fact references no central Delegation",
                )
            })?;
        let business_actor = harness_core::agentfirm_api::ActorRef {
            kind: match reference.business_actor_kind.as_str() {
                "human" => harness_core::agentfirm_api::ActorKind::Human,
                "agent_member" => harness_core::agentfirm_api::ActorKind::AgentMember,
                "service" => harness_core::agentfirm_api::ActorKind::Service,
                _ => {
                    return Err(FabricError::none(
                        FabricErrorCode::UnauthorizedActor,
                        "remote fact business actor kind is not allowed",
                    ))
                }
            },
            id: reference.business_actor_id.clone(),
        };
        if publication.created_by != business_actor
            || publication.origin_node_id != operation.source_node_id.clone().unwrap_or_default()
            || publication.origin_team_id != delegation.target_placement.team_id
        {
            return Err(FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "remote fact changed actor, source Node/Team, or Delegation revision",
            ));
        }
        let context = harness_store::CollaborationMutationContext {
            company_id: self.company_id.clone(),
            authenticated_actor: business_actor.clone(),
            command_name: "remote_fact_publish".into(),
            idempotency_key: operation.idempotency_key.clone(),
            expected_revision: 0,
            occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
        };
        store
            .publish_remote_fact(
                &context,
                &publication,
                &[business_actor, delegation.target_host_ref.clone()],
                &delegation.target_placement,
            )
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("Control Plane remote fact fold failed: {error}"),
                )
            })?;
        if let Some(operational_decision) = publication.operational_decision_ref.as_ref() {
            let attestation = store
                .collaboration_source_work_attestation(
                    &self.company_id,
                    &delegation.source_work_attestation_id,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("source Work attestation lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "accepted remote result has no frozen source Work attestation",
                    )
                })?;
            let result_context = harness_store::CollaborationMutationContext {
                company_id: self.company_id.clone(),
                authenticated_actor: delegation.target_host_ref.clone(),
                command_name: "delegation.result_available".into(),
                idempotency_key: format!("{}:result-available", operation.idempotency_key),
                expected_revision: reference.expected_revision,
                occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
            };
            store
                .mark_delegation_result_available(
                    &result_context,
                    &delegation.id,
                    &publication.id,
                    operational_decision,
                    &harness_store::ResolvedCollaborationAuthority {
                        source_host: attestation.source_host_ref,
                        source_work_owner: attestation.source_owner_ref,
                        target_host: delegation.target_host_ref.clone(),
                        target_placement: delegation.target_placement.clone(),
                        source_work_application_service: attestation.work_application_service_ref,
                        source_gateway_generation: attestation.source_gateway_generation,
                    },
                    &delegation.target_placement,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("Control Plane result-available fold failed: {error}"),
                    )
                })?;
        }
        Ok(Vec::new())
    }

    fn fold_target_application(
        &self,
        operation: &RoutedOperation,
        result: &TargetApplicationResult,
        receipt: &RouteReceipt,
        observed_at_unix_ms: u64,
    ) -> Result<Vec<harness_fabric::gateway_runtime::ControlPlaneFollowUp>, FabricError> {
        if operation.kind != COLLABORATION_BUSINESS_OPERATION_KIND {
            return Ok(Vec::new());
        }
        if receipt.kind != ReceiptKind::OperationApplied
            || receipt.application_effect != Some(EffectCertainty::Applied)
            || result.effect != EffectCertainty::Applied
        {
            return Ok(Vec::new());
        }
        let reference = match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => reference,
            _ => return Ok(Vec::new()),
        };
        if reference.business_kind == "delegation_propose" {
            if result.result_schema != "agentfirm.collaboration.delegation_proposal_validated.v1" {
                return Err(FabricError::unknown(
                    operation.id.clone(),
                    "delegation proposal applied receipt has an unexpected result schema",
                ));
            }
            let request = serde_json::from_value::<harness_store::ProposeDelegationRequest>(
                reference.payload.get("request").cloned().ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "proposal route lacks the frozen request",
                    )
                })?,
            )
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("proposal request is invalid: {error}"),
                )
            })?;
            let attestation =
                serde_json::from_value::<harness_core::collaboration::SourceWorkAttestation>(
                    reference
                        .payload
                        .get("source_work_attestation")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::unknown(
                                operation.id.clone(),
                                "proposal route lacks source Work attestation",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("source Work attestation is invalid: {error}"),
                    )
                })?;
            let policy_id = reference
                .payload
                .get("policy_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::unknown(operation.id.clone(), "proposal route lacks policy_id")
                })?;
            let target_host = serde_json::from_value::<harness_core::agentfirm_api::ActorRef>(
                result
                    .result
                    .get("target_host_ref")
                    .cloned()
                    .ok_or_else(|| {
                        FabricError::unknown(
                            operation.id.clone(),
                            "proposal validation lacks target Host",
                        )
                    })?,
            )
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("target Host result is invalid: {error}"),
                )
            })?;
            let store = HarnessStore::new(&self.collaboration_root);
            let attestation_context = harness_store::CollaborationMutationContext {
                company_id: self.company_id.clone(),
                authenticated_actor: attestation.work_application_service_ref.clone(),
                command_name: "import_source_work_attestation".into(),
                idempotency_key: format!("attestation:{}", operation.id),
                expected_revision: 0,
                occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
            };
            store
                .put_source_work_attestation(
                    &attestation_context,
                    &attestation,
                    &attestation.work_application_service_ref,
                    operation.source_gateway_generation.unwrap_or_default(),
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("source Work attestation import failed: {error}"),
                    )
                })?;
            let policy = store
                .collaboration_inbound_policy(&self.company_id, policy_id)
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("target inbound policy lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::UnauthorizedActor,
                        "target inbound policy is not centrally registered",
                    )
                })?;
            let business_actor = harness_core::agentfirm_api::ActorRef {
                kind: match reference.business_actor_kind.as_str() {
                    "human" => harness_core::agentfirm_api::ActorKind::Human,
                    "agent_member" => harness_core::agentfirm_api::ActorKind::AgentMember,
                    "service" => harness_core::agentfirm_api::ActorKind::Service,
                    _ => {
                        return Err(FabricError::none(
                            FabricErrorCode::UnauthorizedActor,
                            "proposal business actor kind is not allowed",
                        ))
                    }
                },
                id: reference.business_actor_id.clone(),
            };
            let authority = harness_store::ResolvedCollaborationAuthority {
                source_host: attestation.source_host_ref.clone(),
                source_work_owner: attestation.source_owner_ref.clone(),
                target_host,
                target_placement: request.target_placement.clone(),
                source_work_application_service: attestation.work_application_service_ref.clone(),
                source_gateway_generation: operation.source_gateway_generation.unwrap_or_default(),
            };
            let context = harness_store::CollaborationMutationContext {
                company_id: self.company_id.clone(),
                authenticated_actor: business_actor,
                command_name: "delegation_propose".into(),
                idempotency_key: operation.idempotency_key.clone(),
                expected_revision: reference.expected_revision,
                occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
            };
            store
                .propose_collaboration_delegation(&context, &request, &authority, &policy)
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("Control Plane delegation fold failed: {error}"),
                    )
                })?;
            return Ok(Vec::new());
        }
        if reference.business_kind == "delegation_decide" {
            if result.result_schema != "agentfirm.collaboration.delegation_decision_validated.v1" {
                return Err(FabricError::unknown(
                    operation.id.clone(),
                    "delegation decision applied receipt has an unexpected result schema",
                ));
            }
            let decision =
                serde_json::from_value::<harness_core::collaboration::DelegationDecision>(
                    result.result.get("decision").cloned().ok_or_else(|| {
                        FabricError::unknown(
                            operation.id.clone(),
                            "delegation decision result lacks the frozen decision",
                        )
                    })?,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("delegation decision result is invalid: {error}"),
                    )
                })?;
            let observed =
                serde_json::from_value::<harness_core::collaboration::TargetPlacementRef>(
                    result
                        .result
                        .get("target_placement")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::unknown(
                                operation.id.clone(),
                                "delegation decision result lacks target placement",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("delegation decision placement is invalid: {error}"),
                    )
                })?;
            let store = HarnessStore::new(&self.collaboration_root);
            let delegation_id = result
                .result
                .get("delegation_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "delegation decision result lacks delegation_id",
                    )
                })?;
            let before = store
                .collaboration_delegation(&self.company_id, delegation_id)
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("delegation lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "delegation decision references no central relationship",
                    )
                })?;
            let business_actor = harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: reference.business_actor_id.clone(),
            };
            let authority = harness_store::ResolvedCollaborationAuthority {
                source_host: before.source_owner_ref.clone(),
                source_work_owner: before.source_owner_ref.clone(),
                target_host: before.target_host_ref.clone(),
                target_placement: before.target_placement.clone(),
                source_work_application_service: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: format!("source-work-service:{}", before.source_node_id),
                },
                source_gateway_generation: operation.source_gateway_generation.unwrap_or_default(),
            };
            let context = harness_store::CollaborationMutationContext {
                company_id: self.company_id.clone(),
                authenticated_actor: business_actor,
                command_name: "delegation_decide".into(),
                idempotency_key: operation.idempotency_key.clone(),
                expected_revision: reference.expected_revision,
                occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
            };
            let decided = store
                .decide_collaboration_delegation(
                    &context,
                    delegation_id,
                    &decision,
                    &authority,
                    &observed,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("Control Plane delegation decision fold failed: {error}"),
                    )
                })?;
            if decided.projection.state
                != harness_core::collaboration::DelegationState::ProvisioningTargetWork
            {
                return Ok(Vec::new());
            }
            let target_work = store
                .target_work_create_operation(
                    &self.company_id,
                    delegation_id,
                    &format!("unix-ms:{observed_at_unix_ms}"),
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("target Work follow-up build failed: {error}"),
                    )
                })?;
            let control_actor = AuthenticatedActor {
                company_id: self.company_id.clone(),
                actor_id: self.actor_id.clone(),
                actor_kind: harness_fabric::ActorKind::Service,
                role_bindings: BTreeSet::from([
                    "company_control_plane".into(),
                    "fabric_submit".into(),
                ]),
                session_id: format!("control-plane:{}", operation.control_plane_generation),
                issued_at_unix_ms: observed_at_unix_ms,
                expires_at_unix_ms: observed_at_unix_ms.saturating_add(30_000),
            };
            let routed = harness_store::route_collaboration_business_operation(
                &target_work,
                &harness_store::CollaborationFabricRouteContext {
                    authenticated_actor: control_actor.clone(),
                    resolved_business_actor: before.target_host_ref,
                    source: harness_store::CollaborationFabricSource::ControlPlane,
                    control_plane_generation: operation.control_plane_generation,
                    target_execution_space_id: operation.target_execution_space_id.clone(),
                    created_at_unix_ms: observed_at_unix_ms,
                    expires_at_unix_ms: observed_at_unix_ms.saturating_add(10 * 60 * 1000),
                },
            )?;
            return Ok(vec![
                harness_fabric::gateway_runtime::ControlPlaneFollowUp {
                    authenticated_actor: control_actor,
                    operation: routed,
                },
            ]);
        }
        if reference.business_kind == "delegation_cancel_decide" {
            if result.result_schema != "agentfirm.collaboration.cancellation_decision_validated.v1"
            {
                return Err(FabricError::unknown(
                    operation.id.clone(),
                    "cancellation decision applied receipt has an unexpected result schema",
                ));
            }
            let delegation_id = result
                .result
                .get("delegation_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision result lacks delegation_id",
                    )
                })?;
            let request_id = result
                .result
                .get("request_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision result lacks request_id",
                    )
                })?;
            let decision = serde_json::from_value::<
                harness_core::collaboration::DelegationCancellationDecision,
            >(result.result.get("decision").cloned().ok_or_else(
                || {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision result lacks decision",
                    )
                },
            )?)
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("cancellation decision result is invalid: {error}"),
                )
            })?;
            let observed =
                serde_json::from_value::<harness_core::collaboration::TargetPlacementRef>(
                    result
                        .result
                        .get("target_placement")
                        .cloned()
                        .ok_or_else(|| {
                            FabricError::unknown(
                                operation.id.clone(),
                                "cancellation decision result lacks target placement",
                            )
                        })?,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("cancellation placement is invalid: {error}"),
                    )
                })?;
            let store = HarnessStore::new(&self.collaboration_root);
            let delegation = store
                .collaboration_delegation(&self.company_id, delegation_id)
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("cancellation Delegation lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision references no Delegation",
                    )
                })?;
            let source_attestation = store
                .collaboration_source_work_attestation(
                    &self.company_id,
                    &delegation.source_work_attestation_id,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("source Host attestation lookup failed: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "cancellation decision has no source Host attestation",
                    )
                })?;
            let authority = harness_store::ResolvedCollaborationAuthority {
                source_host: source_attestation.source_host_ref,
                source_work_owner: delegation.source_owner_ref.clone(),
                target_host: delegation.target_host_ref.clone(),
                target_placement: delegation.target_placement.clone(),
                source_work_application_service: source_attestation.work_application_service_ref,
                source_gateway_generation: source_attestation.source_gateway_generation,
            };
            store
                .decide_delegation_cancellation(
                    &harness_store::CollaborationMutationContext {
                        company_id: self.company_id.clone(),
                        authenticated_actor: delegation.target_host_ref,
                        command_name: "delegation_cancel_decide".into(),
                        idempotency_key: operation.idempotency_key.clone(),
                        expected_revision: reference.expected_revision,
                        occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
                    },
                    delegation_id,
                    request_id,
                    &decision,
                    &authority,
                    &observed,
                )
                .map_err(|error| {
                    FabricError::unknown(
                        operation.id.clone(),
                        format!("Control Plane cancellation decision fold failed: {error}"),
                    )
                })?;
            return Ok(Vec::new());
        }
        if reference.business_kind == "artifact_grant" {
            if result.result_schema != "agentfirm.collaboration.artifact_imported.v1" {
                return Err(FabricError::unknown(
                    operation.id.clone(),
                    "artifact grant receipt is not a canonical source ArtifactImport",
                ));
            }
            let import = serde_json::from_value::<harness_core::collaboration::ArtifactImport>(
                result
                    .result
                    .get("artifact_import")
                    .cloned()
                    .ok_or_else(|| {
                        FabricError::unknown(
                            operation.id.clone(),
                            "artifact import result is absent",
                        )
                    })?,
            )
            .map_err(|error| FabricError::unknown(operation.id.clone(), error.to_string()))?;
            let store = HarnessStore::new(&self.collaboration_root);
            let control_actor = harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::Service,
                id: self.actor_id.clone(),
            };
            store
                .record_collaboration_artifact_import(
                    &harness_store::CollaborationMutationContext {
                        company_id: self.company_id.clone(),
                        authenticated_actor: control_actor.clone(),
                        command_name: "artifact_import.fold".into(),
                        idempotency_key: format!("artifact-import:{}", operation.idempotency_key),
                        expected_revision: 0,
                        occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
                    },
                    &import,
                    &operation.id,
                    &control_actor,
                )
                .map_err(|error| FabricError::unknown(operation.id.clone(), error.to_string()))?;
            return Ok(Vec::new());
        }
        if reference.business_kind != "target_work_create" {
            return Ok(Vec::new());
        }
        if result.result_schema != "agentfirm.collaboration.target_work_created.v1" {
            return Err(FabricError::unknown(
                operation.id.clone(),
                "target Work applied receipt has an unexpected result schema",
            ));
        }
        let target_work_ref = serde_json::from_value::<harness_core::collaboration::RemoteWorkRef>(
            result
                .result
                .get("target_work_ref")
                .cloned()
                .ok_or_else(|| {
                    FabricError::unknown(
                        operation.id.clone(),
                        "target Work applied receipt lacks target_work_ref",
                    )
                })?,
        )
        .map_err(|error| {
            FabricError::unknown(
                operation.id.clone(),
                format!("target Work reference is invalid: {error}"),
            )
        })?;
        let store = HarnessStore::new(&self.collaboration_root);
        let control_actor = harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::Service,
            id: self.actor_id.clone(),
        };
        let context = harness_store::CollaborationMutationContext {
            company_id: self.company_id.clone(),
            authenticated_actor: control_actor.clone(),
            command_name: "fold_target_work_created".into(),
            idempotency_key: format!("fold:{}", operation.id),
            expected_revision: reference.expected_revision,
            occurred_at: format!("unix-ms:{observed_at_unix_ms}"),
        };
        let observed = harness_core::collaboration::TargetPlacementRef {
            team_id: reference.target_team_id,
            team_revision: reference.target_team_revision,
            node_id: operation.target_node_id.clone(),
            placement_generation: reference.placement_generation,
        };
        store
            .apply_target_work_created(
                &context,
                result
                    .result
                    .get("target_work_ref")
                    .and_then(|_| {
                        operation
                            .idempotency_key
                            .strip_prefix("target-work-create:")
                    })
                    .ok_or_else(|| {
                        FabricError::unknown(
                            operation.id.clone(),
                            "target Work operation has no canonical delegation idempotency binding",
                        )
                    })?,
                &target_work_ref,
                &observed,
                &operation.id,
                &control_actor,
            )
            .map_err(|error| {
                FabricError::unknown(
                    operation.id.clone(),
                    format!("Control Plane collaboration fold failed: {error}"),
                )
            })?;
        Ok(Vec::new())
    }
}

fn remote_fabric_schema_bundle_digest() -> String {
    harness_fabric::sha256_hex(include_bytes!(
        "../../../schemas/remote-fabric/schema-bundle.v1.json"
    ))
}

pub(crate) fn fabric_command(
    store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(CliError::Usage(
            "fabric requires control-plane|node-gateway".into(),
        ));
    };
    match command {
        "control-plane" => control_plane_command(resolved, &args[1..]),
        "node-gateway" => node_gateway_command(store, resolved, &args[1..]),
        "route" => route_command(store, resolved, &args[1..]),
        other => Err(CliError::Usage(format!("unknown fabric command: {other}"))),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueueCollaborationProposalRequest {
    pub company_id: String,
    pub source_work_id: String,
    pub target_team_id: String,
    pub target_team_revision: u64,
    pub target_node_id: String,
    pub target_execution_space_id: String,
    pub policy_id: String,
    pub requested_outcome: String,
    pub outcome_class: String,
    pub acceptance_contract: String,
    pub expires_unix_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueueCollaborationMessageRequest {
    pub company_id: String,
    pub target_team_id: String,
    pub target_team_revision: u64,
    pub target_node_id: String,
    pub target_execution_space_id: String,
    pub expected_delegation_revision: u64,
    pub expires_unix_ms: u64,
}

pub(crate) fn resolve_collaboration_message_authority(
    store: &HarnessStore,
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    credential: &super::AgentFirmHttpCredential,
    draft: &harness_core::agentfirm_api::MessageDraft,
    request: &QueueCollaborationMessageRequest,
) -> Result<harness_core::collaboration::CollaborationMessageAuthority, FabricError> {
    use harness_core::agentfirm_api::WorkExecutionBindingStatus;
    use harness_core::collaboration::{CollaborationMessageAuthority, DelegationState};

    let scope = draft.collaboration_scope.as_ref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "cross-node Message requires server-resolvable CollaborationScope",
        )
    })?;
    let delegation_id = scope.delegation_id.as_deref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "cross-node Message requires an exact central Delegation id",
        )
    })?;
    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let collaboration = layout.open_collaboration_store(&request.company_id)?;
    let delegation = collaboration
        .collaboration_delegation(&request.company_id, delegation_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "cross-node Message references no central Delegation",
            )
        })?;
    let target_work_ref = delegation.target_work_ref.clone().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "cross-node Message requires an accepted Delegation with target Work",
        )
    })?;
    let canonical_policy = collaboration
        .collaboration_inbound_policy(
            &request.company_id,
            &delegation.inbound_policy_snapshot.policy_id,
        )
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "Delegation inbound policy is not centrally available",
            )
        })?;
    let policy_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "policy_id": canonical_policy.id,
        "policy_revision": canonical_policy.revision,
        "mode": canonical_policy.mode,
        "allowed_outcome_classes": canonical_policy.allowed_outcome_classes,
        "max_active_delegations": canonical_policy.max_active_delegations,
    }));
    let source_work = store
        .latest_works()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .find(|work| work.id == delegation.source_work_ref.work_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "Delegation source Work is not current in this Execution Space",
            )
        })?;
    let source_team_id = source_work.team_id.as_deref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "Delegation source Work is not Team-bound",
        )
    })?;
    let source_teams = store
        .teams()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?;
    let source_team = source_teams
        .iter()
        .rev()
        .find(|team| team.id == source_team_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "Delegation source Team is not current",
            )
        })?;
    let source_team_revision = source_teams
        .iter()
        .filter(|team| team.id == source_team_id)
        .count() as u64;
    let (source_work_projection, source_work_event_id) = exact_work_projection_at_revision(
        store,
        execution_space_id,
        &source_work.id,
        source_work.version,
    )?;
    let current_source_ref = harness_core::collaboration::RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: execution_space_id.into(),
        node_id: local_node_id.into(),
        team_id: source_team.id.clone(),
        team_revision: source_team_revision,
        placement_generation: 1,
        work_id: source_work.id.clone(),
        work_revision: source_work.version,
        work_event_id: source_work_event_id,
        digest: harness_store::canonical_json_fingerprint(
            &serde_json::to_value(&source_work_projection).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?,
        ),
    };
    let source_sessions = store
        .fabric_agent_sessions(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?;
    let active_binding = store
        .fabric_work_execution_bindings(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .filter(|binding| {
            binding.work_id == source_work.id
                && binding.work_revision == source_work.version
                && binding.team_id == source_team.id
                && binding.agent_member_id == delegation.source_owner_ref.id
                && binding.status == WorkExecutionBindingStatus::Active
                && source_sessions.iter().any(|session| {
                    session.id == binding.agent_session_id
                        && session.agent_member_id == binding.agent_member_id
                        && session.runtime_generation == binding.agent_session_generation
                        && session.node_id == local_node_id
                        && session.lifecycle
                            != harness_core::agentfirm_api::AgentSessionStatus::Closed
                })
        })
        .collect::<Vec<_>>();
    let source_attestation = collaboration
        .collaboration_source_work_attestation(
            &request.company_id,
            &delegation.source_work_attestation_id,
        )
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "source Work attestation is missing",
            )
        })?;
    let actor_authorized = credential.actor == source_attestation.source_host_ref
        || credential.actor == delegation.source_owner_ref;
    if delegation.company_id != request.company_id
        || delegation.state != DelegationState::Active
        || delegation.revision != request.expected_delegation_revision
        || delegation.source_team_id != source_team.id
        || delegation.source_node_id != local_node_id
        || delegation.source_work_ref != current_source_ref
        || source_team.node_id != local_node_id
        || request.target_team_id != delegation.target_placement.team_id
        || request.target_team_revision != delegation.target_placement.team_revision
        || request.target_node_id != delegation.target_placement.node_id
        || request.target_execution_space_id != target_work_ref.execution_space_id
        || canonical_policy.revoked_at.is_some()
        || canonical_policy.target_team_id != delegation.target_placement.team_id
        || canonical_policy.source_team_id != delegation.source_team_id
        || canonical_policy.created_by_target_host != delegation.target_host_ref
        || policy_digest != delegation.inbound_policy_snapshot.policy_digest
        || scope.source_team_id != delegation.source_team_id
        || scope.target_team_id != delegation.target_placement.team_id
        || scope.expected_delegation_revision != Some(delegation.revision)
        || scope.source_work_ref.as_ref() != Some(&current_source_ref)
        || scope.target_work_ref.as_ref() != Some(&target_work_ref)
        || draft.team_id.as_deref() != Some(delegation.source_team_id.as_str())
        || draft.work_id.as_deref() != Some(source_work.id.as_str())
        || !actor_authorized
        || active_binding.len() != 1
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "cross-node Message is outside the exact accepted Delegation, current Work binding, policy, or placement authority",
        ));
    }
    let mut authority = CollaborationMessageAuthority {
        company_id: request.company_id.clone(),
        delegation_id: delegation.id,
        delegation_revision: delegation.revision,
        source_work_ref: current_source_ref,
        target_work_ref,
        target_placement: delegation.target_placement,
        source_owner_ref: delegation.source_owner_ref,
        source_host_ref: source_attestation.source_host_ref,
        target_host_ref: delegation.target_host_ref,
        inbound_policy_snapshot: delegation.inbound_policy_snapshot,
        authority_digest: String::new(),
    };
    authority.authority_digest = harness_store::canonical_json_fingerprint(&serde_json::json!({
        "company_id": authority.company_id,
        "delegation_id": authority.delegation_id,
        "delegation_revision": authority.delegation_revision,
        "source_work_ref": authority.source_work_ref,
        "target_work_ref": authority.target_work_ref,
        "target_placement": authority.target_placement,
        "source_owner_ref": authority.source_owner_ref,
        "source_host_ref": authority.source_host_ref,
        "target_host_ref": authority.target_host_ref,
        "inbound_policy_snapshot": authority.inbound_policy_snapshot,
    }));
    Ok(authority)
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueueRemoteFactPublicationRequest {
    pub company_id: String,
    pub delegation_id: String,
    pub fact_kind: harness_core::collaboration::RemoteFactKind,
    pub fact_id: String,
    pub source_work_ref: harness_core::collaboration::RemoteWorkRef,
    pub expires_unix_ms: u64,
    pub retain_until: String,
}

fn exact_work_projection_at_revision(
    store: &HarnessStore,
    execution_space_id: &str,
    work_id: &str,
    work_revision: u64,
) -> Result<(harness_core::Work, String), FabricError> {
    let projection_matches = |value: &serde_json::Value| {
        serde_json::from_value::<harness_core::Work>(value.clone())
            .ok()
            .filter(|work| work.id == work_id && work.version == work_revision)
    };
    for operation in store
        .canonical_operations_for_space(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .rev()
    {
        if let Some(work) = projection_matches(&operation.resulting_projection) {
            return Ok((work, operation.event.id));
        }
        for record in operation.immutable_side_records.into_iter().rev() {
            if let Some(work) = projection_matches(&record) {
                return Ok((work, operation.event.id));
            }
        }
    }
    for operation in store
        .work_operations()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .rev()
    {
        if operation.work.id == work_id && operation.work.version == work_revision {
            return Ok((operation.work, operation.event.id));
        }
    }
    Err(FabricError::none(
        FabricErrorCode::ExpectedRevisionConflict,
        "native fact does not bind an exact durable target Work revision",
    ))
}

fn accepted_work_decision_ref(
    store: &HarnessStore,
    execution_space_id: &str,
    work_id: &str,
    report_id: &str,
    work_ref: &harness_core::collaboration::RemoteWorkRef,
    target_host_id: &str,
) -> Result<Option<harness_core::collaboration::WorkOperationalDecisionRef>, FabricError> {
    let operation = store
        .canonical_operations_for_space(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .rev()
        .find(|operation| {
            operation.event.aggregate_kind == "work"
                && operation.event.aggregate_id == work_id
                && operation.event.transition == "accepted"
                && operation.event.performed_by_actor.kind
                    == harness_core::agentfirm_api::ActorKind::AgentMember
                && operation.event.performed_by_actor.id == target_host_id
                && operation
                    .event
                    .payload
                    .get("work_report_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(report_id)
        });
    let Some(operation) = operation else {
        return Ok(None);
    };
    let accepted_work =
        serde_json::from_value::<harness_core::Work>(operation.resulting_projection.clone())
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
    if accepted_work.version != work_ref.work_revision + 1
        || accepted_work.phase != harness_core::WorkPhase::Closed
        || accepted_work.resolution != Some(harness_core::WorkResolution::Accepted)
    {
        return Err(FabricError::none(
            FabricErrorCode::ExpectedRevisionConflict,
            "accepted target Work decision does not bind the published submitted revision",
        ));
    }
    Ok(Some(
        harness_core::collaboration::WorkOperationalDecisionRef {
            decision_id: operation.event.id.clone(),
            work_ref: work_ref.clone(),
            decision_revision: operation.event.resulting_version,
            digest: harness_store::canonical_json_fingerprint(
                &serde_json::to_value(&operation.event).map_err(|error| {
                    FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
                })?,
            ),
        },
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_collaboration_proposal(
    store: &HarnessStore,
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    credential: &super::AgentFirmHttpCredential,
    idempotency_key: &str,
    request: &QueueCollaborationProposalRequest,
    now_unix_ms: u64,
) -> Result<serde_json::Value, FabricError> {
    if idempotency_key.trim().is_empty()
        || request.company_id.trim().is_empty()
        || request.source_work_id.trim().is_empty()
        || request.target_execution_space_id.trim().is_empty()
        || request.target_team_revision == 0
        || request.expires_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "collaboration proposal requires bounded identity, exact target revision, idempotency, and future expiry",
        ));
    }
    let work = store
        .latest_works()
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("source Work lookup failed: {error}"),
            )
        })?
        .into_iter()
        .find(|work| work.id == request.source_work_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "source Work does not exist",
            )
        })?;
    let team_id = work.team_id.clone().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "source Work is not bound to a canonical AgentTeam",
        )
    })?;
    let teams = store.teams().map_err(|error| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            format!("source AgentTeam lookup failed: {error}"),
        )
    })?;
    let team_revision = teams.iter().filter(|team| team.id == team_id).count() as u64;
    let team = teams
        .iter()
        .rev()
        .find(|team| team.id == team_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "source AgentTeam does not exist",
            )
        })?;
    let actor_is_host = credential.actor.kind
        == harness_core::agentfirm_api::ActorKind::AgentMember
        && credential.actor.id == team.host_agent_id;
    let actor_is_owner = credential.actor.kind
        == harness_core::agentfirm_api::ActorKind::AgentMember
        && work.owner_member_id.as_deref() == Some(credential.actor.id.as_str());
    if !actor_is_host && !actor_is_owner {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "only the exact source Host or current Work owner may propose a Delegation",
        ));
    }
    if local_node_id.trim().is_empty()
        || team.node_id != local_node_id
        || request.target_node_id == local_node_id
    {
        return Err(FabricError::none(
            FabricErrorCode::TargetNotPlaced,
            "source Team must belong to this Node and target Team must be cross-node",
        ));
    }
    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let local = layout.open_node_local(&request.company_id, local_node_id)?;
    let session = local.active_session()?.ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "source Node has no current authenticated Fabric gateway session",
        )
    })?;
    let latest_event = store
        .work_events()
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("source Work event lookup failed: {error}"),
            )
        })?
        .into_iter()
        .rev()
        .find(|event| event.work_id == work.id && event.resulting_version == work.version)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "source Work has no exact current WorkEvent",
            )
        })?;
    let source_work_ref = harness_core::collaboration::RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: execution_space_id.into(),
        node_id: local_node_id.into(),
        team_id: team.id.clone(),
        team_revision,
        placement_generation: 1,
        work_id: work.id.clone(),
        work_revision: work.version,
        work_event_id: latest_event.id,
        digest: harness_store::canonical_json_fingerprint(&serde_json::to_value(&work).map_err(
            |error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()),
        )?),
    };
    let source_host = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::AgentMember,
        id: team.host_agent_id.clone(),
    };
    let source_owner = work
        .owner_member_id
        .as_ref()
        .map(|id| harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: id.clone(),
        })
        .unwrap_or_else(|| source_host.clone());
    let work_service = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Service,
        id: format!("work-application:{execution_space_id}"),
    };
    let mut attestation = harness_core::collaboration::SourceWorkAttestation {
        id: source_work_attestation_id(
            &work.id,
            work.version,
            session.gateway_generation,
            idempotency_key,
        ),
        company_id: request.company_id.clone(),
        source_work_ref,
        source_owner_ref: source_owner,
        source_host_ref: source_host,
        work_application_service_ref: work_service.clone(),
        source_gateway_generation: session.gateway_generation,
        attestation_digest: String::new(),
        issued_at: format!("unix-ms:{now_unix_ms}"),
    };
    attestation.attestation_digest =
        harness_store::canonical_json_fingerprint(&serde_json::json!({
            "id": attestation.id,
            "company_id": attestation.company_id,
            "source_work_ref": attestation.source_work_ref,
            "source_owner_ref": attestation.source_owner_ref,
            "source_host_ref": attestation.source_host_ref,
            "work_application_service_ref": attestation.work_application_service_ref,
            "source_gateway_generation": attestation.source_gateway_generation,
            "issued_at": attestation.issued_at,
        }));
    store
        .put_source_work_attestation(
            &harness_store::CollaborationMutationContext {
                company_id: request.company_id.clone(),
                authenticated_actor: work_service.clone(),
                command_name: "source_work_attest".into(),
                idempotency_key: format!("attestation:{idempotency_key}"),
                expected_revision: 0,
                occurred_at: format!("unix-ms:{now_unix_ms}"),
            },
            &attestation,
            &work_service,
            session.gateway_generation,
        )
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                format!("source Work attestation failed: {error}"),
            )
        })?;
    let target_placement = harness_core::collaboration::TargetPlacementRef {
        team_id: request.target_team_id.clone(),
        team_revision: request.target_team_revision,
        node_id: request.target_node_id.clone(),
        placement_generation: 1,
    };
    let proposal = harness_store::ProposeDelegationRequest {
        delegation_id: format!("delegation:{idempotency_key}"),
        source_work_attestation_id: attestation.id,
        target_placement,
        requested_outcome: request.requested_outcome.clone(),
        outcome_class: request.outcome_class.clone(),
        acceptance_contract: request.acceptance_contract.clone(),
        operation_id: format!("collaboration-propose:{idempotency_key}"),
    };
    let business = store
        .delegation_propose_operation(
            &harness_store::CollaborationMutationContext {
                company_id: request.company_id.clone(),
                authenticated_actor: credential.actor.clone(),
                command_name: "delegation_propose".into(),
                idempotency_key: idempotency_key.into(),
                expected_revision: 0,
                occurred_at: format!("unix-ms:{now_unix_ms}"),
            },
            &proposal,
            &request.policy_id,
        )
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("delegation proposal build failed: {error}"),
            )
        })?;
    let node_actor = AuthenticatedActor {
        company_id: request.company_id.clone(),
        actor_id: local_node_id.into(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: format!(
            "{}:{}",
            session.node_daemon_id, session.node_daemon_generation
        ),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: request.expires_unix_ms,
    };
    let routed = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: node_actor.clone(),
            resolved_business_actor: credential.actor.clone(),
            source: harness_store::CollaborationFabricSource::Node {
                source_execution_space_id: execution_space_id.into(),
                source_gateway_generation: session.gateway_generation,
                source_node_daemon_id: session.node_daemon_id.clone(),
                source_node_daemon_generation: session.node_daemon_generation,
            },
            control_plane_generation: session.control_plane_generation,
            target_execution_space_id: Some(request.target_execution_space_id.clone()),
            created_at_unix_ms: now_unix_ms,
            expires_at_unix_ms: request.expires_unix_ms,
        },
    )?;
    let (outbox, replayed) = local.prepare_outbox(&session, &node_actor, &routed, now_unix_ms)?;
    Ok(serde_json::json!({
        "delegation_id": proposal.delegation_id,
        "operation_id": routed.id,
        "outbox_state": outbox.local_state,
        "replayed": replayed,
    }))
}

fn source_work_attestation_id(
    work_id: &str,
    work_revision: u64,
    gateway_generation: u64,
    idempotency_key: &str,
) -> String {
    format!(
        "source-work-attestation:{work_id}:{work_revision}:{gateway_generation}:{idempotency_key}"
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_remote_fact_publication(
    store: &HarnessStore,
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    credential: &super::AgentFirmHttpCredential,
    idempotency_key: &str,
    expected_delegation_revision: u64,
    request: &QueueRemoteFactPublicationRequest,
    now_unix_ms: u64,
) -> Result<serde_json::Value, FabricError> {
    use harness_core::collaboration::{
        RemoteFactKind, RemoteFactPublication, RemoteFactSnapshot, RemoteWorkRef,
        TargetPlacementRef,
    };

    if idempotency_key.trim().is_empty()
        || expected_delegation_revision == 0
        || request.company_id.trim().is_empty()
        || request.delegation_id.trim().is_empty()
        || request.fact_id.trim().is_empty()
        || request.source_work_ref.execution_space_id.trim().is_empty()
        || request.source_work_ref.team_revision == 0
        || request.source_work_ref.placement_generation != 1
        || request.source_work_ref.node_id == local_node_id
        || request.expires_unix_ms <= now_unix_ms
        || request.retain_until.trim().is_empty()
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "remote fact publication requires exact Delegation/source placement, immutable fact identity, idempotency and future expiry",
        ));
    }

    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let local = layout.open_node_local(&request.company_id, local_node_id)?;
    let target_work_result_id = format!("route-target-work-{}", request.delegation_id);
    let target_work_result = local
        .snapshot()?
        .results
        .get(&target_work_result_id)
        .cloned()
        .filter(|result| {
            result.result_schema == "agentfirm.collaboration.target_work_created.v1"
                && result.effect == harness_fabric::EffectCertainty::Applied
        })
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "remote fact requires the applied target Work receipt for this Delegation",
            )
        })?;
    let relationship_target_work_ref = serde_json::from_value::<RemoteWorkRef>(
        target_work_result
            .result
            .get("target_work_ref")
            .cloned()
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "applied target Work receipt lacks target_work_ref",
                )
            })?,
    )
    .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;

    let (
        fact_work_id,
        fact_work_revision,
        fact_revision,
        result_report_id,
        created_by,
        summary,
        schema,
        fact,
        artifact_refs,
        evidence_refs,
    ) = match request.fact_kind {
        RemoteFactKind::Report => {
            let report = store
                .trust_work_reports(execution_space_id)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .into_iter()
                .find(|report| report.id == request.fact_id)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ExpectedRevisionConflict,
                        "native WorkReport does not exist",
                    )
                })?;
            let redacted = serde_json::json!({
                "kind": report.kind,
                "summary": report.summary,
                "candidate_fingerprint": report.candidate_fingerprint,
                "artifact_refs": report.artifact_refs,
                "evidence_refs": report.evidence_refs,
                "known_risks": report.known_risks,
                "confidence": report.confidence,
                "recommended_next_action": report.recommended_next_action,
            });
            let artifact_refs = report.artifact_refs.clone();
            let evidence_refs = report.evidence_refs.clone();
            (
                report.work_id,
                report.work_revision,
                report.report_revision,
                (report.kind == harness_core::agentfirm_api::WorkReportKind::Result)
                    .then_some(report.id.clone()),
                report.authored_by,
                redacted["summary"].as_str().unwrap_or_default().to_string(),
                "agentfirm.remote-fact.work-report.v1",
                redacted,
                artifact_refs,
                evidence_refs,
            )
        }
        RemoteFactKind::Finding => {
            let finding = store
                .trust_work_findings(execution_space_id)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .into_iter()
                .find(|finding| finding.id == request.fact_id)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ExpectedRevisionConflict,
                        "native WorkFinding does not exist",
                    )
                })?;
            let redacted = serde_json::json!({
                "kind": finding.kind,
                "summary": finding.summary,
                "affected_work_refs": finding.affected_work_refs,
                "reusable_asset_refs": finding.reusable_asset_refs,
                "invalidated_assumptions": finding.invalidated_assumptions,
                "evidence_refs": finding.evidence_refs,
                "confidence": finding.confidence,
            });
            (
                finding.work_id,
                finding.work_revision,
                1,
                None,
                finding.reported_by,
                redacted["summary"].as_str().unwrap_or_default().to_string(),
                "agentfirm.remote-fact.work-finding.v1",
                redacted,
                Vec::new(),
                finding.evidence_refs,
            )
        }
        RemoteFactKind::Failure => {
            let analysis = store
                .trust_failure_analyses(execution_space_id)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .into_iter()
                .find(|analysis| analysis.id == request.fact_id)
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ExpectedRevisionConflict,
                        "native FailureAnalysis does not exist",
                    )
                })?;
            let redacted = serde_json::json!({
                "observed_failure": analysis.observed_failure,
                "impact": analysis.impact,
                "primary_cause_status": analysis.primary_cause_status,
                "primary_cause": analysis.primary_cause,
                "retry_safety": analysis.retry_safety,
                "side_effect_summary": analysis.side_effect_summary,
                "recovery_options": analysis.recovery_options,
                "recommended_host_decision": analysis.recommended_host_decision,
                "evidence_refs": analysis.evidence_refs,
                "confidence": analysis.confidence,
            });
            (
                analysis.work_id,
                analysis.work_revision,
                1,
                None,
                analysis.reported_by,
                redacted["observed_failure"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                "agentfirm.remote-fact.failure-analysis.v1",
                redacted,
                Vec::new(),
                analysis.evidence_refs,
            )
        }
    };
    if created_by != credential.actor {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "credential is not the native fact author",
        ));
    }
    let current_work = store
        .latest_works()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .find(|work| work.id == fact_work_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "native fact target Work does not exist",
            )
        })?;
    let (work, work_event_id) = exact_work_projection_at_revision(
        store,
        execution_space_id,
        &fact_work_id,
        fact_work_revision,
    )?;
    let team_id = work.team_id.clone().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "native target Work is not Team-bound",
        )
    })?;
    let teams = store
        .teams()
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?;
    let team_revision = teams.iter().filter(|team| team.id == team_id).count() as u64;
    let team = teams
        .iter()
        .rev()
        .find(|team| team.id == team_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "target AgentTeam does not exist",
            )
        })?;
    let exact_active_binding = store
        .fabric_work_execution_bindings(execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .filter(|binding| {
            binding.work_id == current_work.id
                && binding.work_revision == current_work.version
                && binding.team_id == team.id
                && binding.agent_member_id == credential.actor.id
                && binding.status == harness_core::agentfirm_api::WorkExecutionBindingStatus::Active
        })
        .count()
        == 1;
    if current_work.team_id.as_deref() != Some(team.id.as_str())
        || current_work.version < work.version
        || team.node_id != local_node_id
        || current_work.owner_member_id.as_deref() != Some(credential.actor.id.as_str())
        || !exact_active_binding
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "remote fact requires the exact current target Work owner and active WorkExecutionBinding",
        ));
    }
    let native_fact_work_ref = RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: execution_space_id.into(),
        node_id: local_node_id.into(),
        team_id: team.id.clone(),
        team_revision,
        placement_generation: 1,
        work_id: work.id.clone(),
        work_revision: work.version,
        work_event_id,
        digest: harness_store::canonical_json_fingerprint(&serde_json::to_value(&work).map_err(
            |error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()),
        )?),
    };
    let operational_decision_ref = match result_report_id.as_deref() {
        Some(report_id) => accepted_work_decision_ref(
            store,
            execution_space_id,
            &work.id,
            report_id,
            &native_fact_work_ref,
            &team.host_agent_id,
        )?,
        None => None,
    };
    if current_work.version != work.version && operational_decision_ref.is_none() {
        return Err(FabricError::none(
            FabricErrorCode::ExpectedRevisionConflict,
            "superseded native fact requires the exact accepted result Work decision",
        ));
    }
    let publication_id = format!(
        "remote-fact:{}:{:?}:{}",
        request.delegation_id, request.fact_kind, request.fact_id
    )
    .to_ascii_lowercase();
    let fact_digest = harness_store::canonical_json_fingerprint(&fact);
    let publication = RemoteFactPublication {
        id: publication_id.clone(),
        company_id: request.company_id.clone(),
        delegation_id: request.delegation_id.clone(),
        origin_node_id: local_node_id.into(),
        origin_team_id: team.id.clone(),
        fact_work_ref: relationship_target_work_ref.clone(),
        native_fact_work_ref,
        delegation_source_work_ref: request.source_work_ref.clone(),
        fact_kind: request.fact_kind,
        fact_id: request.fact_id.clone(),
        fact_revision,
        fact_digest: fact_digest.clone(),
        summary,
        classification: "team".into(),
        snapshot: RemoteFactSnapshot {
            publication_id: publication_id.clone(),
            fact_schema: schema.into(),
            canonical_redacted_fact: fact,
            canonical_digest: fact_digest,
        },
        artifact_refs,
        evidence_refs,
        operational_decision_ref,
        created_by: credential.actor.clone(),
        created_at: format!("unix-ms:{now_unix_ms}"),
        retain_until: request.retain_until.clone(),
    };
    if publication.native_fact_work_ref.work_id != publication.fact_work_ref.work_id
        || publication.native_fact_work_ref.team_id != publication.fact_work_ref.team_id
        || publication.native_fact_work_ref.node_id != publication.fact_work_ref.node_id
        || publication.native_fact_work_ref.placement_generation
            != publication.fact_work_ref.placement_generation
        || publication.fact_work_ref != relationship_target_work_ref
        || publication.origin_team_id != relationship_target_work_ref.team_id
        || publication.origin_node_id != relationship_target_work_ref.node_id
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "remote fact disagrees with the exact locally applied target Work receipt",
        ));
    }
    let source_placement = TargetPlacementRef {
        team_id: request.source_work_ref.team_id.clone(),
        team_revision: request.source_work_ref.team_revision,
        node_id: request.source_work_ref.node_id.clone(),
        placement_generation: request.source_work_ref.placement_generation,
    };
    let business = store
        .remote_fact_publish_operation(
            &harness_store::CollaborationMutationContext {
                company_id: request.company_id.clone(),
                authenticated_actor: credential.actor.clone(),
                command_name: "remote_fact_publish".into(),
                idempotency_key: idempotency_key.into(),
                expected_revision: expected_delegation_revision,
                occurred_at: format!("unix-ms:{now_unix_ms}"),
            },
            &publication,
            &source_placement,
            local_node_id,
        )
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    let session = local.active_session()?.ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "target Node has no current authenticated Fabric gateway session",
        )
    })?;
    let node_actor = AuthenticatedActor {
        company_id: request.company_id.clone(),
        actor_id: local_node_id.into(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: format!(
            "{}:{}",
            session.node_daemon_id, session.node_daemon_generation
        ),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: request.expires_unix_ms,
    };
    let routed = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: node_actor.clone(),
            resolved_business_actor: credential.actor.clone(),
            source: harness_store::CollaborationFabricSource::Node {
                source_execution_space_id: execution_space_id.into(),
                source_gateway_generation: session.gateway_generation,
                source_node_daemon_id: session.node_daemon_id.clone(),
                source_node_daemon_generation: session.node_daemon_generation,
            },
            control_plane_generation: session.control_plane_generation,
            target_execution_space_id: Some(request.source_work_ref.execution_space_id.clone()),
            created_at_unix_ms: now_unix_ms,
            expires_at_unix_ms: request.expires_unix_ms,
        },
    )?;
    if let Some(existing) = local.snapshot()?.outboxes.get(&routed.id) {
        let existing_operation = existing.operation.as_ref().ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::RecoveryRequired,
                "remote fact replay retained no operation body and requires operator recovery",
            )
        })?;
        let reference = match existing_operation.closed_body()? {
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference)
                if reference.business_kind == "remote_fact_publish" =>
            {
                reference
            }
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "remote fact operation identity was reused by another business kind",
                ))
            }
        };
        let existing_publication = serde_json::from_value::<RemoteFactPublication>(
            reference
                .payload
                .get("publication")
                .cloned()
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::IdempotencyConflict,
                        "remote fact replay lacks its frozen publication",
                    )
                })?,
        )
        .map_err(|error| {
            FabricError::none(FabricErrorCode::IdempotencyConflict, error.to_string())
        })?;
        let mut semantic_publication = publication.clone();
        semantic_publication.created_at = existing_publication.created_at.clone();
        if semantic_publication != existing_publication
            || existing_operation.idempotency_key != idempotency_key
            || existing_operation.expected_target_revision != Some(expected_delegation_revision)
            || existing_operation.expires_at_unix_ms != request.expires_unix_ms
            || existing_operation.target_execution_space_id.as_deref()
                != Some(request.source_work_ref.execution_space_id.as_str())
        {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "remote fact replay changed its actor, fact, Work/placement, revision, retention, or expiry",
            ));
        }
        return Ok(serde_json::json!({
            "delegation_id": request.delegation_id,
            "publication": existing_publication,
            "operation_id": existing_operation.id,
            "outbox_state": existing.local_state,
            "replayed": true,
        }));
    }
    let (outbox, replayed) = local.prepare_outbox(&session, &node_actor, &routed, now_unix_ms)?;
    Ok(serde_json::json!({
        "delegation_id": request.delegation_id,
        "publication": publication,
        "operation_id": routed.id,
        "outbox_state": outbox.local_state,
        "replayed": replayed,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_collaboration_message(
    firm_home: &Path,
    execution_space_id: &str,
    local_node_id: &str,
    credential: &super::AgentFirmHttpCredential,
    idempotency_key: &str,
    message: &harness_core::agentfirm_api::Message,
    request: &QueueCollaborationMessageRequest,
    admission_authority: harness_core::collaboration::MessageAdmissionAuthority,
    now_unix_ms: u64,
) -> Result<serde_json::Value, FabricError> {
    let harness_core::collaboration::MessageAdmissionAuthority::WorkDelegation(authority) =
        admission_authority
    else {
        // DEV-35 exposes the generic source-authority handoff. The peer route
        // and its control-plane application remain owned by DEV-34.
        return Err(FabricError::none(
            FabricErrorCode::SchemaIncompatible,
            "peer-Team Message admission is ready for the peer_message_deliver route",
        ));
    };
    let scope = message.collaboration_scope.as_ref().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "cross-node Message requires CollaborationScope",
        )
    })?;
    if message.sender_actor_ref != credential.actor
        || message.source_execution_space_id != execution_space_id
        || message.source_node_id != local_node_id
        || message.idempotency_key != idempotency_key
        || request.target_node_id == local_node_id
        || request.target_team_revision == 0
        || request.expected_delegation_revision == 0
        || request.expires_unix_ms <= now_unix_ms
        || scope.target_team_id != request.target_team_id
        || scope.expected_delegation_revision != Some(request.expected_delegation_revision)
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "Message route disagrees with server-authored Message, exact sender, Delegation revision, or target",
        ));
    }
    let target_placement = harness_core::collaboration::TargetPlacementRef {
        team_id: request.target_team_id.clone(),
        team_revision: request.target_team_revision,
        node_id: request.target_node_id.clone(),
        placement_generation: 1,
    };
    let canonical_message_envelope = serde_json::to_value(message)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    let reference = harness_fabric::MessageReference {
        message_id: message.id.clone(),
        body_digest: message.body_digest.clone(),
        canonical_message_envelope: Some(canonical_message_envelope),
        message_object_ref: None,
    };
    let payload = serde_json::json!({
        "message_reference": reference,
        "delegation_authority": authority,
    });
    let business = harness_core::collaboration::RoutedBusinessOperation {
        id: format!("collaboration-message:{}", message.id),
        protocol_version: "agentfirm.fabric.v1".into(),
        company_id: request.company_id.clone(),
        kind: harness_core::collaboration::RoutedBusinessKind::TeamMessageDeliver,
        authenticated_actor: credential.actor.clone(),
        source_node_id: local_node_id.into(),
        target_placement,
        expected_revision: request.expected_delegation_revision,
        idempotency_key: format!("route:{idempotency_key}"),
        payload_digest: harness_store::canonical_json_fingerprint(&payload),
        payload,
        required_capability: harness_core::collaboration::RoutedBusinessKind::TeamMessageDeliver
            .required_capability(),
        ordering_key: format!(
            "delegation:{}",
            scope.delegation_id.as_deref().unwrap_or("host-to-host")
        ),
        created_at: format!("unix-ms:{now_unix_ms}"),
    };
    let layout = RemoteFabricStoreLayout::open(firm_home)?;
    let local = layout.open_node_local(&request.company_id, local_node_id)?;
    let session = local.active_session()?.ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "source Node has no current authenticated Fabric gateway session",
        )
    })?;
    if message.source_node_daemon_id != session.node_daemon_id
        || message.source_authority_generation != session.node_daemon_generation
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "Message was not authored by the exact current NodeDaemon generation",
        ));
    }
    let node_actor = AuthenticatedActor {
        company_id: request.company_id.clone(),
        actor_id: local_node_id.into(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: format!(
            "{}:{}",
            session.node_daemon_id, session.node_daemon_generation
        ),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: request.expires_unix_ms,
    };
    let routed = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: node_actor.clone(),
            resolved_business_actor: credential.actor.clone(),
            source: harness_store::CollaborationFabricSource::Node {
                source_execution_space_id: execution_space_id.into(),
                source_gateway_generation: session.gateway_generation,
                source_node_daemon_id: session.node_daemon_id.clone(),
                source_node_daemon_generation: session.node_daemon_generation,
            },
            control_plane_generation: session.control_plane_generation,
            target_execution_space_id: Some(request.target_execution_space_id.clone()),
            created_at_unix_ms: now_unix_ms,
            expires_at_unix_ms: request.expires_unix_ms,
        },
    )?;
    let (outbox, replayed) = local.prepare_outbox(&session, &node_actor, &routed, now_unix_ms)?;
    Ok(serde_json::json!({
        "message_id": message.id,
        "operation_id": routed.id,
        "outbox_state": outbox.local_state,
        "replayed": replayed,
    }))
}

fn route_command(
    wave4c_store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    if args.first().map(String::as_str) != Some("queue") {
        return Err(CliError::Usage(
            "fabric route queue --company <id> --target-node <uuid> --target-space <id> --kind probe|runtime|message --body-file <path> --operation-id <id> --idempotency-key <key> --ordering-key <key> [--source-space <id>]".into(),
        ));
    }
    let company_id = required(args, "--company")?;
    let target_node_id = required(args, "--target-node")?;
    let target_execution_space_id = required(args, "--target-space")?;
    let operation_id = required(args, "--operation-id")?;
    let idempotency_key = required(args, "--idempotency-key")?;
    let ordering_key = required(args, "--ordering-key")?;
    let kind = required(args, "--kind")?;
    let body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(required_path(args, "--body-file")?)?)?;
    let node_id = super::read_local_node_id()?;
    if target_node_id == node_id {
        return Err(CliError::Usage(
            "Remote Fabric route target must be a distinct ExecutionNode".into(),
        ));
    }
    let now = now_unix_ms().map_err(fabric_error)?;
    let firm_home = firm_home(resolved, args)?;
    let layout = RemoteFabricStoreLayout::open(&firm_home).map_err(fabric_error)?;
    let local = layout
        .open_node_local(&company_id, &node_id)
        .map_err(fabric_error)?;
    let session = local
        .active_session()
        .map_err(fabric_error)?
        .ok_or_else(|| {
            CliError::Usage(
                "NodeGateway has no durable active session; start the current NodeGateway first"
                    .into(),
            )
        })?;
    let lease = wave4c_store
        .latest_node_daemon_lease(&node_id)?
        .filter(|lease| {
            lease.status == harness_core::NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > now
                && lease.daemon_id == session.node_daemon_id
                && lease.generation == session.node_daemon_generation
        })
        .ok_or_else(|| {
            CliError::Usage(
                "NodeGateway session is not a child of the exact current NodeDaemonLease".into(),
            )
        })?;
    let (
        wire_kind,
        body_schema,
        source_execution_space_id,
        expected_target_revision,
        priority,
        expires_at_unix_ms,
    ) = match kind.as_str() {
        "probe" => {
            let probe: harness_fabric::FabricProbeBody = serde_json::from_value(body.clone())?;
            if probe.probe.trim().is_empty() {
                return Err(CliError::Usage(
                    "remote probe must contain a bounded non-empty probe label".into(),
                ));
            }
            (
                harness_fabric::PROBE_OPERATION_KIND,
                harness_fabric::PROBE_BODY_SCHEMA,
                value(args, "--source-space"),
                None,
                harness_fabric::OperationPriority::Control,
                now.saturating_add(5 * 60_000),
            )
        }
        "runtime" => {
            let reference: harness_fabric::RuntimeCommandReference =
                serde_json::from_value(body.clone())?;
            let intent = reference.canonical_command_intent.clone();
            if intent.target_execution_space_id != target_execution_space_id {
                return Err(CliError::Usage(
                    "remote RuntimeCommand intent must match the exact target Execution Space; target identity and capability are server-resolved"
                        .into(),
                ));
            }
            (
                harness_fabric::RUNTIME_COMMAND_REFERENCE_KIND,
                harness_fabric::RUNTIME_COMMAND_REFERENCE_SCHEMA,
                value(args, "--source-space"),
                Some(intent.expected_version),
                harness_fabric::OperationPriority::Control,
                intent.expires_unix_ms,
            )
        }
        "message" => {
            let reference: harness_fabric::MessageReference = serde_json::from_value(body.clone())?;
            let envelope = reference
                .canonical_message_envelope
                .as_ref()
                .ok_or_else(|| {
                    CliError::Usage(
                        "route queue currently requires an embedded canonical Message envelope"
                            .into(),
                    )
                })?;
            let message: harness_core::agentfirm_api::Message =
                serde_json::from_value(envelope.clone())?;
            if !wave4c_store
                .fabric_messages(&message.source_execution_space_id)?
                .iter()
                .any(|stored| stored == &message)
            {
                return Err(CliError::Usage(
                        "remote Message must already exist as the exact immutable source-authored Message"
                            .into(),
                    ));
            }
            (
                harness_fabric::MESSAGE_REFERENCE_KIND,
                harness_fabric::MESSAGE_REFERENCE_SCHEMA,
                Some(message.source_execution_space_id),
                Some(0),
                harness_fabric::OperationPriority::Normal,
                now.saturating_add(5 * 60_000),
            )
        }
        _ => {
            return Err(CliError::Usage(
                "--kind must be probe|runtime|message; arbitrary transport mutations are closed"
                    .into(),
            ))
        }
    };
    if let Some(existing) = local
        .snapshot()
        .map_err(fabric_error)?
        .outboxes
        .get(&operation_id)
        .cloned()
    {
        let existing_operation = existing.operation.as_ref().ok_or_else(|| {
            fabric_error(FabricError::none(
                FabricErrorCode::OperationUnknown,
                "durable source outbox lost its canonical operation envelope",
            ))
        })?;
        let capability = route_capability(&kind);
        let same_intent = route_queue_intent_matches(
            existing_operation,
            &company_id,
            &operation_id,
            wire_kind,
            &node_id,
            source_execution_space_id.as_deref(),
            &target_node_id,
            &target_execution_space_id,
            &idempotency_key,
            &ordering_key,
            expected_target_revision,
            body_schema,
            &body,
            capability,
        )
        .map_err(fabric_error)?;
        if !same_intent {
            return Err(fabric_error(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "route queue replay changed its durable semantic intent",
            )));
        }
        existing_operation.validate_digest().map_err(fabric_error)?;
        existing_operation.closed_body().map_err(fabric_error)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "queued_operation": existing,
                "replayed": true,
            }))?
        );
        return Ok(());
    }
    let actor = AuthenticatedActor {
        company_id: company_id.clone(),
        actor_id: node_id.clone(),
        actor_kind: harness_fabric::ActorKind::Service,
        role_bindings: std::collections::BTreeSet::from(["fabric_submit".into()]),
        session_id: format!("node-daemon:{}:{}", lease.daemon_id, lease.generation),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now.saturating_add(30_000),
    };
    let operation = harness_fabric::RoutedOperation {
        id: operation_id.clone(),
        company_id,
        kind: wire_kind.into(),
        source_authority: harness_fabric::OperationSourceAuthority::Node,
        source_node_id: Some(node_id),
        target_node_id,
        source_gateway_generation: Some(session.gateway_generation),
        source_node_daemon_id: Some(lease.daemon_id),
        source_node_daemon_generation: Some(lease.generation),
        control_plane_generation: session.control_plane_generation,
        source_execution_space_id,
        target_execution_space_id: Some(target_execution_space_id),
        actor: actor.clone(),
        actor_runtime_generation: None,
        authorization_context: std::collections::BTreeMap::from([(
            "capability".into(),
            route_capability(&kind).into(),
        )]),
        idempotency_key,
        ordering_key,
        correlation_id: format!("route:{operation_id}"),
        causation_id: None,
        expected_target_revision,
        body_schema: body_schema.into(),
        body_digest: harness_fabric::json_digest(&body).map_err(fabric_error)?,
        body,
        priority,
        created_at_unix_ms: now,
        expires_at_unix_ms,
        protocol_version: harness_fabric::FABRIC_PROTOCOL_VERSION,
        schema_version: harness_fabric::FABRIC_SCHEMA_VERSION.into(),
        canonicalization_version: harness_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
    };
    operation.closed_body().map_err(fabric_error)?;
    if kind == "runtime" {
        let intent = super::remote_fabric::runtime_intent_from_operation(&operation)
            .map_err(fabric_error)?;
        if intent.expires_unix_ms <= now {
            return Err(CliError::Usage(
                "remote RuntimeCommand expired before local durable queueing".into(),
            ));
        }
    } else if kind == "message" {
        super::remote_fabric::resolved_message_from_operation(&operation).map_err(fabric_error)?;
    }
    let (queued, replayed) = local
        .prepare_outbox(&session, &actor, &operation, now)
        .map_err(fabric_error)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "queued_operation": queued,
            "replayed": replayed,
        }))?
    );
    Ok(())
}

fn route_capability(kind: &str) -> &'static str {
    match kind {
        "probe" => "remote-probe",
        "runtime" => "remote-runtime",
        "message" => "remote-message",
        _ => unreachable!("route kind was closed before capability resolution"),
    }
}

#[allow(clippy::too_many_arguments)]
fn route_queue_intent_matches(
    existing: &harness_fabric::RoutedOperation,
    company_id: &str,
    operation_id: &str,
    wire_kind: &str,
    source_node_id: &str,
    source_execution_space_id: Option<&str>,
    target_node_id: &str,
    target_execution_space_id: &str,
    idempotency_key: &str,
    ordering_key: &str,
    expected_target_revision: Option<u64>,
    body_schema: &str,
    body: &serde_json::Value,
    capability: &str,
) -> Result<bool, FabricError> {
    Ok(existing.company_id == company_id
        && existing.id == operation_id
        && existing.kind == wire_kind
        && existing.source_authority == harness_fabric::OperationSourceAuthority::Node
        && existing.source_node_id.as_deref() == Some(source_node_id)
        && existing.source_execution_space_id.as_deref() == source_execution_space_id
        && existing.target_node_id == target_node_id
        && existing.target_execution_space_id.as_deref() == Some(target_execution_space_id)
        && existing.idempotency_key == idempotency_key
        && existing.ordering_key == ordering_key
        && existing.expected_target_revision == expected_target_revision
        && existing.body_schema == body_schema
        && existing.body == *body
        && existing.body_digest == harness_fabric::json_digest(body)?
        && existing
            .authorization_context
            .get("capability")
            .map(String::as_str)
            == Some(capability))
}

fn control_plane_command(resolved: &ResolvedStore, args: &[String]) -> CliResult<()> {
    match args.first().map(String::as_str) {
        Some("backup") => {
            let company_id = required(args, "--company")?;
            let output = required_path(args, "--output")?;
            let layout =
                RemoteFabricStoreLayout::open(firm_home(resolved, args)?).map_err(fabric_error)?;
            let manifest = layout
                .open_control_plane(&company_id)
                .map_err(fabric_error)?
                .create_backup(&output)
                .map_err(fabric_error)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            return Ok(());
        }
        Some("restore") => {
            let company_id = required(args, "--company")?;
            let backup = required_path(args, "--backup")?;
            let layout =
                RemoteFabricStoreLayout::open(firm_home(resolved, args)?).map_err(fabric_error)?;
            let target = layout
                .control_plane_root(&company_id)
                .map_err(fabric_error)?;
            let manifest = harness_fabric::FabricStore::restore_backup(&backup, &target)
                .map_err(fabric_error)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            return Ok(());
        }
        Some("serve") => {}
        _ => {
            return Err(CliError::Usage(
                "fabric control-plane requires serve|backup|restore; backup --company <id> --output <new-dir>; restore --company <id> --backup <dir> [--firm-home <dir>]".into(),
            ))
        }
    }
    let company_id = required(args, "--company")?;
    let gateway_addr = required(args, "--gateway-addr")?;
    let instance_id = value(args, "--instance-id")
        .unwrap_or_else(|| format!("control-plane:{}", std::process::id()));
    let firm_home = firm_home(resolved, args)?;
    let layout = RemoteFabricStoreLayout::open(&firm_home).map_err(fabric_error)?;
    let store = Arc::new(
        layout
            .open_control_plane(&company_id)
            .map_err(fabric_error)?,
    );
    let collaboration_root = layout
        .collaboration_root(&company_id)
        .map_err(fabric_error)?;
    layout
        .open_collaboration_store(&company_id)
        .map_err(fabric_error)?;
    let artifact_key = required_key_file(args, "--artifact-key-file")?;
    let capability_key = required_key_file(args, "--capability-key-file")?;
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(&company_id, artifact_key);
    let control = ControlPlane::new(&company_id, &instance_id, &store, &keys, capability_key);
    let now = now_unix_ms().map_err(fabric_error)?;
    let prior_revision = store
        .snapshot()
        .map_err(fabric_error)?
        .control_plane_leases
        .get(&company_id)
        .map_or(0, |lease| lease.revision);
    let lease = control
        .acquire_lease(
            &format!("control-plane-lease:{instance_id}"),
            prior_revision,
            now,
        )
        .map_err(fabric_error)?;
    let generation = lease.control_plane_generation;
    let tls = ControlPlaneTlsFiles {
        server_certificate_chain_pem: required_path(args, "--server-cert")?,
        server_private_key_pem: required_path(args, "--server-key")?,
        node_ca_pem: required_path(args, "--node-ca")?,
    };
    tls.validate().map_err(fabric_error)?;
    let http_addr = required(args, "--http-addr")?;
    let http_socket = http_addr
        .parse::<std::net::SocketAddr>()
        .map_err(|_| CliError::Usage("--http-addr must be an explicit socket address".into()))?;
    if !http_socket.ip().is_loopback() {
        return Err(CliError::Usage(
            "Host REST is bearer-authenticated and must bind loopback; expose it only through a trusted TLS reverse proxy"
                .into(),
        ));
    }
    let host_token = required_secret_file(args, "--host-token-file")?;
    if host_token.len() < 32 {
        return Err(CliError::Usage(
            "--host-token must contain at least 32 characters".into(),
        ));
    }
    let trusted_origin =
        value(args, "--trusted-origin").unwrap_or_else(|| format!("http://{http_addr}"));
    let ca = harness_fabric::pki::FabricCaMaterial {
        certificate_pem: std::fs::read_to_string(required_path(args, "--ca-cert")?)?,
        private_key_pem: required_secret_file(args, "--ca-key")?,
    };
    let listener = std::net::TcpListener::bind(&gateway_addr)?;
    let stop = Arc::new(AtomicBool::new(false));
    let http_stop = stop.clone();
    let http_store = store.clone();
    let http_company = company_id.clone();
    let http_instance = instance_id.clone();
    let http_collaboration_root = collaboration_root.clone();
    std::thread::spawn(move || {
        if let Err(error) = serve_host_http(
            &http_addr,
            &trusted_origin,
            &host_token,
            &http_company,
            &http_instance,
            generation,
            http_store,
            artifact_key,
            capability_key,
            &ca,
            http_collaboration_root,
            http_stop,
        ) {
            eprintln!("Remote Fabric Host REST stopped: {error}");
        }
    });
    let heartbeat_stop = stop.clone();
    let heartbeat_store = store.clone();
    let heartbeat_company = company_id.clone();
    let heartbeat_instance = instance_id.clone();
    std::thread::spawn(move || {
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(&heartbeat_company, artifact_key);
        let control = ControlPlane::new(
            &heartbeat_company,
            &heartbeat_instance,
            &heartbeat_store,
            &keys,
            capability_key,
        );
        let mut revision = lease.revision;
        while !heartbeat_stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(10));
            let now = now_unix_ms().expect("Control Plane heartbeat clock");
            match control.heartbeat_lease(generation, revision, now) {
                Ok(next) => revision = next.revision,
                Err(error) => {
                    eprintln!("Remote Fabric Control Plane lease lost: {error}");
                    heartbeat_stop.store(true, Ordering::SeqCst);
                }
            }
        }
    });
    println!(
        "Remote Fabric Control Plane company={company_id} generation={generation} host=http://{} gateway=wss://{gateway_addr}{}",
        required(args, "--http-addr")?,
        harness_fabric::transport::FABRIC_GATEWAY_PATH
    );
    let max_connections = value(args, "--max-connections")
        .map(|raw| raw.parse::<u64>())
        .transpose()
        .map_err(|_| CliError::Usage("--max-connections must be an integer".into()))?;
    let accepted = AtomicU64::new(0);
    for incoming in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let tcp = incoming?;
        let tls = tls.clone();
        let session_store = store.clone();
        let company = company_id.clone();
        let instance = instance_id.clone();
        let collaboration_root = collaboration_root.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), FabricError> {
                let (mut socket, peer) = accept_control_plane_mtls(tcp, &tls)?;
                let keys = InMemoryArtifactKeyBackend::default();
                keys.insert(&company, artifact_key);
                let control =
                    ControlPlane::new(&company, &instance, &session_store, &keys, capability_key);
                let application = Wave6ControlPlaneApplication {
                    collaboration_root,
                    company_id: company.clone(),
                    actor_id: format!("control-plane:{instance}"),
                };
                serve_control_plane_session_with_application(
                    &mut socket,
                    &peer,
                    &control,
                    generation,
                    &application,
                )
            })();
            if let Err(error) = result {
                eprintln!("Remote Fabric gateway session ended: {error}");
            }
        });
        let count = accepted.fetch_add(1, Ordering::SeqCst).saturating_add(1);
        if max_connections.is_some_and(|limit| count >= limit) {
            break;
        }
    }
    stop.store(true, Ordering::SeqCst);
    Ok(())
}

fn node_gateway_command(
    wave4c_store: &HarnessStore,
    resolved: &ResolvedStore,
    args: &[String],
) -> CliResult<()> {
    if args.first().map(String::as_str) != Some("serve") {
        return Err(CliError::Usage(
            "fabric node-gateway serve --company <id> --control-plane <wss-url> --client-cert <path> --client-key <path> --control-plane-ca <path> --certificate-serial <serial> --public-key-fingerprint <sha256>".into(),
        ));
    }
    let company_id = required(args, "--company")?;
    let node_id = super::read_local_node_id()?;
    let now = now_unix_ms().map_err(fabric_error)?;
    let daemon = wave4c_store
        .latest_node_daemon_lease(&node_id)?
        .filter(|lease| {
            lease.status == harness_core::NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > now
        })
        .ok_or_else(|| {
            CliError::Usage(
                "NodeGateway requires the exact current active Wave4C NodeDaemonLease".into(),
            )
        })?;
    let firm_home = firm_home(resolved, args)?;
    let layout = RemoteFabricStoreLayout::open(firm_home).map_err(fabric_error)?;
    let local = layout
        .open_node_local(&company_id, &node_id)
        .map_err(fabric_error)?;
    let claimed_schema_digest = required(args, "--schema-bundle-digest")?;
    let schema_digest = remote_fabric_schema_bundle_digest();
    if claimed_schema_digest != schema_digest {
        return Err(CliError::Usage(
            "--schema-bundle-digest must equal the digest of the schema bundle compiled into this exact firm build"
                .into(),
        ));
    }
    let credentials = resolve_node_credentials(args, &company_id, &node_id)?;
    let hello = NodeHello {
        company_id: company_id.clone(),
        node_id: node_id.clone(),
        instance_id: value(args, "--instance-id")
            .unwrap_or_else(|| format!("gateway:{}", std::process::id())),
        node_daemon_id: daemon.daemon_id.clone(),
        node_daemon_generation: daemon.generation,
        protocol_min: FABRIC_PROTOCOL_VERSION,
        protocol_max: FABRIC_PROTOCOL_VERSION,
        schema_bundle_digest: schema_digest,
        features: std::collections::BTreeSet::from([
            "durable-routing".into(),
            "remote-runtime".into(),
            "remote-message".into(),
            "artifact-transfer".into(),
            "cross-team-collaboration".into(),
        ]),
        build_sha: super::build_git_rev().to_string(),
        last_persisted_route_seq: local
            .snapshot()
            .map_err(fabric_error)?
            .inboxes
            .values()
            .map(|inbox| inbox.route_seq)
            .max()
            .unwrap_or(0),
        unresolved_operation_ids: local.unresolved_operation_ids().map_err(fabric_error)?,
        certificate_serial: credentials.certificate_serial.clone(),
        public_key_fingerprint: credentials.public_key_fingerprint.clone(),
    };
    let config = NodeFabricConfig {
        company_id,
        node_id,
        control_plane_url: required(args, "--control-plane")?,
        reconnect_floor_ms: 250,
        reconnect_ceiling_ms: 10_000,
    };
    let mut gateway = match &credentials.tls {
        ResolvedNodeTls::Files(tls) => NodeGatewayConnection::connect(&config, tls, hello),
        ResolvedNodeTls::Material(tls) => {
            NodeGatewayConnection::connect_with_material(&config, tls, hello)
        }
    }
    .map_err(fabric_error)?;
    local
        .bind_gateway_session(&gateway.session)
        .map_err(fabric_error)?;
    println!(
        "Remote Fabric NodeGateway connected node={} gateway_generation={} control_plane_generation={}",
        gateway.session.node_id,
        gateway.session.gateway_generation,
        gateway.session.control_plane_generation
    );
    let once = args.iter().any(|arg| arg == "--once");
    let mut application = Wave4cApplication {
        probe: ProbeApplication,
        firm_home: layout.firm_home().to_path_buf(),
        node_id: gateway.session.node_id.clone(),
        daemon_id: gateway.session.node_daemon_id.clone(),
        daemon_generation: gateway.session.node_daemon_generation,
    };
    loop {
        // A real two-machine mTLS heartbeat includes a durable lease CAS.
        // HeartbeatAck is followed by zero or more routed-operation frames,
        // but v1 has no batch-end frame. Every read therefore needs the same
        // bounded LAN timeout: a shorter idle poll can leave a delayed routed
        // frame in the socket and misread it as the next HeartbeatAck.
        gateway
            .set_read_timeout(Some(GATEWAY_FRAME_READ_TIMEOUT))
            .map_err(fabric_error)?;
        gateway.heartbeat().map_err(|mut error| {
            error.message = format!("gateway heartbeat failed: {}", error.message);
            fabric_error(error)
        })?;
        loop {
            match gateway.apply_next(&local, &mut application) {
                Ok(receipt) => println!(
                    "Remote Fabric applied operation={} effect={:?}",
                    receipt.operation_id, receipt.application_effect
                ),
                Err(error) if error.code == FabricErrorCode::TargetOffline && error.retryable => {
                    break
                }
                Err(error)
                    if error.code == FabricErrorCode::TargetOffline
                        && error.message == "pending delivery batch is complete" =>
                {
                    break
                }
                Err(mut error) => {
                    error.message = format!("gateway pending delivery failed: {}", error.message);
                    return Err(fabric_error(error));
                }
            }
        }
        for mut operation in local.pending_outbox_operations().map_err(fabric_error)? {
            let now_unix_ms = now_unix_ms().map_err(fabric_error)?;
            if operation.expires_at_unix_ms <= now_unix_ms {
                if let Some(outbox) = local
                    .expire_unaccepted_outbox(&gateway.session, &operation.id, now_unix_ms)
                    .map_err(fabric_error)?
                {
                    println!(
                        "Remote Fabric settled unaccepted expired operation={} truth={}",
                        operation.id,
                        outbox
                            .terminal_receipt_ref
                            .as_deref()
                            .unwrap_or("local:not_applied:operation_expired")
                    );
                    continue;
                }
            }
            let receipts = gateway
                .reconcile_operations(&local, BTreeSet::from([operation.id.clone()]))
                .map_err(fabric_error)?;
            if !receipts.is_empty() {
                // FabricStore already owns route truth. Accepted operations
                // are reconciled on later heartbeats until a generation-fenced
                // target terminal receipt arrives; they are never resubmitted.
                continue;
            }
            if operation.source_gateway_generation != Some(gateway.session.gateway_generation)
                || operation.source_node_daemon_id.as_deref()
                    != Some(gateway.session.node_daemon_id.as_str())
                || operation.source_node_daemon_generation
                    != Some(gateway.session.node_daemon_generation)
                || operation.control_plane_generation != gateway.session.control_plane_generation
            {
                operation = local
                    .rebind_unaccepted_outbox(&gateway.session, &operation.id, &receipts)
                    .map_err(fabric_error)?;
            }
            let actor = operation.actor.clone();
            let receipt = gateway
                .submit_operation(&local, &actor, operation)
                .map_err(fabric_error)?;
            println!(
                "Remote Fabric submitted operation={} receipt={:?}",
                receipt.operation_id, receipt.kind
            );
        }
        if once {
            gateway.close().map_err(fabric_error)?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

struct Wave4cApplication {
    probe: ProbeApplication,
    firm_home: PathBuf,
    node_id: String,
    daemon_id: String,
    daemon_generation: u64,
}

impl Wave4cApplication {
    fn target_store(
        &self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<(String, HarnessStore), FabricError> {
        let execution_space_id =
            operation
                .target_execution_space_id
                .as_deref()
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::TargetNotPlaced,
                        "routed application has no target Execution Space",
                    )
                })?;
        let space = super::execution_space::context_for_id(&self.firm_home, execution_space_id)
            .map_err(|error| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    format!("Execution Space registry failed: {error}"),
                )
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::TargetNotPlaced,
                    "target Execution Space is not registered on this Node",
                )
            })?;
        Ok((
            execution_space_id.into(),
            HarnessStore::new(space.store_root),
        ))
    }

    fn persist_message(
        &self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<(String, serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
        let message = super::remote_fabric::resolved_message_from_operation(operation)?;
        let (execution_space_id, store) = self.target_store(operation)?;
        let context = harness_core::agentfirm_api::MutationContext {
            execution_space_id,
            authenticated_actor: harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::Service,
                id: self.daemon_id.clone(),
            },
            authority_actor: None,
            command_name: "remote_message_persist".into(),
            idempotency_key: operation.id.clone(),
            expected_version: 0,
            request_fingerprint: Some(harness_fabric::json_digest(operation).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?),
        };
        let persisted = store
            .persist_remote_message(
                &context,
                operation,
                message,
                &self.node_id,
                &self.daemon_id,
                self.daemon_generation,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::UnauthorizedActor, error.to_string())
            })?;
        Ok((
            "agentfirm.remote_fabric.message_persisted.v1".into(),
            serde_json::json!({
                "message_id": persisted.projection.id,
                "canonical_event_id": persisted.event.id,
                "replayed": persisted.replayed,
            }),
            harness_fabric::EffectCertainty::Applied,
        ))
    }
}

impl NodeApplication for Wave4cApplication {
    fn authorize_artifact_download(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
        capability: &harness_fabric::ArtifactCapability,
    ) -> Result<(), FabricError> {
        let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
            operation.closed_body()?
        else {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "artifact download is not a collaboration operation",
            ));
        };
        if reference.business_kind != "artifact_grant" {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "artifact download is not an artifact grant",
            ));
        }
        let payload: CollaborationArtifactGrantEnvelope =
            serde_json::from_value(reference.payload.clone()).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        validate_artifact_grant_authority(
            &payload,
            capability,
            &reference,
            operation,
            &self.node_id,
        )
    }

    fn replay_downloaded_artifact(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<Option<(String, serde_json::Value, harness_fabric::EffectCertainty)>, FabricError>
    {
        let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
            operation.closed_body()?
        else {
            return Ok(None);
        };
        if reference.business_kind != "artifact_grant" {
            return Ok(None);
        }
        let payload: CollaborationArtifactGrantEnvelope =
            serde_json::from_value(reference.payload.clone()).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        let (_, store) = self.target_store(operation)?;
        let Some(import) = store
            .collaboration_artifact_import(&operation.company_id, &payload.manifest.id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
        else {
            return Ok(None);
        };
        if import.operation_id != operation.id
            || import.delegation_id != payload.delegation_id
            || import.artifact_digest != payload.manifest.sha256
            || import.source_node_id != self.node_id
            || import.source_node_daemon_id != self.daemon_id
            || import.source_node_daemon_generation != self.daemon_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "artifact import replay changed operation, bytes, or NodeDaemon generation",
            ));
        }
        store
            .read_collaboration_artifact_import_bytes(&operation.company_id, &payload.manifest.id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::ArtifactTampered, error.to_string())
            })?;
        Ok(Some((
            "agentfirm.collaboration.artifact_imported.v1".into(),
            serde_json::json!({"artifact_import": import, "replayed": true}),
            harness_fabric::EffectCertainty::Applied,
        )))
    }

    fn apply(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
    ) -> Result<(String, serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
        match operation.closed_body()? {
            harness_fabric::ClosedOperationBody::Probe(_)
            | harness_fabric::ClosedOperationBody::ReconcileProbe(_) => self.probe.apply(operation),
            harness_fabric::ClosedOperationBody::RuntimeCommand(_) => {
                let envelope = super::remote_fabric::resolved_runtime_command_from_operation(
                    operation,
                    &self.node_id,
                    &self.daemon_id,
                    self.daemon_generation,
                )?;
                let (result, effect) = dispatch_resolved_runtime_command(
                    &self.firm_home,
                    operation,
                    &envelope,
                    &self.node_id,
                    &self.daemon_id,
                    self.daemon_generation,
                )?;
                Ok((
                    "agentfirm.remote_fabric.runtime_command_result.v1".into(),
                    result,
                    effect,
                ))
            }
            harness_fabric::ClosedOperationBody::Message(_) => self.persist_message(operation),
            harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => {
                match reference.business_kind.as_str() {
                    "target_work_create"
                    | "delegation_propose"
                    | "delegation_decide"
                    | "delegation_cancel_request"
                    | "delegation_cancel_decide"
                    | "remote_fact_publish" => {
                        let (_, store) = self.target_store(operation)?;
                        harness_store::apply_collaboration_target_operation(
                            &store,
                            operation,
                            &format!(
                                "unix-ms:{}",
                                harness_fabric::gateway_runtime::now_unix_ms()?
                            ),
                        )
                    }
                    "artifact_grant" => Err(FabricError::unknown(
                        operation.id.clone(),
                        "artifact grant cannot be applied before its one-use capability is consumed and source bytes are durably imported",
                    )),
                    "team_message_deliver" => self.persist_message(operation),
                    _ => Err(FabricError::none(
                        FabricErrorCode::FeatureIncompatible,
                        "target Node has no local business authority for this Control Plane-owned collaboration kind",
                    )),
                }
            }
            _ => Err(FabricError::none(
                FabricErrorCode::FeatureIncompatible,
                "Node application adapter does not own this routed reference kind",
            )),
        }
    }

    fn apply_downloaded_artifact(
        &mut self,
        operation: &harness_fabric::RoutedOperation,
        artifact_id: &str,
        artifact_digest: &str,
        bytes: &[u8],
    ) -> Result<(String, serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
        let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
            operation.closed_body()?
        else {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "downloaded artifact is not a collaboration operation",
            ));
        };
        if reference.business_kind != "artifact_grant" {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "downloaded artifact is not an artifact grant",
            ));
        }
        let payload: CollaborationArtifactGrantEnvelope =
            serde_json::from_value(reference.payload.clone()).map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        validate_artifact_grant_authority(
            &payload,
            &payload.read_capability,
            &reference,
            operation,
            &self.node_id,
        )?;
        let delegation_id = payload.delegation_id.clone();
        let (_, store) = self.target_store(operation)?;
        let delegation = payload.delegation;
        let attestation = payload.source_work_attestation;
        if delegation.source_node_id != self.node_id
            || operation.target_node_id != self.node_id
            || payload.manifest.id != artifact_id
            || payload.manifest.sha256 != artifact_digest
            || payload.manifest.size_bytes != bytes.len() as u64
            || payload.read_capability.artifact_id != artifact_id
            || payload.read_capability.artifact_digest != artifact_digest
            || harness_fabric::sha256_hex(bytes) != artifact_digest
        {
            return Err(FabricError::none(
                FabricErrorCode::ArtifactTampered,
                "downloaded artifact bytes or source Node disagree with canonical authority",
            ));
        }
        let imported_at = harness_fabric::gateway_runtime::now_unix_ms()?;
        let import = harness_core::collaboration::ArtifactImport {
            id: format!("artifact-import:{artifact_id}"),
            company_id: operation.company_id.clone(),
            delegation_id,
            artifact_id: artifact_id.into(),
            artifact_digest: artifact_digest.into(),
            size_bytes: bytes.len() as u64,
            source_node_id: self.node_id.clone(),
            source_node_daemon_id: self.daemon_id.clone(),
            source_node_daemon_generation: self.daemon_generation,
            source_team_id: delegation.source_team_id.clone(),
            source_host_ref: attestation.source_host_ref.clone(),
            source_work_ref: delegation.source_work_ref.clone(),
            operation_id: operation.id.clone(),
            imported_at_unix_ms: imported_at,
            revision: 1,
        };
        let persisted = store
            .persist_collaboration_artifact_import_with_frozen_authority(
                &harness_store::CollaborationMutationContext {
                    company_id: operation.company_id.clone(),
                    authenticated_actor: harness_core::agentfirm_api::ActorRef {
                        kind: harness_core::agentfirm_api::ActorKind::Service,
                        id: self.daemon_id.clone(),
                    },
                    command_name: "artifact_import.persist".into(),
                    idempotency_key: operation.id.clone(),
                    expected_revision: 0,
                    occurred_at: format!("unix-ms:{imported_at}"),
                },
                &import,
                bytes,
                &delegation,
                &attestation,
            )
            .map_err(|error| FabricError::unknown(operation.id.clone(), error.to_string()))?;
        Ok((
            "agentfirm.collaboration.artifact_imported.v1".into(),
            serde_json::json!({"artifact_import": persisted.projection, "replayed": persisted.replayed}),
            harness_fabric::EffectCertainty::Applied,
        ))
    }
}

fn dispatch_resolved_runtime_command(
    firm_home: &Path,
    operation: &harness_fabric::RoutedOperation,
    envelope: &harness_core::agentfirm_api::ControlCommandEnvelope,
    target_node_id: &str,
    target_node_daemon_id: &str,
    target_node_daemon_generation: u64,
) -> Result<(serde_json::Value, harness_fabric::EffectCertainty), FabricError> {
    use harness_core::agentfirm_api::{RuntimeCommandStatus, RuntimeEffectCertainty};

    super::remote_fabric::validate_resolved_runtime_command(
        operation,
        envelope,
        target_node_id,
        target_node_daemon_id,
        target_node_daemon_generation,
    )?;
    let transport = super::supervisor_daemon::runtime_command_via_socket(
        firm_home,
        &envelope.target_node_id,
        envelope,
    );
    let space = super::execution_space::context_for_id(firm_home, &envelope.execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::TargetNotPlaced,
                "RuntimeCommand target Execution Space is not registered on this Node",
            )
        })?;
    let record = HarnessStore::new(space.store_root)
        .runtime_commands(&envelope.execution_space_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .into_iter()
        .find(|record| record.id == envelope.id);
    let transport_detail = match transport {
        Ok(response) => response
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| "NodeDaemon returned a non-terminal response".into()),
        Err(error) => format!("NodeDaemon transport ended before a response: {error}"),
    };
    match record {
        Some(record)
            if record.status == RuntimeCommandStatus::Applied
                && record.effect_certainty == RuntimeEffectCertainty::Applied =>
        {
            Ok((
                serde_json::json!({
                    "runtime_command_id": record.id,
                    "status": record.status,
                    "result": record.result,
                }),
                harness_fabric::EffectCertainty::Applied,
            ))
        }
        Some(record)
            if record.status == RuntimeCommandStatus::Failed
                && matches!(
                    record.effect_certainty,
                    RuntimeEffectCertainty::None | RuntimeEffectCertainty::NotApplied
                ) =>
        {
            Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                record.failure_code.unwrap_or(transport_detail),
            ))
        }
        Some(record)
            if record.status == RuntimeCommandStatus::RecoveryRequired
                || record.effect_certainty == RuntimeEffectCertainty::Unknown =>
        {
            let mut failure = FabricError::unknown(
                operation.id.clone(),
                record.failure_code.unwrap_or(transport_detail),
            );
            failure
                .details
                .insert("runtime_command_id".into(), envelope.id.clone());
            failure.details.insert(
                "reconciliation".into(),
                "resolve the durable target RuntimeCommand before any retry".into(),
            );
            Err(failure)
        }
        Some(record) => Err(FabricError::unknown(
            operation.id.clone(),
            format!(
                "RuntimeCommand remained non-terminal ({:?}/{:?}): {transport_detail}",
                record.status, record.effect_certainty
            ),
        )),
        None => Err(FabricError::unknown(
            operation.id.clone(),
            format!("RuntimeCommand has no durable target admission: {transport_detail}"),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn serve_host_http(
    addr: &str,
    trusted_origin: &str,
    host_token: &str,
    company_id: &str,
    instance_id: &str,
    generation: u64,
    store: Arc<harness_fabric::FabricStore>,
    artifact_key: [u8; 32],
    capability_key: [u8; 32],
    ca: &harness_fabric::pki::FabricCaMaterial,
    collaboration_root: PathBuf,
    stop: Arc<AtomicBool>,
) -> CliResult<()> {
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let keys = InMemoryArtifactKeyBackend::default();
    keys.insert(company_id, artifact_key);
    let control = ControlPlane::new(company_id, instance_id, &store, &keys, capability_key);
    while !stop.load(Ordering::SeqCst) {
        let (stream, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let request = match read_http_request(stream, trusted_origin, host_token) {
            Ok(request) => request,
            Err(error) => {
                eprintln!("Remote Fabric Host REST request rejected: {error}");
                continue;
            }
        };
        handle_host_http(
            request,
            trusted_origin,
            host_token,
            &control,
            generation,
            ca,
            &collaboration_root,
        )?;
    }
    Ok(())
}

struct HttpRequest {
    stream: TcpStream,
    method: String,
    target: String,
    headers: std::collections::BTreeMap<String, String>,
    body: Vec<u8>,
}

const STANDARD_FABRIC_HTTP_BODY_LIMIT: usize = 1024 * 1024;
const MAX_FABRIC_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
// Artifact completion uses a closed JSON envelope with hexadecimal content.
// Reserve bounded space for the signed capability and JSON framing without
// widening any other Host REST endpoint beyond the normal 1 MiB limit.
const ARTIFACT_COMPLETE_HTTP_BODY_LIMIT: usize = MAX_FABRIC_ARTIFACT_BYTES * 2 + 256 * 1024;

fn is_artifact_complete_path(path: &str) -> bool {
    path.strip_prefix("/v1/fabric/artifacts/")
        .and_then(|rest| rest.strip_suffix("/complete"))
        .is_some_and(|artifact_id| !artifact_id.is_empty() && !artifact_id.contains('/'))
}

fn fabric_http_body_limit(method: &str, target: &str) -> usize {
    let path = target.split('?').next().unwrap_or_default();
    if method == "POST" && is_artifact_complete_path(path) {
        ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
    } else {
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    }
}

fn authorized_fabric_http_body_limit(
    method: &str,
    target: &str,
    headers: &std::collections::BTreeMap<String, String>,
    trusted_origin: &str,
    host_token: &str,
) -> usize {
    let requested_limit = fabric_http_body_limit(method, target);
    let large_body_authorized = requested_limit > STANDARD_FABRIC_HTTP_BODY_LIMIT
        && headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|presented| constant_time_secret_eq(presented, host_token))
        && headers
            .get("origin")
            .is_none_or(|origin| origin == trusted_origin);
    if large_body_authorized {
        requested_limit
    } else {
        STANDARD_FABRIC_HTTP_BODY_LIMIT
    }
}

fn read_http_request(
    mut stream: TcpStream,
    trusted_origin: &str,
    host_token: &str,
) -> CliResult<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    if !matches!(method.as_str(), "GET" | "POST" | "OPTIONS")
        || (!target.starts_with("/v1/fabric/") && !target.starts_with("/v1/collaboration/"))
    {
        write_http_json(
            &mut stream,
            "404 Not Found",
            &serde_json::json!({"ok":false,"error":"unknown_fabric_endpoint"}),
            None,
        )?;
        return Err(CliError::Usage("unknown Remote Fabric endpoint".into()));
    }
    let mut headers = std::collections::BTreeMap::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| CliError::Usage("malformed HTTP header".into()))?;
        let name = name.trim().to_ascii_lowercase();
        if headers.insert(name.clone(), value.trim().into()).is_some() {
            return Err(CliError::Usage("duplicate HTTP header".into()));
        }
        if name == "content-length" {
            content_length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| CliError::Usage("invalid Content-Length".into()))?;
        }
    }
    let body_limit =
        authorized_fabric_http_body_limit(&method, &target, &headers, trusted_origin, host_token);
    if content_length > body_limit {
        return Err(CliError::Usage(format!(
            "Remote Fabric REST body exceeds endpoint limit of {body_limit} bytes"
        )));
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(HttpRequest {
        stream,
        method,
        target,
        headers,
        body,
    })
}

fn handle_host_http<K: harness_fabric::ArtifactKeyBackend>(
    mut request: HttpRequest,
    trusted_origin: &str,
    host_token: &str,
    control: &ControlPlane<'_, K>,
    generation: u64,
    ca: &harness_fabric::pki::FabricCaMaterial,
    collaboration_root: &Path,
) -> CliResult<()> {
    let path = request.target.split('?').next().unwrap_or_default();
    if request.method == "OPTIONS" {
        let origin = request.headers.get("origin").map(String::as_str);
        if origin != Some(trusted_origin) {
            write_http_json(
                &mut request.stream,
                "403 Forbidden",
                &serde_json::json!({"ok":false,"error":"untrusted_origin"}),
                None,
            )?;
        } else {
            write_http_json(
                &mut request.stream,
                "200 OK",
                &serde_json::json!({"ok":true}),
                origin,
            )?;
        }
        return Ok(());
    }
    let origin = request.headers.get("origin").map(String::as_str);
    if origin.is_some_and(|origin| origin != trusted_origin) {
        return respond_fabric_error(
            &mut request.stream,
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "browser origin is not the configured trusted Control Plane origin",
            ),
            None,
        );
    }
    if path.starts_with("/v1/collaboration/") {
        let result = handle_collaboration_control_plane_http(
            &request.method,
            path,
            &request.target,
            &request.headers,
            &request.body,
            control,
            generation,
            collaboration_root,
        );
        return match result {
            Ok(value) => write_http_json(&mut request.stream, "200 OK", &value, origin),
            Err(error) => respond_fabric_error(&mut request.stream, error, origin),
        };
    }
    let node_enroll = request.method == "POST" && path == "/v1/fabric/nodes/enroll";
    let actor = if node_enroll {
        None
    } else {
        let presented = request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "));
        if !presented.is_some_and(|presented| constant_time_secret_eq(presented, host_token))
            || request.headers.keys().any(|name| {
                matches!(
                    name.as_str(),
                    "x-agentfirm-actor-id" | "x-agentfirm-actor-kind" | "x-agentfirm-authority-id"
                )
            })
        {
            return respond_fabric_error(
                &mut request.stream,
                FabricError::none(
                    FabricErrorCode::UnauthorizedActor,
                    "Company-issued Host credential is missing or request attempted identity selection",
                ),
                origin,
            );
        }
        Some(AuthenticatedActor {
            company_id: control.company_id().into(),
            actor_id: "company-host:http".into(),
            actor_kind: harness_fabric::ActorKind::Human,
            role_bindings: std::collections::BTreeSet::from([
                "company_host".into(),
                "artifact_write".into(),
                "artifact_read".into(),
            ]),
            session_id: format!("host-http:{generation}"),
            issued_at_unix_ms: now_unix_ms().map_err(fabric_error)?,
            expires_at_unix_ms: now_unix_ms().map_err(fabric_error)?.saturating_add(30_000),
        })
    };
    let body = if request.body.is_empty() {
        serde_json::Value::Null
    } else {
        match serde_json::from_slice(&request.body) {
            Ok(value) => value,
            Err(error) => {
                return respond_fabric_error(
                    &mut request.stream,
                    FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()),
                    origin,
                )
            }
        }
    };
    let now = now_unix_ms().map_err(fabric_error)?;
    let result = route_host_http(
        &request.method,
        path,
        &request.target,
        &body,
        actor.as_ref(),
        control,
        generation,
        ca,
        now,
        host_token,
    );
    match result {
        Ok(value) => write_http_json(&mut request.stream, "200 OK", &value, origin),
        Err(error) => respond_fabric_error(&mut request.stream, error, origin),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationDecisionHttpRequest {
    reason: String,
    target_execution_space_id: String,
    expires_unix_ms: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationInboundPolicyHttpRequest {
    policy_id: String,
    target_team_id: String,
    source_team_id: String,
    mode: harness_core::collaboration::DelegationInboundMode,
    allowed_outcome_classes: Vec<String>,
    max_active_delegations: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationCancellationRequestHttpRequest {
    reason: String,
    target_execution_space_id: String,
    expires_unix_ms: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationCancellationDecisionHttpRequest {
    reason: String,
    native_work_event_ref: String,
    target_execution_space_id: String,
    expires_unix_ms: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationArtifactGrantHttpRequest {
    target_execution_space_id: String,
    expires_unix_ms: u64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationArtifactInitiateHttpRequest {
    artifact_id: String,
    operation_id: Option<String>,
    media_type: String,
    size_bytes: u64,
    sha256: String,
    classification: harness_fabric::ArtifactClassification,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CollaborationArtifactRetentionHttpRequest {
    expected_artifact_revision: u64,
    retention_duration_ms: u64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CollaborationArtifactGrantEnvelope {
    delegation_id: String,
    delegation: harness_core::collaboration::WorkDelegationV1,
    source_work_attestation: harness_core::collaboration::SourceWorkAttestation,
    manifest: harness_fabric::RemoteArtifactManifest,
    read_capability: harness_fabric::ArtifactCapability,
    source_placement: harness_core::collaboration::TargetPlacementRef,
}

#[allow(clippy::too_many_arguments)]
fn resolve_frozen_artifact_grant_replay(
    operations: &std::collections::BTreeMap<String, RoutedOperation>,
    receipts: &std::collections::BTreeMap<String, RouteReceipt>,
    company_id: &str,
    delegation_id: &str,
    artifact_id: &str,
    idempotency_key: &str,
    expected_revision: u64,
    target_execution_space_id: &str,
    expires_unix_ms: u64,
    credential_actor: &harness_core::agentfirm_api::ActorRef,
) -> Result<Option<RouteReceipt>, FabricError> {
    let operation_id = format!("route-artifact-grant:{delegation_id}:{artifact_id}");
    let Some(existing) = operations.get(&operation_id) else {
        return Ok(None);
    };
    let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
        existing.closed_body()?
    else {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "artifact grant identity belongs to another operation kind",
        ));
    };
    let payload: CollaborationArtifactGrantEnvelope =
        serde_json::from_value(reference.payload.clone()).map_err(|error| {
            FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
        })?;
    let credential_kind = match credential_actor.kind {
        harness_core::agentfirm_api::ActorKind::Human => "human",
        harness_core::agentfirm_api::ActorKind::AgentMember => "agent_member",
        harness_core::agentfirm_api::ActorKind::External
        | harness_core::agentfirm_api::ActorKind::Service => "service",
    };
    let exact = existing.company_id == company_id
        && existing.idempotency_key == idempotency_key
        && existing.expected_target_revision == Some(expected_revision)
        && existing.target_execution_space_id.as_deref() == Some(target_execution_space_id)
        && existing.actor.expires_at_unix_ms == expires_unix_ms
        && reference.business_kind == "artifact_grant"
        && reference.business_actor_kind == credential_kind
        && reference.business_actor_id == credential_actor.id
        && payload.delegation_id == delegation_id
        && payload.delegation.id == delegation_id
        && payload.delegation.revision == expected_revision
        && payload.delegation.target_host_ref == *credential_actor
        && payload.manifest.id == artifact_id;
    if !exact {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "same artifact grant identity was reused with a different request",
        ));
    }
    receipts
        .values()
        .filter(|receipt| {
            receipt.operation_id == operation_id
                && receipt.kind == ReceiptKind::ControlPlaneAccepted
        })
        .min_by_key(|receipt| (receipt.created_at_unix_ms, receipt.id.as_str()))
        .cloned()
        .map(Some)
        .ok_or_else(|| {
            FabricError::unknown(
                operation_id,
                "accepted artifact grant has no durable ControlPlane receipt",
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn grant_collaboration_artifact<K: harness_fabric::ArtifactKeyBackend>(
    store: &HarnessStore,
    control: &ControlPlane<'_, K>,
    generation: u64,
    delegation_id: &str,
    artifact_id: &str,
    request: &CollaborationArtifactGrantHttpRequest,
    credential_actor: &harness_core::agentfirm_api::ActorRef,
    control_actor: &AuthenticatedActor,
    idempotency_key: &str,
    expected_revision: u64,
    now: u64,
) -> Result<serde_json::Value, FabricError> {
    if request.target_execution_space_id.trim().is_empty() || request.expires_unix_ms <= now {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "artifact grant requires exact source Execution Space and future expiry",
        ));
    }
    let operation_id = format!("route-artifact-grant:{delegation_id}:{artifact_id}");
    let frozen_replay = std::cell::RefCell::new(None::<RouteReceipt>);
    store.with_collaboration_authority_fence(
        |locked_store| {
            // Replay resolution and mutable collaboration validation share the
            // same writer fence. Two identical requests therefore cannot both
            // miss the frozen operation before one mints a time-dependent
            // one-use capability and commits the canonical Fabric route.
            let fabric_state = control.store().snapshot()?;
            if let Some(receipt) = resolve_frozen_artifact_grant_replay(
                &fabric_state.operations,
                &fabric_state.receipts,
                control.company_id(),
                delegation_id,
                artifact_id,
                idempotency_key,
                expected_revision,
                &request.target_execution_space_id,
                request.expires_unix_ms,
                credential_actor,
            )? {
                frozen_replay.replace(Some(receipt));
                return Ok(());
            }
            let delegation = locked_store
                .collaboration_delegation(control.company_id(), delegation_id)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ExpectedRevisionConflict,
                        "Delegation does not exist",
                    )
                })?;
            let attestation = locked_store
                .collaboration_source_work_attestation(
                    control.company_id(),
                    &delegation.source_work_attestation_id,
                )
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        "Delegation lacks source Host attestation",
                    )
                })?;
            let policy = locked_store
                .collaboration_inbound_policy(
                    control.company_id(),
                    &delegation.inbound_policy_snapshot.policy_id,
                )
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::UnauthorizedActor,
                        "artifact grant inbound policy is missing",
                    )
                })?;
            let manifest = control.artifact_manifest(artifact_id)?.ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ArtifactInvalid,
                    "artifact manifest does not exist",
                )
            })?;
            validate_current_artifact_grant_authority(
                control.company_id(),
                &delegation,
                &attestation,
                &policy,
                &manifest,
                credential_actor,
                expected_revision,
            )
        },
        || {
            if let Some(receipt) = frozen_replay.borrow_mut().take() {
                return Ok(serde_json::json!({
                    "delegation_id": delegation_id,
                    "artifact_id": artifact_id,
                    "operation_id": operation_id,
                    "receipt": receipt,
                    "replayed": true,
                }));
            }
            let delegation = store
                .collaboration_delegation(control.company_id(), delegation_id)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::ExpectedRevisionConflict,
                        "Delegation does not exist",
                    )
                })?;
            let attestation = store
                .collaboration_source_work_attestation(
                    control.company_id(),
                    &delegation.source_work_attestation_id,
                )
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
                })?
                .ok_or_else(|| {
                    FabricError::none(
                        FabricErrorCode::InvalidPayload,
                        "Delegation lacks source Host attestation",
                    )
                })?;
            let manifest = control.artifact_manifest(artifact_id)?.ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ArtifactInvalid,
                    "artifact manifest does not exist",
                )
            })?;
            let grantor = AuthenticatedActor {
                company_id: control.company_id().into(),
                actor_id: credential_actor.id.clone(),
                actor_kind: match credential_actor.kind {
                    harness_core::agentfirm_api::ActorKind::Human => {
                        harness_fabric::ActorKind::Human
                    }
                    harness_core::agentfirm_api::ActorKind::AgentMember => {
                        harness_fabric::ActorKind::AgentMember
                    }
                    harness_core::agentfirm_api::ActorKind::External
                    | harness_core::agentfirm_api::ActorKind::Service => {
                        harness_fabric::ActorKind::Service
                    }
                },
                role_bindings: BTreeSet::from(["artifact_write".into()]),
                session_id: format!("collaboration-artifact:{idempotency_key}"),
                issued_at_unix_ms: now,
                expires_at_unix_ms: request.expires_unix_ms,
            };
            let capability = control.issue_delegated_download_capability(
                &grantor,
                generation,
                artifact_id,
                &attestation.source_host_ref.id,
                &delegation.source_node_id,
                now,
            )?;
            let business = store
                .artifact_grant_operation(
                    &harness_store::CollaborationMutationContext {
                        company_id: control.company_id().into(),
                        authenticated_actor: credential_actor.clone(),
                        command_name: "artifact_grant".into(),
                        idempotency_key: idempotency_key.into(),
                        expected_revision,
                        occurred_at: format!("unix-ms:{now}"),
                    },
                    delegation_id,
                    &manifest,
                    &capability,
                )
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::UnauthorizedActor, error.to_string())
                })?;
            let routed_actor = AuthenticatedActor {
                expires_at_unix_ms: request.expires_unix_ms,
                ..control_actor.clone()
            };
            let routed = harness_store::route_collaboration_business_operation(
                &business,
                &harness_store::CollaborationFabricRouteContext {
                    authenticated_actor: routed_actor.clone(),
                    resolved_business_actor: credential_actor.clone(),
                    source: harness_store::CollaborationFabricSource::ControlPlane,
                    control_plane_generation: generation,
                    target_execution_space_id: Some(request.target_execution_space_id.clone()),
                    created_at_unix_ms: now,
                    expires_at_unix_ms: request.expires_unix_ms,
                },
            )?;
            let (_, _, receipt, replayed) =
                control.accept_control_plane_operation(generation, &routed_actor, routed, now)?;
            Ok(serde_json::json!({
                "delegation_id": delegation_id,
                "artifact_id": artifact_id,
                "operation_id": receipt.operation_id,
                "receipt": receipt,
                "replayed": replayed,
            }))
        },
    )
}

fn validate_artifact_grant_authority(
    payload: &CollaborationArtifactGrantEnvelope,
    capability: &harness_fabric::ArtifactCapability,
    reference: &harness_fabric::CollaborationBusinessReference,
    operation: &harness_fabric::RoutedOperation,
    source_node_id: &str,
) -> Result<(), FabricError> {
    let actor_matches = payload.delegation.target_host_ref.id == reference.business_actor_id
        && matches!(
            (
                &payload.delegation.target_host_ref.kind,
                reference.business_actor_kind.as_str()
            ),
            (harness_core::agentfirm_api::ActorKind::Human, "human")
                | (
                    harness_core::agentfirm_api::ActorKind::AgentMember,
                    "agent_member"
                )
        );
    if &payload.read_capability != capability
        || capability.node_id != source_node_id
        || capability.company_id != operation.company_id
        || capability.purpose != harness_fabric::ArtifactCapabilityPurpose::Download
        || payload.delegation.id != payload.delegation_id
        || payload.delegation.company_id != operation.company_id
        || payload.delegation.source_node_id != source_node_id
        || payload.delegation.source_work_attestation_id != payload.source_work_attestation.id
        || payload.source_work_attestation.company_id != operation.company_id
        || payload.source_work_attestation.source_work_ref != payload.delegation.source_work_ref
        || payload.source_work_attestation.source_owner_ref != payload.delegation.source_owner_ref
        || payload.source_work_attestation.source_host_ref.id != capability.issued_to
        || !actor_matches
        || payload.source_placement.team_id != payload.delegation.source_team_id
        || payload.source_placement.team_revision
            != payload.delegation.source_work_ref.team_revision
        || payload.source_placement.node_id != source_node_id
        || payload.source_placement.placement_generation
            != payload.delegation.source_work_ref.placement_generation
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "artifact capability or frozen central collaboration authority changed or is outside the authenticated source Node",
        ));
    }
    Ok(())
}

fn unix_ms_timestamp(value: &str) -> Option<u64> {
    value.strip_prefix("unix-ms:")?.parse().ok()
}

fn collaboration_actor_can_read_delegation(
    store: &HarnessStore,
    company_id: &str,
    actor: &harness_core::agentfirm_api::ActorRef,
    delegation: &harness_core::collaboration::WorkDelegationV1,
) -> Result<bool, FabricError> {
    let source_host = store
        .collaboration_source_work_attestation(company_id, &delegation.source_work_attestation_id)
        .map_err(|error| FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string()))?
        .map(|attestation| attestation.source_host_ref);
    Ok(actor == &delegation.source_owner_ref
        || source_host.as_ref() == Some(actor)
        || actor == &delegation.target_host_ref)
}

pub(crate) fn encode_collaboration_cursor(
    cursor: &harness_store::CollaborationScopedCursor,
    secret: &str,
) -> Result<String, FabricError> {
    let bytes = serde_json::to_vec(cursor)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    let payload = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "collaboration cursor signing key is invalid",
        )
    })?;
    mac.update(payload.as_bytes());
    let signature = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("v1.{payload}.{signature}"))
}

pub(crate) fn decode_collaboration_cursor(
    encoded: &str,
    secret: &str,
) -> Result<harness_store::CollaborationScopedCursor, FabricError> {
    let mut parts = encoded.split('.');
    let (Some("v1"), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "collaboration cursor is malformed",
        ));
    };
    let signature_bytes = (0..signature.len())
        .step_by(2)
        .map(|offset| {
            signature
                .get(offset..offset + 2)
                .ok_or(())
                .and_then(|value| u8::from_str_radix(value, 16).map_err(|_| ()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "collaboration cursor signature is invalid",
            )
        })?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "collaboration cursor signing key is invalid",
        )
    })?;
    mac.update(payload.as_bytes());
    if mac.verify_slice(&signature_bytes).is_err() || payload.len() % 2 != 0 {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "collaboration cursor signature is invalid",
        ));
    }
    let bytes = (0..payload.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&payload[offset..offset + 2], 16).map_err(|_| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "collaboration cursor encoding is invalid",
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::from_slice(&bytes)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn handle_collaboration_control_plane_http<K: harness_fabric::ArtifactKeyBackend>(
    method: &str,
    path: &str,
    target: &str,
    headers: &std::collections::BTreeMap<String, String>,
    body: &[u8],
    control: &ControlPlane<'_, K>,
    generation: u64,
    collaboration_root: &Path,
) -> Result<serde_json::Value, FabricError> {
    if headers.keys().any(|name| {
        matches!(
            name.as_str(),
            "x-agentfirm-actor-id"
                | "x-agentfirm-actor-kind"
                | "x-agentfirm-authority-id"
                | "x-agentfirm-authority-kind"
        )
    }) {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "collaboration request headers cannot select actor or authority identity",
        ));
    }
    let credential = super::resolve_agentfirm_http_credential(
        headers.get("x-agentfirm-token").map(String::as_str),
    )
    .map_err(|message| FabricError::none(FabricErrorCode::UnauthorizedActor, message))?;
    let store = HarnessStore::new(collaboration_root);
    if method == "GET" && path == "/v1/collaboration/delegations" {
        let query_value = |key: &str| {
            target.split_once('?').and_then(|(_, query)| {
                query.split('&').find_map(|part| {
                    let (candidate, value) = part.split_once('=')?;
                    (candidate == key).then_some(value)
                })
            })
        };
        let state = target
            .split_once('?')
            .and_then(|(_, query)| {
                query.split('&').find_map(|part| {
                    let (key, value) = part.split_once('=')?;
                    (key == "state").then_some(value)
                })
            })
            .map(|state| match state {
                "proposed" => Ok(harness_core::collaboration::DelegationState::Proposed),
                "awaiting_target_decision" => {
                    Ok(harness_core::collaboration::DelegationState::AwaitingTargetDecision)
                }
                "provisioning_target_work" => {
                    Ok(harness_core::collaboration::DelegationState::ProvisioningTargetWork)
                }
                "active" => Ok(harness_core::collaboration::DelegationState::Active),
                "result_available" => {
                    Ok(harness_core::collaboration::DelegationState::ResultAvailable)
                }
                "cancellation_requested" => {
                    Ok(harness_core::collaboration::DelegationState::CancellationRequested)
                }
                "terminal" => Ok(harness_core::collaboration::DelegationState::Terminal),
                _ => Err(FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "unknown Delegation state filter",
                )),
            })
            .transpose()?;
        let filter = harness_store::CollaborationDelegationFilter {
            source_team_id: None,
            target_team_id: None,
            node_id: None,
            state,
        };
        let cursor_secret = headers.get("x-agentfirm-token").ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "collaboration cursor requires authenticated credential",
            )
        })?;
        let cursor = query_value("cursor")
            .map(|value| decode_collaboration_cursor(value, cursor_secret))
            .transpose()?;
        let limit = query_value("limit")
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "collaboration limit is invalid",
                )
            })?
            .unwrap_or(100);
        let page = store
            .list_collaboration_delegations_for_actor(
                control.company_id(),
                &credential.actor,
                &filter,
                cursor,
                limit,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        let next_cursor = page
            .next_cursor
            .as_ref()
            .map(|value| encode_collaboration_cursor(value, cursor_secret))
            .transpose()?;
        return Ok(serde_json::json!({
            "items": page.items,
            "as_of_store_sequence": page.as_of_store_sequence,
            "next_cursor": next_cursor,
        }));
    }
    if method == "POST" && path == "/v1/collaboration/inbound-policies" {
        let idempotency_key = headers
            .get("idempotency-key")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "Idempotency-Key is required",
                )
            })?;
        let expected_revision = headers
            .get("if-match")
            .and_then(|value| value.trim_matches('"').parse::<u64>().ok())
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "If-Match exact policy revision is required",
                )
            })?;
        let request = serde_json::from_slice::<CollaborationInboundPolicyHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        if request.policy_id.trim().is_empty()
            || request.target_team_id.trim().is_empty()
            || request.source_team_id.trim().is_empty()
            || request.allowed_outcome_classes.is_empty()
            || request.max_active_delegations == 0
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "inbound policy requires an authenticated AgentMember target Host and bounded policy scope",
            ));
        }
        let teams = store.teams().map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("target AgentTeam lookup failed: {error}"),
            )
        })?;
        let target_team = teams
            .iter()
            .rev()
            .find(|team| team.id == request.target_team_id)
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "target AgentTeam does not exist",
                )
            })?;
        let resolved_target_host = harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: target_team.host_agent_id.clone(),
        };
        if credential.actor != resolved_target_host {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "only the server-resolved exact target Host may author inbound policy",
            ));
        }
        let now = now_unix_ms()?;
        let policy = harness_core::collaboration::DelegationInboundPolicy {
            id: request.policy_id,
            company_id: control.company_id().into(),
            target_team_id: request.target_team_id,
            source_team_id: request.source_team_id,
            mode: request.mode,
            allowed_outcome_classes: request.allowed_outcome_classes,
            max_active_delegations: request.max_active_delegations,
            created_by_target_host: credential.actor.clone(),
            revision: expected_revision.saturating_add(1),
            created_at: format!("unix-ms:{now}"),
            revoked_at: None,
        };
        let written = store
            .put_collaboration_inbound_policy(
                &harness_store::CollaborationMutationContext {
                    company_id: control.company_id().into(),
                    authenticated_actor: credential.actor.clone(),
                    command_name: "delegation_inbound_policy.put".into(),
                    idempotency_key: idempotency_key.clone(),
                    expected_revision,
                    occurred_at: format!("unix-ms:{now}"),
                },
                &policy,
                &resolved_target_host,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        return Ok(serde_json::json!({
            "policy": written.projection,
            "replayed": written.replayed,
        }));
    }
    let suffix = path
        .strip_prefix("/v1/collaboration/delegations/")
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                "unknown collaboration endpoint",
            )
        })?;
    if method == "GET" && suffix.ends_with("/publications") {
        let delegation_id = suffix
            .strip_suffix("/publications")
            .filter(|value| !value.is_empty() && !value.contains('/'))
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "publication list path is malformed",
                )
            })?;
        let delegation = store
            .collaboration_delegation(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        let source_host = store
            .collaboration_source_work_attestation(
                control.company_id(),
                &delegation.source_work_attestation_id,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .map(|attestation| attestation.source_host_ref);
        if credential.actor != delegation.source_owner_ref
            && source_host.as_ref() != Some(&credential.actor)
            && credential.actor != delegation.target_host_ref
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "publication projection requires the exact source owner/Host or target Host",
            ));
        }
        let publications = store
            .collaboration_publications(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?;
        return Ok(serde_json::json!({
            "delegation_id": delegation_id,
            "delegation_revision": delegation.revision,
            "items": publications,
        }));
    }
    if method == "GET" && !suffix.contains('/') {
        let delegation = store
            .collaboration_delegation(control.company_id(), suffix)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        if !collaboration_actor_can_read_delegation(
            &store,
            control.company_id(),
            &credential.actor,
            &delegation,
        )? {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "Delegation projection requires the exact source owner/Host or target Host",
            ));
        }
        return Ok(serde_json::json!({"delegation":delegation}));
    }
    let idempotency_key = headers
        .get("idempotency-key")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "Idempotency-Key is required",
            )
        })?;
    let expected_revision = headers
        .get("if-match")
        .and_then(|value| value.trim_matches('"').parse::<u64>().ok())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "If-Match exact Delegation revision is required",
            )
        })?;
    let now = now_unix_ms()?;
    let control_actor = AuthenticatedActor {
        company_id: control.company_id().into(),
        actor_id: format!("control-plane:{generation}"),
        actor_kind: harness_fabric::ActorKind::Service,
        // This is the server's exact Control Plane generation, not the
        // browser credential. It owns transport submission while the
        // resolved business actor remains separately frozen in the closed
        // collaboration envelope.
        role_bindings: BTreeSet::from(["company_control_plane".into(), "fabric_submit".into()]),
        session_id: format!("control-plane:{generation}"),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now.saturating_add(30_000),
    };
    let path_parts = suffix.split('/').collect::<Vec<_>>();
    if method == "POST"
        && path_parts.len() == 3
        && path_parts[1] == "artifacts"
        && path_parts[2] == "initiate"
    {
        let delegation_id = path_parts[0];
        let request = serde_json::from_slice::<CollaborationArtifactInitiateHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        let delegation = store
            .collaboration_delegation(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        if delegation.revision != expected_revision
            || !matches!(
                delegation.state,
                harness_core::collaboration::DelegationState::Active
                    | harness_core::collaboration::DelegationState::ResultAvailable
            )
            || credential.actor != delegation.target_host_ref
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "only the exact target Host may initiate an artifact for current active Delegation",
            ));
        }
        let target_work_ref = delegation.target_work_ref.as_ref().ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ExpectedRevisionConflict,
                "active Delegation has no exact target Work",
            )
        })?;
        let attestation = store
            .collaboration_source_work_attestation(
                control.company_id(),
                &delegation.source_work_attestation_id,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "Delegation lacks source Host attestation",
                )
            })?;
        let writer = AuthenticatedActor {
            company_id: control.company_id().into(),
            actor_id: credential.actor.id,
            actor_kind: harness_fabric::ActorKind::AgentMember,
            role_bindings: BTreeSet::from(["artifact_write".into()]),
            session_id: format!("collaboration-artifact-init:{idempotency_key}"),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(30_000),
        };
        if let Some(existing) = control.artifact_manifest(&request.artifact_id)? {
            let exact = existing.company_id == control.company_id()
                && existing.source_node_id == delegation.target_placement.node_id
                && existing.source_team_id.as_deref()
                    == Some(delegation.target_placement.team_id.as_str())
                && existing.source_work_id.as_deref() == Some(target_work_ref.work_id.as_str())
                && existing.operation_id == request.operation_id
                && existing.media_type == request.media_type
                && existing.size_bytes == request.size_bytes
                && existing.sha256 == request.sha256
                && existing.classification == request.classification
                && existing.initiator == writer.actor_id
                && existing.authorized_readers
                    == BTreeSet::from([attestation.source_host_ref.id.clone()]);
            if !exact {
                return Err(FabricError::none(
                    FabricErrorCode::IdempotencyConflict,
                    "artifact identity was reused with a different collaboration scope or payload",
                ));
            }
            let (manifest, upload_capability) = control.replay_collaboration_upload_capability(
                &writer,
                generation,
                &request.artifact_id,
                now,
            )?;
            return Ok(serde_json::json!({
                "delegation_id": delegation_id,
                "manifest": manifest,
                "upload_capability": upload_capability,
                "replayed": true,
            }));
        }
        let (manifest, upload_capability) = control.initiate_collaboration_artifact(
            &writer,
            generation,
            &request.artifact_id,
            &delegation.target_placement.node_id,
            &delegation.target_placement.team_id,
            &target_work_ref.work_id,
            request.operation_id.as_deref(),
            &request.media_type,
            request.size_bytes,
            &request.sha256,
            request.classification,
            BTreeSet::from([attestation.source_host_ref.id]),
            now,
        )?;
        return Ok(serde_json::json!({
            "delegation_id": delegation_id,
            "manifest": manifest,
            "upload_capability": upload_capability,
        }));
    }
    if method == "POST"
        && path_parts.len() == 4
        && path_parts[1] == "artifacts"
        && path_parts[3] == "grant"
    {
        let delegation_id = path_parts[0];
        let artifact_id = path_parts[2];
        let request = serde_json::from_slice::<CollaborationArtifactGrantHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        return grant_collaboration_artifact(
            &store,
            control,
            generation,
            delegation_id,
            artifact_id,
            &request,
            &credential.actor,
            &control_actor,
            idempotency_key,
            expected_revision,
            now,
        );
    }
    if method == "POST"
        && path_parts.len() == 4
        && path_parts[1] == "artifacts"
        && path_parts[3] == "retention"
    {
        let delegation_id = path_parts[0];
        let artifact_id = path_parts[2];
        let request = serde_json::from_slice::<CollaborationArtifactRetentionHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        const MAX_COLLABORATION_RETENTION_MS: u64 = 365 * 24 * 60 * 60 * 1_000;
        if request.retention_duration_ms == 0
            || request.retention_duration_ms > MAX_COLLABORATION_RETENTION_MS
        {
            return Err(FabricError::none(
                FabricErrorCode::ArtifactInvalid,
                "retention duration must be positive and within the server one-year policy",
            ));
        }
        let delegation = store
            .collaboration_delegation(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        if delegation.revision != expected_revision
            || credential.actor != delegation.target_host_ref
            || delegation.state != harness_core::collaboration::DelegationState::Terminal
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "retention requires exact target Host and terminal Delegation revision",
            ));
        }
        let manifest = control.artifact_manifest(artifact_id)?.ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ArtifactInvalid,
                "artifact manifest does not exist",
            )
        })?;
        if manifest.company_id != control.company_id()
            || manifest.source_node_id != delegation.target_placement.node_id
            || manifest.source_team_id.as_deref()
                != Some(delegation.target_placement.team_id.as_str())
            || manifest.source_work_id.as_deref()
                != delegation
                    .target_work_ref
                    .as_ref()
                    .map(|work| work.work_id.as_str())
            || manifest.deleted_at_unix_ms.is_some()
        {
            return Err(FabricError::none(
                FabricErrorCode::ArtifactInvalid,
                "artifact retention scope disagrees with the exact terminal Delegation",
            ));
        }
        let artifact_import = store
            .collaboration_artifact_import(control.company_id(), artifact_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .filter(|import| {
                import.delegation_id == delegation_id
                    && import.source_node_id == delegation.source_node_id
                    && import.source_team_id == delegation.source_team_id
                    && import.source_work_ref == delegation.source_work_ref
                    && import.artifact_digest == manifest.sha256
                    && import.size_bytes == manifest.size_bytes
            })
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::RecoveryRequired,
                    "artifact retention requires one exact canonical source-owned ArtifactImport",
                )
            })?;
        let retain_until = harness_core::collaboration::CollaborationRetentionAnchor {
            terminal_transport_at_unix_ms: manifest.completed_at_unix_ms,
            terminal_delegation_at_unix_ms: unix_ms_timestamp(&delegation.updated_at),
            source_import_completed_at_unix_ms: Some(artifact_import.imported_at_unix_ms),
        }
        .retain_until_unix_ms(request.retention_duration_ms)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::ArtifactInvalid,
                "retention cannot start until transport, Delegation, and source durable import are all terminal",
            )
        })?;
        let writer = AuthenticatedActor {
            company_id: control.company_id().into(),
            actor_id: credential.actor.id,
            actor_kind: harness_fabric::ActorKind::AgentMember,
            role_bindings: BTreeSet::from(["artifact_write".into()]),
            session_id: format!("collaboration-artifact-retention:{idempotency_key}"),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now.saturating_add(30_000),
        };
        let manifest = control.schedule_collaboration_artifact_retention(
            &writer,
            generation,
            artifact_id,
            request.expected_artifact_revision,
            retain_until,
            now,
        )?;
        return Ok(serde_json::json!({
            "delegation_id": delegation_id,
            "manifest": manifest,
            "retention_start": retain_until.saturating_sub(request.retention_duration_ms),
        }));
    }
    if method == "POST" && suffix.ends_with("/cancellation-requests") {
        let delegation_id = suffix
            .strip_suffix("/cancellation-requests")
            .filter(|value| !value.is_empty() && !value.contains('/'))
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "cancellation request path is malformed",
                )
            })?;
        let request = serde_json::from_slice::<CollaborationCancellationRequestHttpRequest>(body)
            .map_err(|error| {
            FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
        })?;
        if request.reason.trim().is_empty()
            || request.target_execution_space_id.trim().is_empty()
            || request.expires_unix_ms <= now
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "cancellation request requires reason, exact target Execution Space, and future expiry",
            ));
        }
        let delegation = store
            .collaboration_delegation(control.company_id(), delegation_id)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ExpectedRevisionConflict,
                    "Delegation does not exist",
                )
            })?;
        let attestation = store
            .collaboration_source_work_attestation(
                control.company_id(),
                &delegation.source_work_attestation_id,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::StoreUnavailable, error.to_string())
            })?
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "Delegation lacks its server-authored source Work attestation",
                )
            })?;
        let authority = harness_store::ResolvedCollaborationAuthority {
            source_host: attestation.source_host_ref,
            source_work_owner: attestation.source_owner_ref,
            target_host: delegation.target_host_ref.clone(),
            target_placement: delegation.target_placement.clone(),
            source_work_application_service: attestation.work_application_service_ref,
            source_gateway_generation: attestation.source_gateway_generation,
        };
        let cancellation = harness_core::collaboration::DelegationCancellationRequest {
            id: format!("delegation-cancellation-request:{idempotency_key}"),
            delegation_id: delegation_id.into(),
            expected_delegation_revision: expected_revision,
            requested_by: credential.actor.clone(),
            reason: request.reason,
            state: harness_core::collaboration::CancellationRequestState::Pending,
            target_host_decision_ref: None,
            revision: 1,
            created_at: format!("unix-ms:{now}"),
            updated_at: format!("unix-ms:{now}"),
        };
        let mutation_context = harness_store::CollaborationMutationContext {
            company_id: control.company_id().into(),
            authenticated_actor: credential.actor.clone(),
            command_name: "delegation_cancel_request".into(),
            idempotency_key: idempotency_key.clone(),
            expected_revision,
            occurred_at: format!("unix-ms:{now}"),
        };
        let pending = store
            .request_delegation_cancellation(&mutation_context, &cancellation, &authority)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::ExpectedRevisionConflict, error.to_string())
            })?;
        let business = store
            .delegation_cancel_request_operation(
                &harness_store::CollaborationMutationContext {
                    expected_revision: pending.projection.revision,
                    command_name: "delegation_cancel_request_route".into(),
                    ..mutation_context
                },
                &cancellation,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::ExpectedRevisionConflict, error.to_string())
            })?;
        let routed_actor = AuthenticatedActor {
            expires_at_unix_ms: request.expires_unix_ms,
            ..control_actor.clone()
        };
        let routed = harness_store::route_collaboration_business_operation(
            &business,
            &harness_store::CollaborationFabricRouteContext {
                authenticated_actor: routed_actor.clone(),
                resolved_business_actor: credential.actor,
                source: harness_store::CollaborationFabricSource::ControlPlane,
                control_plane_generation: generation,
                target_execution_space_id: Some(request.target_execution_space_id),
                created_at_unix_ms: now,
                expires_at_unix_ms: request.expires_unix_ms,
            },
        )?;
        let (_, _, receipt, replayed) =
            control.accept_control_plane_operation(generation, &routed_actor, routed, now)?;
        return Ok(serde_json::json!({
            "delegation": pending.projection,
            "cancellation_request": cancellation,
            "operation_id": receipt.operation_id,
            "receipt": receipt,
            "replayed": pending.replayed || replayed,
        }));
    }
    let cancellation_parts = path_parts;
    if method == "POST"
        && cancellation_parts.len() == 4
        && cancellation_parts[1] == "cancellation-requests"
        && matches!(cancellation_parts[3], "accept" | "reject")
    {
        let delegation_id = cancellation_parts[0];
        let request_id = cancellation_parts[2];
        let action = cancellation_parts[3];
        let request = serde_json::from_slice::<CollaborationCancellationDecisionHttpRequest>(body)
            .map_err(|error| {
                FabricError::none(FabricErrorCode::InvalidPayload, error.to_string())
            })?;
        if request.reason.trim().is_empty()
            || request.native_work_event_ref.trim().is_empty()
            || request.target_execution_space_id.trim().is_empty()
            || request.expires_unix_ms <= now
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "cancellation decision requires reason, native Work event, exact target Execution Space, and future expiry",
            ));
        }
        let decision = harness_core::collaboration::DelegationCancellationDecision {
            id: format!("delegation-cancellation-decision:{idempotency_key}"),
            cancellation_request_id: request_id.into(),
            expected_request_revision: 1,
            decision: if action == "accept" {
                harness_core::collaboration::CancellationDecisionKind::Accept
            } else {
                harness_core::collaboration::CancellationDecisionKind::Reject
            },
            decided_by_target_host: credential.actor.clone(),
            native_work_event_ref: request.native_work_event_ref,
            reason: request.reason,
            created_at: format!("unix-ms:{now}"),
        };
        let business = store
            .delegation_cancel_decide_operation(
                &harness_store::CollaborationMutationContext {
                    company_id: control.company_id().into(),
                    authenticated_actor: credential.actor.clone(),
                    command_name: "delegation_cancel_decide".into(),
                    idempotency_key: idempotency_key.clone(),
                    expected_revision,
                    occurred_at: format!("unix-ms:{now}"),
                },
                delegation_id,
                request_id,
                &decision,
            )
            .map_err(|error| {
                FabricError::none(FabricErrorCode::ExpectedRevisionConflict, error.to_string())
            })?;
        let routed_actor = AuthenticatedActor {
            expires_at_unix_ms: request.expires_unix_ms,
            ..control_actor.clone()
        };
        let routed = harness_store::route_collaboration_business_operation(
            &business,
            &harness_store::CollaborationFabricRouteContext {
                authenticated_actor: routed_actor.clone(),
                resolved_business_actor: credential.actor,
                source: harness_store::CollaborationFabricSource::ControlPlane,
                control_plane_generation: generation,
                target_execution_space_id: Some(request.target_execution_space_id),
                created_at_unix_ms: now,
                expires_at_unix_ms: request.expires_unix_ms,
            },
        )?;
        let (_, _, receipt, replayed) =
            control.accept_control_plane_operation(generation, &routed_actor, routed, now)?;
        return Ok(serde_json::json!({
            "delegation_id": delegation_id,
            "cancellation_request_id": request_id,
            "operation_id": receipt.operation_id,
            "receipt": receipt,
            "replayed": replayed,
        }));
    }
    let (delegation_id, action) = suffix.rsplit_once('/').ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "collaboration mutation path is malformed",
        )
    })?;
    if method != "POST" || !matches!(action, "accept" | "reject") {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "unknown collaboration mutation",
        ));
    }
    let request = serde_json::from_slice::<CollaborationDecisionHttpRequest>(body)
        .map_err(|error| FabricError::none(FabricErrorCode::InvalidPayload, error.to_string()))?;
    if request.reason.trim().is_empty()
        || request.target_execution_space_id.trim().is_empty()
        || request.expires_unix_ms <= now
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "decision requires reason, exact target Execution Space, and future expiry",
        ));
    }
    let decision = harness_core::collaboration::DelegationDecision {
        id: format!("delegation-decision:{idempotency_key}"),
        delegation_id: delegation_id.into(),
        expected_delegation_revision: expected_revision,
        decision: if action == "accept" {
            harness_core::collaboration::DelegationDecisionKind::Accept
        } else {
            harness_core::collaboration::DelegationDecisionKind::Reject
        },
        decided_by_target_host: credential.actor.clone(),
        reason: request.reason,
        created_at: format!("unix-ms:{now}"),
    };
    let business = store
        .delegation_decide_operation(
            &harness_store::CollaborationMutationContext {
                company_id: control.company_id().into(),
                authenticated_actor: credential.actor.clone(),
                command_name: "delegation_decide".into(),
                idempotency_key: idempotency_key.clone(),
                expected_revision,
                occurred_at: format!("unix-ms:{now}"),
            },
            delegation_id,
            &decision,
        )
        .map_err(|error| {
            FabricError::none(FabricErrorCode::ExpectedRevisionConflict, error.to_string())
        })?;
    let control_actor = AuthenticatedActor {
        expires_at_unix_ms: request.expires_unix_ms,
        ..control_actor
    };
    let routed = harness_store::route_collaboration_business_operation(
        &business,
        &harness_store::CollaborationFabricRouteContext {
            authenticated_actor: control_actor.clone(),
            resolved_business_actor: credential.actor,
            source: harness_store::CollaborationFabricSource::ControlPlane,
            control_plane_generation: generation,
            target_execution_space_id: Some(request.target_execution_space_id),
            created_at_unix_ms: now,
            expires_at_unix_ms: request.expires_unix_ms,
        },
    )?;
    let (_, _, receipt, replayed) =
        control.accept_control_plane_operation(generation, &control_actor, routed, now)?;
    Ok(serde_json::json!({
        "delegation_id": delegation_id,
        "operation_id": receipt.operation_id,
        "receipt": receipt,
        "replayed": replayed,
    }))
}

#[allow(clippy::too_many_arguments)]
fn route_host_http<K: harness_fabric::ArtifactKeyBackend>(
    method: &str,
    path: &str,
    target: &str,
    body: &serde_json::Value,
    actor: Option<&AuthenticatedActor>,
    control: &ControlPlane<'_, K>,
    generation: u64,
    ca: &harness_fabric::pki::FabricCaMaterial,
    now: u64,
    host_token: &str,
) -> Result<serde_json::Value, FabricError> {
    let required_actor = || {
        actor.ok_or_else(|| {
            FabricError::none(FabricErrorCode::UnauthorizedActor, "Host actor is required")
        })
    };
    if method == "POST" && path == "/v1/fabric/enrollments" {
        reject_unknown_json_fields(
            body,
            &[
                "enrollment_id",
                "requested_name",
                "allowed_capabilities",
                "authorized_node_daemon_id",
                "authorized_node_daemon_generation",
                "expires_at_unix_ms",
            ],
        )?;
        let actor = required_actor()?;
        let enrollment_id = json_string(body, "enrollment_id")?;
        let requested_name = json_string(body, "requested_name")?;
        let capabilities = json_string_set(body, "allowed_capabilities")?;
        let authorized_node_daemon_id = json_string(body, "authorized_node_daemon_id")?;
        let authorized_node_daemon_generation =
            json_u64(body, "authorized_node_daemon_generation")?;
        let expires_at = body
            .get("expires_at_unix_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| now.saturating_add(10 * 60 * 1000));
        let raw_token = format!(
            "enroll-{}",
            harness_fabric::sha256_hex(format!("{host_token}:{enrollment_id}:{now}").as_bytes())
        );
        let enrollment = control.create_enrollment_bound(
            actor,
            generation,
            &enrollment_id,
            &raw_token,
            &requested_name,
            capabilities,
            &authorized_node_daemon_id,
            authorized_node_daemon_generation,
            expires_at,
            now,
        )?;
        return Ok(serde_json::json!({"enrollment":enrollment,"raw_token":raw_token}));
    }
    if method == "POST" && path == "/v1/fabric/nodes/enroll" {
        reject_unknown_json_fields(
            body,
            &[
                "raw_token",
                "node_id",
                "display_name",
                "csr_pem",
                "schema_bundle_digest",
            ],
        )?;
        let raw_token = json_string(body, "raw_token")?;
        let node_id = json_string(body, "node_id")?;
        let display_name = json_string(body, "display_name")?;
        let csr_pem = json_string(body, "csr_pem")?;
        let claimed_schema_digest = json_string(body, "schema_bundle_digest")?;
        let schema_digest = remote_fabric_schema_bundle_digest();
        if claimed_schema_digest != schema_digest {
            return Err(FabricError::none(
                FabricErrorCode::SchemaIncompatible,
                "Node enrollment schema digest does not match the Control Plane's actual compiled schema bundle",
            ));
        }
        harness_fabric::pki::verify_node_csr(&csr_pem, control.company_id(), &node_id)?;
        let issued = harness_fabric::pki::issue_node_certificate(
            ca,
            &csr_pem,
            control.company_id(),
            &node_id,
            now,
        )?;
        let (node, certificate) = control.consume_enrollment_csr(
            generation,
            &raw_token,
            &node_id,
            &display_name,
            &csr_pem,
            &issued.serial,
            issued.expires_at_unix_ms,
            &schema_digest,
            now,
        )?;
        return Ok(serde_json::json!({
            "node":node,
            "certificate":certificate,
            "client_certificate_pem":issued.certificate_pem,
            "company_ca_pem":ca.certificate_pem,
        }));
    }
    let state = control.store().snapshot()?;
    if method == "GET" && path == "/v1/fabric/nodes" {
        let limit = query_value(target, "limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50)
            .clamp(1, 200);
        let cursor = query_value(target, "cursor");
        let status = query_value(target, "status");
        let mut nodes = state.nodes.values().cloned().collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.id.cmp(&right.id));
        let cursor_node_id = cursor
            .as_ref()
            .map(|cursor| {
                nodes
                    .iter()
                    .find(|node| {
                        fabric_node_cursor(control.company_id(), &node.id, state.revision)
                            == *cursor
                    })
                    .map(|node| node.id.clone())
                    .ok_or_else(|| {
                        FabricError::none(
                            FabricErrorCode::ExpectedRevisionConflict,
                            "Fabric node cursor is invalid or belongs to an older snapshot",
                        )
                    })
            })
            .transpose()?;
        nodes.retain(|node| {
            cursor_node_id
                .as_ref()
                .is_none_or(|cursor_node_id| node.id > *cursor_node_id)
                && status.as_ref().is_none_or(|status| {
                    format!("{:?}", node.administrative_status).to_ascii_lowercase() == *status
                })
        });
        let page = nodes.into_iter().take(limit).collect::<Vec<_>>();
        let next_cursor = page
            .last()
            .map(|node| fabric_node_cursor(control.company_id(), &node.id, state.revision));
        let diagnostics = harness_fabric::diagnostics::inspect_fabric(
            control.store(),
            control.company_id(),
            now,
        )?;
        return Ok(serde_json::json!({
            "nodes":page,
            "next_cursor":next_cursor,
            "diagnostics":diagnostics,
        }));
    }
    if method == "GET" {
        if let Some(artifact_id) = path
            .strip_prefix("/v1/fabric/artifacts/")
            .and_then(|rest| rest.strip_suffix("/download-capability"))
        {
            let node_id = query_value(target, "node_id").ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    "download-capability requires node_id",
                )
            })?;
            let capability = control.issue_download_capability(
                required_actor()?,
                generation,
                artifact_id,
                &node_id,
                now,
            )?;
            return Ok(serde_json::json!({"download_capability":capability}));
        }
        if let Some(node_id) = path.strip_prefix("/v1/fabric/nodes/") {
            let node = state.nodes.get(node_id).cloned().ok_or_else(|| {
                FabricError::none(FabricErrorCode::TargetNotPlaced, "Node does not exist")
            })?;
            let lease = state.gateway_leases.get(node_id);
            let diagnostic = harness_fabric::diagnostics::inspect_fabric(
                control.store(),
                control.company_id(),
                now,
            )?
            .nodes
            .into_iter()
            .find(|diagnostic| diagnostic.node_id == node_id);
            return Ok(serde_json::json!({
                "node":node,
                "gateway_lease":lease,
                "connection_status":node.connection_status(lease, generation, now),
                "diagnostic":diagnostic,
            }));
        }
        if let Some(operation_id) = path.strip_prefix("/v1/fabric/operations/") {
            let operation = state.operations.get(operation_id).cloned().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "routed operation does not exist",
                )
            })?;
            let attempts = state
                .attempts
                .values()
                .filter(|attempt| attempt.operation_id == operation_id)
                .cloned()
                .collect::<Vec<_>>();
            let receipts = state
                .receipts
                .values()
                .filter(|receipt| receipt.operation_id == operation_id)
                .cloned()
                .collect::<Vec<_>>();
            return Ok(
                serde_json::json!({"operation":operation,"attempts":attempts,"receipts":receipts}),
            );
        }
    }
    if method == "POST" {
        if let Some(node_id) = path
            .strip_prefix("/v1/fabric/nodes/")
            .and_then(|rest| rest.strip_suffix("/certificate/rotate"))
        {
            reject_unknown_json_fields(
                body,
                &["expected_revision", "current_certificate_serial", "csr_pem"],
            )?;
            let actor = required_actor()?;
            actor.require_company_and_role(control.company_id(), "company_host", now)?;
            let expected_revision = json_u64(body, "expected_revision")?;
            let current_certificate_serial = json_string(body, "current_certificate_serial")?;
            let csr_pem = json_string(body, "csr_pem")?;
            let current_gateway = state.gateway_leases.get(node_id).cloned().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::NodeStaleGeneration,
                    "certificate rotation requires the exact current Gateway/NodeDaemon authority",
                )
            })?;
            let issued = harness_fabric::pki::issue_node_certificate(
                ca,
                &csr_pem,
                control.company_id(),
                node_id,
                now,
            )?;
            let (node, certificate) = control.rotate_node_certificate_csr(
                generation,
                node_id,
                current_gateway.gateway_generation,
                &current_gateway.node_daemon_id,
                current_gateway.node_daemon_generation,
                &current_certificate_serial,
                &issued.serial,
                expected_revision,
                &csr_pem,
                issued.expires_at_unix_ms,
                now,
            )?;
            return Ok(serde_json::json!({
                "node":node,
                "certificate":certificate,
                "client_certificate_pem":issued.certificate_pem,
                "company_ca_pem":ca.certificate_pem,
            }));
        }
        if let Some(node_id) = path
            .strip_prefix("/v1/fabric/nodes/")
            .and_then(|rest| rest.strip_suffix("/drain"))
        {
            reject_unknown_json_fields(body, &["expected_revision"])?;
            let revision = json_u64(body, "expected_revision")?;
            let node = control.set_node_administrative_status(
                required_actor()?,
                generation,
                node_id,
                revision,
                NodeAdministrativeStatus::Draining,
                now,
            )?;
            return Ok(serde_json::json!({"node":node}));
        }
        if let Some(node_id) = path
            .strip_prefix("/v1/fabric/nodes/")
            .and_then(|rest| rest.strip_suffix("/revoke"))
        {
            reject_unknown_json_fields(body, &["expected_revision", "reason"])?;
            let revision = json_u64(body, "expected_revision")?;
            let reason = json_string(body, "reason")?;
            let node = control.revoke_node(
                required_actor()?,
                generation,
                node_id,
                revision,
                &reason,
                now,
            )?;
            return Ok(serde_json::json!({"node":node}));
        }
        if path == "/v1/fabric/artifacts/initiate" {
            reject_unknown_json_fields(
                body,
                &[
                    "artifact_id",
                    "source_node_id",
                    "operation_id",
                    "media_type",
                    "size_bytes",
                    "sha256",
                    "classification",
                    "authorized_readers",
                ],
            )?;
            let classification = match json_string(body, "classification")?.as_str() {
                "company_internal" => ArtifactClassification::CompanyInternal,
                "sensitive" => ArtifactClassification::Sensitive,
                _ => {
                    return Err(FabricError::none(
                        FabricErrorCode::ArtifactInvalid,
                        "classification must be company_internal|sensitive",
                    ))
                }
            };
            let (manifest, capability) = control.initiate_artifact(
                required_actor()?,
                generation,
                &json_string(body, "artifact_id")?,
                &json_string(body, "source_node_id")?,
                body.get("operation_id").and_then(serde_json::Value::as_str),
                &json_string(body, "media_type")?,
                json_u64(body, "size_bytes")?,
                &json_string(body, "sha256")?,
                classification,
                json_string_set(body, "authorized_readers")?,
                now,
            )?;
            return Ok(serde_json::json!({"manifest":manifest,"upload_capability":capability}));
        }
        if let Some(artifact_id) = path
            .strip_prefix("/v1/fabric/artifacts/")
            .and_then(|rest| rest.strip_suffix("/complete"))
        {
            reject_unknown_json_fields(body, &["capability", "bytes_hex"])?;
            let capability: harness_fabric::ArtifactCapability =
                serde_json::from_value(body.get("capability").cloned().ok_or_else(|| {
                    FabricError::none(FabricErrorCode::CapabilityInvalid, "capability is required")
                })?)
                .map_err(|error| {
                    FabricError::none(FabricErrorCode::CapabilityInvalid, error.to_string())
                })?;
            if capability.artifact_id != artifact_id {
                return Err(FabricError::none(
                    FabricErrorCode::CapabilityInvalid,
                    "artifact path and capability disagree",
                ));
            }
            let bytes = decode_hex(&json_string(body, "bytes_hex")?)?;
            let manifest = control.complete_artifact(generation, &capability, &bytes, now)?;
            return Ok(serde_json::json!({"manifest":manifest}));
        }
    }
    Err(FabricError::none(
        FabricErrorCode::InvalidPayload,
        "unknown Remote Fabric Host REST endpoint",
    ))
}

fn fabric_node_cursor(company_id: &str, node_id: &str, snapshot_revision: u64) -> String {
    harness_fabric::sha256_hex(format!(
        "agentfirm.remote-fabric.node-cursor.v1\n{company_id}\n{node_id}\n{snapshot_revision}"
    ))
}

fn respond_fabric_error(
    stream: &mut TcpStream,
    error: FabricError,
    origin: Option<&str>,
) -> CliResult<()> {
    let status = match error.code {
        FabricErrorCode::UnauthorizedActor | FabricErrorCode::WrongCompany => "403 Forbidden",
        FabricErrorCode::TargetNotPlaced | FabricErrorCode::OperationUnknown => "404 Not Found",
        FabricErrorCode::ExpectedRevisionConflict | FabricErrorCode::IdempotencyConflict => {
            "409 Conflict"
        }
        FabricErrorCode::StoreUnavailable => "503 Service Unavailable",
        _ => "400 Bad Request",
    };
    write_http_json(
        stream,
        status,
        &serde_json::json!({"ok":false,"error":error}),
        origin,
    )
}

fn write_http_json(
    stream: &mut TcpStream,
    status: &str,
    value: &serde_json::Value,
    origin: Option<&str>,
) -> CliResult<()> {
    let body = serde_json::to_vec(value)?;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
        body.len()
    )?;
    if let Some(origin) = origin {
        write!(
            stream,
            "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, If-Match\r\n"
        )?;
    }
    write!(stream, "Connection: close\r\n\r\n")?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

fn json_string(value: &serde_json::Value, key: &str) -> Result<String, FabricError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("{key} is required"),
            )
        })
}

fn reject_unknown_json_fields(
    value: &serde_json::Value,
    allowed: &[&str],
) -> Result<(), FabricError> {
    let object = value.as_object().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            "Remote Fabric mutation body must be a JSON object",
        )
    })?;
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("unknown Remote Fabric mutation field: {field}"),
        ));
    }
    Ok(())
}

fn constant_time_secret_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn json_u64(value: &serde_json::Value, key: &str) -> Result<u64, FabricError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("{key} is required"),
            )
        })
}

fn json_string_set(
    value: &serde_json::Value,
    key: &str,
) -> Result<std::collections::BTreeSet<String>, FabricError> {
    let values = value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("{key} must be an array"),
            )
        })?;
    let result = values
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect::<Option<std::collections::BTreeSet<_>>>()
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("{key} must contain only strings"),
            )
        })?;
    if result.is_empty() {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("{key} cannot be empty"),
        ));
    }
    Ok(result)
}

fn query_value(target: &str, key: &str) -> Option<String> {
    target.split('?').nth(1)?.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.to_string())
    })
}

fn decode_hex(raw: &str) -> Result<Vec<u8>, FabricError> {
    if !raw.len().is_multiple_of(2) || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(FabricError::none(
            FabricErrorCode::ArtifactInvalid,
            "bytes_hex must contain an even number of hexadecimal characters",
        ));
    }
    (0..raw.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&raw[index..index + 2], 16).map_err(|_| {
                FabricError::none(FabricErrorCode::ArtifactInvalid, "bytes_hex is invalid")
            })
        })
        .collect()
}

enum ResolvedNodeTls {
    Files(NodeTlsIdentityFiles),
    Material(NodeTlsIdentityMaterial),
}

struct ResolvedNodeCredentials {
    tls: ResolvedNodeTls,
    certificate_serial: String,
    public_key_fingerprint: String,
}

fn resolve_node_credentials(
    args: &[String],
    company_id: &str,
    node_id: &str,
) -> CliResult<ResolvedNodeCredentials> {
    match value(args, "--credential-backend")
        .unwrap_or_else(|| "file".into())
        .as_str()
    {
        "file" => Ok(ResolvedNodeCredentials {
            tls: ResolvedNodeTls::Files(NodeTlsIdentityFiles {
                client_certificate_chain_pem: required_path(args, "--client-cert")?,
                client_private_key_pem: required_path(args, "--client-key")?,
                control_plane_ca_pem: required_path(args, "--control-plane-ca")?,
            }),
            certificate_serial: required(args, "--certificate-serial")?,
            public_key_fingerprint: required(args, "--public-key-fingerprint")?,
        }),
        "macos-keychain" => {
            let service = required(args, "--keychain-service")?;
            if service.trim().is_empty() || service.len() > 128 {
                return Err(CliError::Usage(
                    "--keychain-service must be a bounded non-empty service name".into(),
                ));
            }
            // Validate the public enrolled identity before touching Keychain.
            // Besides failing closed, this prevents avoidable ACL prompts when
            // an incomplete login-agent command is installed.
            let certificate_serial = required(args, "--certificate-serial")?;
            let public_key_fingerprint = required(args, "--public-key-fingerprint")?;
            let prefix = format!("{company_id}:{node_id}");
            Ok(ResolvedNodeCredentials {
                tls: ResolvedNodeTls::Material(NodeTlsIdentityMaterial {
                    client_certificate_chain_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:client-certificate"),
                    )?
                    .into_bytes(),
                    client_private_key_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:client-private-key"),
                    )?
                    .into_bytes(),
                    control_plane_ca_pem: keychain_secret(
                        &service,
                        &format!("{prefix}:control-plane-ca"),
                    )?
                    .into_bytes(),
                }),
                // Serial and fingerprint are public certificate identity, not
                // secret key material. Requiring five separate Keychain ACL
                // prompts made a login LaunchAgent repeatedly block on user
                // interaction. Keep only the three PEM materials in Keychain;
                // the explicit public values are still checked by the
                // generation-fenced mTLS welcome against the enrolled Node.
                certificate_serial,
                public_key_fingerprint,
            })
        }
        _ => Err(CliError::Usage(
            "--credential-backend must be file|macos-keychain".into(),
        )),
    }
}

#[cfg(target_os = "macos")]
fn keychain_secret(service: &str, account: &str) -> CliResult<String> {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-w", "-s", service, "-a", account])
        .output()?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "macOS Keychain item is unavailable for service={service} account={account}"
        )));
    }
    let secret = String::from_utf8(output.stdout)
        .map_err(|_| CliError::Usage("macOS Keychain item is not valid UTF-8".into()))?;
    let secret = secret.trim().to_string();
    if secret.is_empty() {
        return Err(CliError::Usage(format!(
            "macOS Keychain item is empty for service={service} account={account}"
        )));
    }
    Ok(secret)
}

#[cfg(not(target_os = "macos"))]
fn keychain_secret(_service: &str, _account: &str) -> CliResult<String> {
    Err(CliError::Usage(
        "macos-keychain credential backend is available only on macOS".into(),
    ))
}

pub(crate) fn firm_home(resolved: &ResolvedStore, args: &[String]) -> CliResult<PathBuf> {
    if let Some(path) = value(args, "--firm-home") {
        return Ok(PathBuf::from(path));
    }
    if let Some(space) = &resolved.execution_space_context {
        return firm_home_from_execution_space_root(&space.store_root);
    }
    super::execution_space::firm_home().map_err(|error| CliError::Usage(error.to_string()))
}

fn firm_home_from_execution_space_root(store_root: &Path) -> CliResult<PathBuf> {
    let execution_spaces = store_root
        .parent()
        .ok_or_else(|| CliError::Usage("cannot derive FIRM_HOME from Execution Space".into()))?;
    if execution_spaces.file_name().and_then(|name| name.to_str()) != Some("execution-spaces") {
        return Err(CliError::Usage(
            "Execution Space store must be a direct child of FIRM_HOME/execution-spaces".into(),
        ));
    }
    execution_spaces
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| CliError::Usage("cannot derive FIRM_HOME from Execution Space".into()))
}

fn required(args: &[String], name: &str) -> CliResult<String> {
    value(args, name).ok_or_else(|| CliError::Usage(format!("missing required {name}")))
}

fn required_path(args: &[String], name: &str) -> CliResult<PathBuf> {
    Ok(PathBuf::from(required(args, name)?))
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn required_key_file(args: &[String], name: &str) -> CliResult<[u8; 32]> {
    let raw = required_secret_file(args, name)?;
    let raw = raw.trim();
    if raw.len() != 64 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Usage(format!(
            "{name} must contain exactly 64 hexadecimal characters"
        )));
    }
    let mut key = [0u8; 32];
    for (index, output) in key.iter_mut().enumerate() {
        *output = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
            .map_err(|_| CliError::Usage(format!("{name} is invalid")))?;
    }
    Ok(key)
}

fn required_secret_file(args: &[String], name: &str) -> CliResult<String> {
    use std::os::unix::fs::PermissionsExt;
    let path = required_path(args, name)?;
    let metadata = std::fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(CliError::Usage(format!(
            "{name} must be a regular non-symlink file with no group/other permissions"
        )));
    }
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

fn fabric_error(error: FabricError) -> CliError {
    CliError::Usage(format!("REMOTE_FABRIC_{:?}: {}", error.code, error.message))
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_work_attestation_identity_is_stable_per_proposal_not_per_work_revision() {
        let first = super::source_work_attestation_id("work-a", 3, 9, "proposal-a");
        assert_eq!(
            first,
            super::source_work_attestation_id("work-a", 3, 9, "proposal-a")
        );
        assert_ne!(
            first,
            super::source_work_attestation_id("work-a", 3, 9, "proposal-b"),
            "independent proposals for one frozen Work revision need independent immutable attestations"
        );
    }

    use super::*;

    #[test]
    fn collaboration_read_scope_is_closed_to_exact_participants() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-collaboration-read-scope-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut attestation: harness_core::collaboration::SourceWorkAttestation =
            serde_json::from_str(include_str!(
                "../../../schemas/collaboration/fixtures/source-work-attestation/valid/server-authored.json"
            ))
            .unwrap();
        let delegation: harness_core::collaboration::WorkDelegationV1 =
            serde_json::from_str(include_str!(
                "../../../schemas/collaboration/fixtures/work-delegation-v1/valid/awaiting.json"
            ))
            .unwrap();
        attestation.id = delegation.source_work_attestation_id.clone();
        let make_operation = |aggregate_kind: &str,
                              aggregate_id: &str,
                              sequence: u64,
                              projection: serde_json::Value| {
            harness_store::CollaborationOperation {
                store_version: harness_core::collaboration::COLLABORATION_STORE_VERSION.into(),
                company_id: "company-1".into(),
                command_name: "fixture".into(),
                authenticated_actor: harness_core::agentfirm_api::ActorRef {
                    kind: harness_core::agentfirm_api::ActorKind::Service,
                    id: "fixture".into(),
                },
                idempotency_key: format!("fixture-{sequence}"),
                request_fingerprint: format!("sha256:{sequence:064x}"),
                aggregate_kind: aggregate_kind.into(),
                aggregate_id: aggregate_id.into(),
                store_sequence: sequence,
                resulting_revision: 1,
                resulting_projection: projection,
                immutable_side_records: Vec::new(),
                created_at: "t1".into(),
            }
        };
        let rows = [
            make_operation(
                "source_work_attestation",
                &attestation.id,
                1,
                serde_json::to_value(&attestation).unwrap(),
            ),
            make_operation(
                "work_delegation_v1",
                &delegation.id,
                2,
                serde_json::to_value(&delegation).unwrap(),
            ),
        ];
        std::fs::write(
            root.join("agentfirm_collaboration_operations.jsonl"),
            rows.iter()
                .map(|row| serde_json::to_string(row).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();
        let store = HarnessStore::new(&root);
        for (label, actor) in [
            ("source owner", attestation.source_owner_ref.clone()),
            ("source Host", attestation.source_host_ref.clone()),
            ("target Host", delegation.target_host_ref.clone()),
        ] {
            assert!(
                collaboration_actor_can_read_delegation(&store, "company-1", &actor, &delegation)
                    .unwrap(),
                "{label} must see the exact Delegation: {actor:?}"
            );
        }
        assert!(!collaboration_actor_can_read_delegation(
            &store,
            "company-1",
            &harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: "host-from-sibling-team".into(),
            },
            &delegation,
        )
        .unwrap());
        assert!(!collaboration_actor_can_read_delegation(
            &store,
            "company-2",
            &attestation.source_host_ref,
            &delegation,
        )
        .unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn control_plane_message_authority_rejects_every_non_active_or_widened_route() {
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
        attestation.id = delegation.source_work_attestation_id.clone();
        attestation.source_work_ref = delegation.source_work_ref.clone();
        attestation.source_owner_ref = delegation.source_owner_ref.clone();
        policy.revision = delegation.inbound_policy_snapshot.policy_revision;
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
        let mut target_work = delegation.source_work_ref.clone();
        target_work.execution_space_id = "space-b".into();
        target_work.node_id = delegation.target_placement.node_id.clone();
        target_work.team_id = delegation.target_placement.team_id.clone();
        target_work.team_revision = delegation.target_placement.team_revision;
        target_work.work_id = "work-b".into();
        target_work.work_revision = 1;
        delegation.target_work_ref = Some(target_work.clone());
        let mut authority = harness_core::collaboration::CollaborationMessageAuthority {
            company_id: delegation.company_id.clone(),
            delegation_id: delegation.id.clone(),
            delegation_revision: delegation.revision,
            source_work_ref: delegation.source_work_ref.clone(),
            target_work_ref: target_work,
            target_placement: delegation.target_placement.clone(),
            source_owner_ref: delegation.source_owner_ref.clone(),
            source_host_ref: attestation.source_host_ref.clone(),
            target_host_ref: delegation.target_host_ref.clone(),
            inbound_policy_snapshot: delegation.inbound_policy_snapshot.clone(),
            authority_digest: String::new(),
        };
        authority.authority_digest =
            harness_store::canonical_json_fingerprint(&serde_json::json!({
                "company_id": authority.company_id,
                "delegation_id": authority.delegation_id,
                "delegation_revision": authority.delegation_revision,
                "source_work_ref": authority.source_work_ref,
                "target_work_ref": authority.target_work_ref,
                "target_placement": authority.target_placement,
                "source_owner_ref": authority.source_owner_ref,
                "source_host_ref": authority.source_host_ref,
                "target_host_ref": authority.target_host_ref,
                "inbound_policy_snapshot": authority.inbound_policy_snapshot,
            }));
        let reference = harness_fabric::CollaborationBusinessReference {
            business_kind: "team_message_deliver".into(),
            required_capability: "collaboration.team_message_deliver".into(),
            business_actor_kind: "agent_member".into(),
            business_actor_id: attestation.source_host_ref.id.clone(),
            target_team_id: delegation.target_placement.team_id.clone(),
            target_team_revision: delegation.target_placement.team_revision,
            placement_generation: delegation.target_placement.placement_generation,
            expected_revision: delegation.revision,
            payload_digest: format!("sha256:{:064x}", 1),
            payload: serde_json::Value::Null,
        };
        validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &policy,
            &authority,
            &reference,
        )
        .expect("exact active authority");

        for state in [
            harness_core::collaboration::DelegationState::Proposed,
            harness_core::collaboration::DelegationState::AwaitingTargetDecision,
            harness_core::collaboration::DelegationState::CancellationRequested,
            harness_core::collaboration::DelegationState::Terminal,
        ] {
            let mut hostile = delegation.clone();
            hostile.state = state;
            assert!(validate_current_collaboration_message_authority(
                "company-1",
                &hostile,
                &attestation,
                &policy,
                &authority,
                &reference,
            )
            .is_err());
        }
        let mut stale_reference = reference.clone();
        stale_reference.expected_revision -= 1;
        assert!(validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &policy,
            &authority,
            &stale_reference,
        )
        .is_err());
        let mut wrong_actor = reference.clone();
        wrong_actor.business_actor_id = "member-from-source-team-without-work-authority".into();
        assert!(validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &policy,
            &authority,
            &wrong_actor,
        )
        .is_err());
        let mut stale_policy = policy.clone();
        stale_policy.revoked_at = Some("unix-ms:4".into());
        assert!(validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &stale_policy,
            &authority,
            &reference,
        )
        .is_err());
        let mut widened_placement = reference;
        widened_placement.target_team_id = "sibling-target-team".into();
        assert!(validate_current_collaboration_message_authority(
            "company-1",
            &delegation,
            &attestation,
            &policy,
            &authority,
            &widened_placement,
        )
        .is_err());
    }

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

    #[test]
    fn remote_fact_admission_rejects_stale_or_revoked_authority_before_fabric_commit() {
        for case in ["stale-revision", "revoked-policy"] {
            let root = std::env::temp_dir().join(format!(
                "agentfirm-remote-fact-fence-{case}-{}-{}",
                std::process::id(),
                now_unix_ms().unwrap()
            ));
            let (attestation, delegation, mut policy, _publication, mut operation) =
                current_remote_fact_fixture();
            if case == "stale-revision" {
                let mut reference = match operation.closed_body().unwrap() {
                    harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) => {
                        reference
                    }
                    _ => unreachable!(),
                };
                reference.expected_revision = delegation.revision - 1;
                operation.body = serde_json::to_value(reference).unwrap();
                operation.body_digest = harness_store::canonical_json_fingerprint(&operation.body);
            } else {
                policy.revoked_at = Some("unix-ms:9".into());
            }
            seed_current_remote_fact_authority(&root, &attestation, &delegation, &policy);
            let before =
                std::fs::read(root.join("agentfirm_collaboration_operations.jsonl")).unwrap();
            let application = Wave6ControlPlaneApplication {
                collaboration_root: root.clone(),
                company_id: "company-1".into(),
                actor_id: "control-plane".into(),
            };
            let commits = std::sync::atomic::AtomicUsize::new(0);
            let mut accept = || {
                commits.fetch_add(1, Ordering::SeqCst);
                panic!("authority rejection must happen before Fabric commit")
            };
            assert!(application
                .admit_and_accept_source(&operation, &operation.actor, &mut accept,)
                .is_err());
            assert_eq!(commits.load(Ordering::SeqCst), 0);
            assert_eq!(
                std::fs::read(root.join("agentfirm_collaboration_operations.jsonl")).unwrap(),
                before
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn delegation_proposal_admission_rejects_revoked_policy_before_fabric_commit() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-proposal-policy-fence-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        let (mut attestation, delegation, mut policy, _, _) = current_remote_fact_fixture();
        attestation.attestation_digest =
            harness_store::canonical_json_fingerprint(&serde_json::json!({
                "id": attestation.id,
                "company_id": attestation.company_id,
                "source_work_ref": attestation.source_work_ref,
                "source_owner_ref": attestation.source_owner_ref,
                "source_host_ref": attestation.source_host_ref,
                "work_application_service_ref": attestation.work_application_service_ref,
                "source_gateway_generation": attestation.source_gateway_generation,
                "issued_at": attestation.issued_at,
            }));
        policy.revoked_at = Some("unix-ms:9".into());
        seed_current_remote_fact_authority(&root, &attestation, &delegation, &policy);
        let request = harness_store::ProposeDelegationRequest {
            delegation_id: "delegation-policy-fence".into(),
            source_work_attestation_id: attestation.id.clone(),
            target_placement: delegation.target_placement.clone(),
            requested_outcome: "bounded implementation".into(),
            outcome_class: "implementation".into(),
            acceptance_contract: "focused checks".into(),
            operation_id: "proposal-policy-fence".into(),
        };
        let payload = serde_json::json!({
            "request": request,
            "source_work_attestation": attestation,
            "policy_id": policy.id,
        });
        let business = harness_core::collaboration::RoutedBusinessOperation {
            id: "proposal-policy-fence".into(),
            protocol_version: "agentfirm.fabric.v1".into(),
            company_id: "company-1".into(),
            kind: harness_core::collaboration::RoutedBusinessKind::DelegationPropose,
            authenticated_actor: attestation.source_host_ref.clone(),
            source_node_id: attestation.source_work_ref.node_id.clone(),
            target_placement: delegation.target_placement,
            expected_revision: 0,
            idempotency_key: "proposal-policy-fence".into(),
            payload_digest: harness_store::canonical_json_fingerprint(&payload),
            payload,
            required_capability: harness_core::collaboration::RoutedBusinessKind::DelegationPropose
                .required_capability(),
            ordering_key: "delegation:proposal-policy-fence".into(),
            created_at: "unix-ms:10".into(),
        };
        let operation = harness_store::route_collaboration_business_operation(
            &business,
            &harness_store::CollaborationFabricRouteContext {
                authenticated_actor: AuthenticatedActor {
                    company_id: "company-1".into(),
                    actor_id: attestation.source_work_ref.node_id.clone(),
                    actor_kind: harness_fabric::ActorKind::Service,
                    role_bindings: BTreeSet::from(["fabric_submit".into()]),
                    session_id: "daemon-a:1".into(),
                    issued_at_unix_ms: 10,
                    expires_at_unix_ms: 1_000,
                },
                resolved_business_actor: attestation.source_host_ref,
                source: harness_store::CollaborationFabricSource::Node {
                    source_execution_space_id: "space-a".into(),
                    source_gateway_generation: 9,
                    source_node_daemon_id: "daemon-a".into(),
                    source_node_daemon_generation: 1,
                },
                control_plane_generation: 3,
                target_execution_space_id: Some("space-b".into()),
                created_at_unix_ms: 10,
                expires_at_unix_ms: 1_000,
            },
        )
        .unwrap();
        let application = Wave6ControlPlaneApplication {
            collaboration_root: root.clone(),
            company_id: "company-1".into(),
            actor_id: "control-plane".into(),
        };
        let commits = std::sync::atomic::AtomicUsize::new(0);
        let mut accept = || {
            commits.fetch_add(1, Ordering::SeqCst);
            panic!("revoked policy must reject before Fabric commit")
        };
        assert!(application
            .admit_and_accept_source(&operation, &operation.actor, &mut accept)
            .is_err());
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_cancellation_cannot_cross_remote_fact_admission_fence() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-remote-fact-cancel-fence-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        let (attestation, delegation, policy, _publication, operation) =
            current_remote_fact_fixture();
        seed_current_remote_fact_authority(&root, &attestation, &delegation, &policy);
        let entered = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let application_root = root.clone();
        let operation_for_thread = operation.clone();
        let entered_application = entered.clone();
        let release_application = release.clone();
        let admission = std::thread::spawn(move || {
            let application = Wave6ControlPlaneApplication {
                collaboration_root: application_root,
                company_id: "company-1".into(),
                actor_id: "control-plane".into(),
            };
            let mut accept = || {
                entered_application.wait();
                release_application.wait();
                Err(FabricError::unknown(
                    operation_for_thread.id.clone(),
                    "deterministic Fabric commit abort",
                ))
            };
            application.admit_and_accept_source(
                &operation_for_thread,
                &operation_for_thread.actor,
                &mut accept,
            )
        });
        entered.wait();

        let cancellation_finished = Arc::new(AtomicBool::new(false));
        let cancellation_finished_thread = cancellation_finished.clone();
        let cancellation_root = root.clone();
        let cancellation_attestation = attestation.clone();
        let cancellation_delegation = delegation.clone();
        let cancellation = std::thread::spawn(move || {
            let store = HarnessStore::new(cancellation_root);
            let authority = harness_store::ResolvedCollaborationAuthority {
                source_host: cancellation_attestation.source_host_ref.clone(),
                source_work_owner: cancellation_attestation.source_owner_ref.clone(),
                target_host: cancellation_delegation.target_host_ref.clone(),
                target_placement: cancellation_delegation.target_placement.clone(),
                source_work_application_service: cancellation_attestation
                    .work_application_service_ref,
                source_gateway_generation: cancellation_attestation.source_gateway_generation,
            };
            let request = harness_core::collaboration::DelegationCancellationRequest {
                id: "cancel-after-admission".into(),
                delegation_id: cancellation_delegation.id.clone(),
                expected_delegation_revision: cancellation_delegation.revision,
                requested_by: cancellation_attestation.source_host_ref.clone(),
                reason: "stop after authority linearization".into(),
                state: harness_core::collaboration::CancellationRequestState::Pending,
                target_host_decision_ref: None,
                revision: 1,
                created_at: "unix-ms:20".into(),
                updated_at: "unix-ms:20".into(),
            };
            store
                .request_delegation_cancellation(
                    &harness_store::CollaborationMutationContext {
                        company_id: "company-1".into(),
                        authenticated_actor: cancellation_attestation.source_host_ref,
                        command_name: "delegation_cancel_request".into(),
                        idempotency_key: "cancel-after-admission".into(),
                        expected_revision: cancellation_delegation.revision,
                        occurred_at: "unix-ms:20".into(),
                    },
                    &request,
                    &authority,
                )
                .unwrap();
            cancellation_finished_thread.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            !cancellation_finished.load(Ordering::SeqCst),
            "cancellation writer must not cross admission through Fabric commit"
        );
        release.wait();
        assert!(admission.join().unwrap().is_err());
        cancellation.join().unwrap();
        assert!(cancellation_finished.load(Ordering::SeqCst));

        let application = Wave6ControlPlaneApplication {
            collaboration_root: root.clone(),
            company_id: "company-1".into(),
            actor_id: "control-plane".into(),
        };
        let commits = std::sync::atomic::AtomicUsize::new(0);
        let mut accept = || {
            commits.fetch_add(1, Ordering::SeqCst);
            panic!("the pre-cancellation route must not reach Fabric")
        };
        assert!(application
            .admit_and_accept_source(&operation, &operation.actor, &mut accept)
            .is_err());
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn current_artifact_grant_authority_rejects_revocation_and_cancellation() {
        let (attestation, mut delegation, mut policy, publication, _) =
            current_remote_fact_fixture();
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

    #[test]
    fn artifact_retention_http_shape_rejects_caller_authored_terminal_times() {
        let accepted = serde_json::from_value::<CollaborationArtifactRetentionHttpRequest>(
            serde_json::json!({
                "expected_artifact_revision": 7,
                "retention_duration_ms": 86_400_000,
            }),
        )
        .expect("closed retention request");
        assert_eq!(accepted.expected_artifact_revision, 7);
        for field in [
            "terminal_transport_at_unix_ms",
            "terminal_delegation_at_unix_ms",
            "source_import_completed_at_unix_ms",
        ] {
            let mut hostile = serde_json::json!({
                "expected_artifact_revision": 7,
                "retention_duration_ms": 86_400_000,
            });
            hostile[field] = serde_json::json!(1);
            assert!(
                serde_json::from_value::<CollaborationArtifactRetentionHttpRequest>(hostile)
                    .is_err(),
                "caller-supplied {field} must fail before artifact mutation"
            );
        }
    }

    #[test]
    fn artifact_grant_replay_uses_frozen_authority_before_mutable_business_state() {
        let delegation: harness_core::collaboration::WorkDelegationV1 =
            serde_json::from_str(include_str!(
                "../../../schemas/collaboration/fixtures/work-delegation-v1/valid/awaiting.json"
            ))
            .expect("Delegation fixture");
        let attestation: harness_core::collaboration::SourceWorkAttestation =
            serde_json::from_str(include_str!(
                "../../../schemas/collaboration/fixtures/source-work-attestation/valid/server-authored.json"
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

    #[test]
    fn concurrent_exact_artifact_grants_commit_one_route_capability_and_receipt() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-artifact-grant-concurrent-replay-{}-{}",
            std::process::id(),
            now_unix_ms().unwrap()
        ));
        let collaboration_root = root.join("collaboration");
        let fabric_root = root.join("fabric");
        std::fs::create_dir_all(&root).unwrap();
        let (attestation, delegation, policy, publication, _) = current_remote_fact_fixture();
        seed_current_remote_fact_authority(&collaboration_root, &attestation, &delegation, &policy);

        let fabric_store = harness_fabric::FabricStore::open(&fabric_root).unwrap();
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert(&delegation.company_id, [7; 32]);
        let control = ControlPlane::new(
            &delegation.company_id,
            "control-artifact-replay",
            &fabric_store,
            &keys,
            [9; 32],
        );
        let lease = control
            .acquire_lease("lease-artifact-replay", 0, 100)
            .unwrap();
        let schema_digest = remote_fabric_schema_bundle_digest();
        let artifact_capability =
            harness_core::collaboration::RoutedBusinessKind::ArtifactGrant.required_capability();
        let allowed_capabilities = BTreeSet::from([
            "artifact-transfer".into(),
            "cross-team-collaboration".into(),
            "durable-routing".into(),
            artifact_capability,
        ]);
        let company_host = AuthenticatedActor {
            company_id: delegation.company_id.clone(),
            actor_id: "company-host:test".into(),
            actor_kind: harness_fabric::ActorKind::Human,
            role_bindings: BTreeSet::from(["company_host".into()]),
            session_id: "company-host:test".into(),
            issued_at_unix_ms: 100,
            expires_at_unix_ms: 20_000,
        };
        let mut source_certificate = None;
        for node_id in [
            &delegation.source_node_id,
            &delegation.target_placement.node_id,
        ] {
            let enrollment_id = format!("enrollment-{node_id}");
            let token = format!("artifact-replay-token-{node_id}-00000000000000000000");
            let daemon_id = format!("daemon-{node_id}");
            control
                .create_enrollment_bound(
                    &company_host,
                    lease.control_plane_generation,
                    &enrollment_id,
                    &token,
                    node_id,
                    allowed_capabilities.clone(),
                    &daemon_id,
                    1,
                    10_000,
                    110,
                )
                .unwrap();
            let csr =
                harness_fabric::pki::generate_node_csr(&delegation.company_id, node_id).unwrap();
            let (_, certificate) = control
                .consume_enrollment_csr(
                    lease.control_plane_generation,
                    &token,
                    node_id,
                    node_id,
                    &csr.csr_pem,
                    &format!("certificate-{node_id}"),
                    20_000,
                    &schema_digest,
                    120,
                )
                .unwrap();
            if node_id == &delegation.source_node_id {
                source_certificate = Some(certificate);
            }
        }
        let source_certificate = source_certificate.unwrap();
        let source_hello = NodeHello {
            company_id: delegation.company_id.clone(),
            node_id: delegation.source_node_id.clone(),
            instance_id: "gateway-source".into(),
            node_daemon_id: format!("daemon-{}", delegation.source_node_id),
            node_daemon_generation: 1,
            protocol_min: FABRIC_PROTOCOL_VERSION,
            protocol_max: FABRIC_PROTOCOL_VERSION,
            schema_bundle_digest: schema_digest.clone(),
            features: allowed_capabilities,
            build_sha: "artifact-replay-test-build".into(),
            last_persisted_route_seq: 0,
            unresolved_operation_ids: BTreeSet::new(),
            certificate_serial: source_certificate.serial.clone(),
            public_key_fingerprint: source_certificate.public_key_fingerprint.clone(),
        };
        control
            .connect_gateway_mtls(
                lease.control_plane_generation,
                &harness_fabric::transport::VerifiedMtlsPeer {
                    company_id: source_hello.company_id.clone(),
                    node_id: source_hello.node_id.clone(),
                    certificate_serial: source_hello.certificate_serial.clone(),
                    public_key_fingerprint: source_hello.public_key_fingerprint.clone(),
                    tls_version: "TLS1.3".into(),
                    websocket_subprotocol: harness_fabric::transport::FABRIC_WEBSOCKET_SUBPROTOCOL
                        .into(),
                },
                &source_hello,
                130,
            )
            .unwrap();

        let artifact_id = "artifact-concurrent-replay";
        let artifact_bytes = b"hello";
        let artifact_writer = AuthenticatedActor {
            company_id: delegation.company_id.clone(),
            actor_id: delegation.target_host_ref.id.clone(),
            actor_kind: harness_fabric::ActorKind::AgentMember,
            role_bindings: BTreeSet::from(["artifact_write".into()]),
            session_id: "artifact-writer:test".into(),
            issued_at_unix_ms: 140,
            expires_at_unix_ms: 10_000,
        };
        let (_, upload_capability) = control
            .initiate_collaboration_artifact(
                &artifact_writer,
                lease.control_plane_generation,
                artifact_id,
                &delegation.target_placement.node_id,
                &delegation.target_placement.team_id,
                &publication.fact_work_ref.work_id,
                None,
                "text/plain",
                artifact_bytes.len() as u64,
                &harness_fabric::sha256_hex(artifact_bytes),
                harness_fabric::ArtifactClassification::CompanyInternal,
                BTreeSet::from([attestation.source_host_ref.id.clone()]),
                150,
            )
            .unwrap();
        control
            .complete_artifact(
                lease.control_plane_generation,
                &upload_capability,
                artifact_bytes,
                160,
            )
            .unwrap();

        let request = CollaborationArtifactGrantHttpRequest {
            target_execution_space_id: delegation.source_work_ref.execution_space_id.clone(),
            expires_unix_ms: 10_000,
        };
        let control_actor = AuthenticatedActor {
            company_id: delegation.company_id.clone(),
            actor_id: format!("control-plane:{}", lease.control_plane_generation),
            actor_kind: harness_fabric::ActorKind::Service,
            role_bindings: BTreeSet::from(["company_control_plane".into(), "fabric_submit".into()]),
            session_id: format!("control-plane:{}", lease.control_plane_generation),
            issued_at_unix_ms: 200,
            expires_at_unix_ms: 20_000,
        };
        let start = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                start.wait();
                grant_collaboration_artifact(
                    &HarnessStore::new(&collaboration_root),
                    &control,
                    lease.control_plane_generation,
                    &delegation.id,
                    artifact_id,
                    &request,
                    &delegation.target_host_ref,
                    &control_actor,
                    "grant-concurrent-replay-key",
                    delegation.revision,
                    300,
                )
            });
            let second = scope.spawn(|| {
                start.wait();
                grant_collaboration_artifact(
                    &HarnessStore::new(&collaboration_root),
                    &control,
                    lease.control_plane_generation,
                    &delegation.id,
                    artifact_id,
                    &request,
                    &delegation.target_host_ref,
                    &control_actor,
                    "grant-concurrent-replay-key",
                    delegation.revision,
                    301,
                )
            });
            [
                first.join().unwrap().unwrap(),
                second.join().unwrap().unwrap(),
            ]
        });
        let first_receipt: RouteReceipt =
            serde_json::from_value(results[0]["receipt"].clone()).unwrap();
        let second_receipt: RouteReceipt =
            serde_json::from_value(results[1]["receipt"].clone()).unwrap();
        assert_eq!(
            first_receipt, second_receipt,
            "exact retries return one receipt"
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result["replayed"] == serde_json::json!(true))
                .count(),
            1,
            "exactly one concurrent request must be the frozen replay"
        );
        let state = fabric_store.snapshot().unwrap();
        assert_eq!(state.operations.len(), 1, "one durable route");
        assert_eq!(state.attempts.len(), 1, "one durable route attempt");
        assert_eq!(
            state
                .receipts
                .values()
                .filter(|receipt| receipt.kind == ReceiptKind::ControlPlaneAccepted)
                .count(),
            1,
            "one durable accepted receipt"
        );
        let operation = state.operations.values().next().unwrap();
        let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
            operation.closed_body().unwrap()
        else {
            panic!("artifact grant route must remain a closed collaboration operation")
        };
        let payload: CollaborationArtifactGrantEnvelope =
            serde_json::from_value(reference.payload).unwrap();
        assert_eq!(payload.read_capability.artifact_id, artifact_id);
        assert!(payload.read_capability.one_use, "one one-use capability");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn real_gateway_frame_timeout_is_bounded_within_the_lease() {
        assert_eq!(GATEWAY_FRAME_READ_TIMEOUT, Duration::from_secs(5));
        assert!(GATEWAY_FRAME_READ_TIMEOUT < Duration::from_secs(30));
    }

    #[test]
    fn execution_space_derives_the_exact_firm_home_without_escaping_to_user_home() {
        assert_eq!(
            firm_home_from_execution_space_root(Path::new(
                "/Users/test/.firm/execution-spaces/space-a"
            ))
            .expect("canonical Execution Space layout"),
            PathBuf::from("/Users/test/.firm")
        );
        assert!(
            firm_home_from_execution_space_root(Path::new("/Users/test/arbitrary/space-a"))
                .is_err()
        );
    }

    #[test]
    fn route_queue_replay_uses_semantic_intent_not_regenerated_timestamps() {
        let mut operation: harness_fabric::RoutedOperation = serde_json::from_str(include_str!(
            "../../../schemas/remote-fabric/fixtures/valid/routed-operation.json"
        ))
        .expect("valid routed operation fixture");
        operation
            .authorization_context
            .insert("capability".into(), "remote-probe".into());
        operation.created_at_unix_ms = 11;
        operation.expires_at_unix_ms = 2_000;
        operation.actor.issued_at_unix_ms = 11;
        operation.actor.expires_at_unix_ms = 2_000;
        let body = serde_json::json!({"probe":"reachable"});

        assert!(route_queue_intent_matches(
            &operation,
            "company-a",
            "operation-1",
            harness_fabric::PROBE_OPERATION_KIND,
            "node-a",
            Some("space-a"),
            "node-b",
            "space-b",
            "idem-1",
            "probe:a:b",
            None,
            harness_fabric::PROBE_BODY_SCHEMA,
            &body,
            "remote-probe",
        )
        .expect("semantic replay comparison"));

        let changed = serde_json::json!({"probe":"changed"});
        assert!(!route_queue_intent_matches(
            &operation,
            "company-a",
            "operation-1",
            harness_fabric::PROBE_OPERATION_KIND,
            "node-a",
            Some("space-a"),
            "node-b",
            "space-b",
            "idem-1",
            "probe:a:b",
            None,
            harness_fabric::PROBE_BODY_SCHEMA,
            &changed,
            "remote-probe",
        )
        .expect("changed replay comparison"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_keychain_credentials_require_public_identity_before_acl_access() {
        let args = vec![
            "--credential-backend".into(),
            "macos-keychain".into(),
            "--keychain-service".into(),
            "agentfirm.test.must-not-be-read".into(),
        ];
        let error = match resolve_node_credentials(&args, "company-test", "node-test") {
            Ok(_) => panic!("incomplete enrolled identity must fail before Keychain access"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("--certificate-serial"));
    }

    #[test]
    fn artifact_complete_body_limit_matches_the_frozen_64_mib_contract_only() {
        assert_eq!(
            fabric_http_body_limit("POST", "/v1/fabric/artifacts/artifact-a/complete"),
            ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
        );
        const { assert!(ARTIFACT_COMPLETE_HTTP_BODY_LIMIT > MAX_FABRIC_ARTIFACT_BYTES * 2) };
        assert_eq!(
            fabric_http_body_limit("POST", "/v1/fabric/artifacts/initiate"),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
        assert_eq!(
            fabric_http_body_limit("GET", "/v1/fabric/artifacts/artifact-a/complete"),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
        assert_eq!(
            fabric_http_body_limit("POST", "/v1/fabric/artifacts/a/nested/complete"),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
    }

    #[test]
    fn host_rest_secret_and_mutation_shapes_fail_closed() {
        assert!(constant_time_secret_eq("host-secret-a", "host-secret-a"));
        assert!(!constant_time_secret_eq("host-secret-a", "host-secret-b"));
        assert!(!constant_time_secret_eq("short", "longer"));
        reject_unknown_json_fields(
            &serde_json::json!({"expected_revision": 1}),
            &["expected_revision"],
        )
        .expect("closed mutation shape");
        assert_eq!(
            reject_unknown_json_fields(
                &serde_json::json!({"expected_revision": 1, "actor_id": "browser"}),
                &["expected_revision"],
            )
            .expect_err("browser identity field fails closed")
            .code,
            FabricErrorCode::InvalidPayload
        );
        let artifact_path = "/v1/fabric/artifacts/artifact-a/complete";
        let trusted_origin = "https://company.example";
        let host_token = "host-secret-a";
        let mut headers = std::collections::BTreeMap::new();
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            STANDARD_FABRIC_HTTP_BODY_LIMIT,
            "unauthenticated local callers never receive the large allocation budget"
        );
        headers.insert("authorization".into(), "Bearer host-secret-a".into());
        headers.insert("origin".into(), trusted_origin.into());
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            ARTIFACT_COMPLETE_HTTP_BODY_LIMIT
        );
        headers.insert("origin".into(), "https://malicious.example".into());
        assert_eq!(
            authorized_fabric_http_body_limit(
                "POST",
                artifact_path,
                &headers,
                trusted_origin,
                host_token,
            ),
            STANDARD_FABRIC_HTTP_BODY_LIMIT
        );
    }

    #[test]
    fn host_rest_enrollment_uses_csr_possession_and_one_atomic_consumption() {
        let root = std::env::temp_dir().join(format!(
            "agentfirm-fabric-http-enroll-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("remove prior test root");
        }
        std::fs::create_dir(&root).expect("create test root");
        let store = harness_fabric::FabricStore::open(&root).expect("FabricStore");
        let keys = InMemoryArtifactKeyBackend::default();
        keys.insert("company-test", [4; 32]);
        let control = ControlPlane::new("company-test", "control-test", &store, &keys, [5; 32]);
        let now = now_unix_ms().expect("clock");
        let lease = control
            .acquire_lease("lease-test", 0, now)
            .expect("Control Plane lease");
        let ca = harness_fabric::pki::generate_ca("company-test").expect("Company CA");
        let actor = AuthenticatedActor {
            company_id: "company-test".into(),
            actor_id: "company-host:http".into(),
            actor_kind: harness_fabric::ActorKind::Human,
            role_bindings: std::collections::BTreeSet::from(["company_host".into()]),
            session_id: "host-test".into(),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + 60_000,
        };
        let created = route_host_http(
            "POST",
            "/v1/fabric/enrollments",
            "/v1/fabric/enrollments",
            &serde_json::json!({
                "enrollment_id":"enrollment-node-a",
                "requested_name":"Node A",
                "allowed_capabilities":["durable-routing","artifact-transfer"],
                "authorized_node_daemon_id":"node-daemon:node-a",
                "authorized_node_daemon_generation":1
            }),
            Some(&actor),
            &control,
            lease.control_plane_generation,
            &ca,
            now + 1,
            "host-token-00000000000000000000000000000000",
        )
        .expect("create enrollment");
        let raw_token = created["raw_token"].as_str().expect("one-time token");
        let csr =
            harness_fabric::pki::generate_node_csr("company-test", "node-a").expect("Node CSR");
        let schema_bundle_digest = remote_fabric_schema_bundle_digest();
        let enrolled = route_host_http(
            "POST",
            "/v1/fabric/nodes/enroll",
            "/v1/fabric/nodes/enroll",
            &serde_json::json!({
                "raw_token":raw_token,
                "node_id":"node-a",
                "display_name":"Node A",
                "csr_pem":csr.csr_pem,
                "schema_bundle_digest":schema_bundle_digest
            }),
            None,
            &control,
            lease.control_plane_generation,
            &ca,
            now + 2,
            "host-token-00000000000000000000000000000000",
        )
        .expect("consume CSR enrollment");
        assert_eq!(enrolled["node"]["id"], "node-a");
        assert_eq!(
            enrolled["node"]["public_key_fingerprint"],
            csr.public_key_fingerprint
        );
        let listed = route_host_http(
            "GET",
            "/v1/fabric/nodes",
            "/v1/fabric/nodes?limit=1",
            &serde_json::Value::Null,
            Some(&actor),
            &control,
            lease.control_plane_generation,
            &ca,
            now + 2,
            "host-token-00000000000000000000000000000000",
        )
        .expect("list Nodes with opaque snapshot cursor");
        let cursor = listed["next_cursor"].as_str().expect("next cursor");
        assert_ne!(cursor, "node-a");
        assert_eq!(cursor.len(), 64);
        assert_eq!(
            route_host_http(
                "GET",
                "/v1/fabric/nodes",
                "/v1/fabric/nodes?cursor=browser-selected-node-a",
                &serde_json::Value::Null,
                Some(&actor),
                &control,
                lease.control_plane_generation,
                &ca,
                now + 2,
                "host-token-00000000000000000000000000000000",
            )
            .expect_err("raw or forged Node cursor fails closed")
            .code,
            FabricErrorCode::ExpectedRevisionConflict
        );
        let before = store.snapshot().expect("before replay");
        let replay = route_host_http(
            "POST",
            "/v1/fabric/nodes/enroll",
            "/v1/fabric/nodes/enroll",
            &serde_json::json!({
                "raw_token":raw_token,
                "node_id":"node-a",
                "display_name":"Node A",
                "csr_pem":csr.csr_pem,
                "schema_bundle_digest":schema_bundle_digest
            }),
            None,
            &control,
            lease.control_plane_generation,
            &ca,
            now + 3,
            "host-token-00000000000000000000000000000000",
        )
        .expect_err("one-use token cannot replay");
        assert_eq!(replay.code, FabricErrorCode::EnrollmentConsumed);
        assert_eq!(store.snapshot().expect("after replay"), before);

        let recovery = route_host_http(
            "POST",
            "/v1/fabric/enrollments",
            "/v1/fabric/enrollments",
            &serde_json::json!({
                "enrollment_id":"enrollment-node-a-recovery",
                "requested_name":"Node A recovered",
                "allowed_capabilities":["durable-routing","artifact-transfer"],
                "authorized_node_daemon_id":"node-daemon:node-a",
                "authorized_node_daemon_generation":2
            }),
            Some(&actor),
            &control,
            lease.control_plane_generation,
            &ca,
            now + 4,
            "host-token-00000000000000000000000000000000",
        )
        .expect("Host grants exact successor recovery");
        let recovery_csr = harness_fabric::pki::generate_node_csr("company-test", "node-a")
            .expect("successor CSR");
        let recovered = route_host_http(
            "POST",
            "/v1/fabric/nodes/enroll",
            "/v1/fabric/nodes/enroll",
            &serde_json::json!({
                "raw_token":recovery["raw_token"],
                "node_id":"node-a",
                "display_name":"Node A recovered",
                "csr_pem":recovery_csr.csr_pem,
                "schema_bundle_digest":schema_bundle_digest
            }),
            None,
            &control,
            lease.control_plane_generation,
            &ca,
            now + 5,
            "host-token-00000000000000000000000000000000",
        )
        .expect("existing Node recovers under Host-frozen successor daemon generation");
        assert_eq!(recovered["node"]["node_revision"], 2);
        assert_eq!(recovered["certificate"]["node_daemon_generation"], 2);
        let recovery_state = store.snapshot().expect("recovery state");
        assert!(recovery_state.revoked_certificate_serials.contains(
            enrolled["certificate"]["serial"]
                .as_str()
                .expect("old serial")
        ));
        std::fs::remove_dir_all(root).expect("remove test root");
    }
}
