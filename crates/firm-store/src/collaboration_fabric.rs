#![allow(clippy::result_large_err)]

use crate::canonical_json_fingerprint;
use firm_core::agentfirm_api::{ActorKind as CoreActorKind, ActorRef};
use firm_core::collaboration::{
    FabricEffectCertainty, FabricError as CollaborationError,
    FabricErrorCode as CollaborationErrorCode, RoutedBusinessOperation, RoutedBusinessReceipt,
};
use firm_core::{
    AgentTeamStatus, TeamActorKind, TeamActorRef, TeamRunStatus, Work, WorkClaimMode,
    WorkCommandContext, WorkCondition, WorkPhase, WorkPriority,
};
use firm_fabric::{
    json_digest, ActorKind, AuthenticatedActor, CollaborationBusinessReference, EffectCertainty,
    FabricError, FabricErrorCode, OperationPriority, OperationSourceAuthority, ReceiptKind,
    RouteReceipt, RoutedOperation, COLLABORATION_BUSINESS_OPERATION_KIND,
    COLLABORATION_BUSINESS_OPERATION_SCHEMA, FABRIC_CANONICALIZATION_VERSION,
    FABRIC_PROTOCOL_VERSION, FABRIC_SCHEMA_VERSION,
};
use std::collections::BTreeMap;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetWorkCreatePayload {
    delegation_id: String,
    requested_outcome: String,
    acceptance_contract: String,
    source_work_ref: firm_core::collaboration::RemoteWorkRef,
    target_placement: firm_core::collaboration::TargetPlacementRef,
}

/// Server-resolved Wave 5 route authority. None of these fields may be copied
/// from the public collaboration request body.
#[derive(Debug, Clone)]
pub struct CollaborationFabricRouteContext {
    pub authenticated_actor: AuthenticatedActor,
    pub resolved_business_actor: ActorRef,
    pub source_gateway_generation: u64,
    pub source_node_daemon_id: String,
    pub source_node_daemon_generation: u64,
    pub control_plane_generation: u64,
    pub source_execution_space_id: String,
    pub target_execution_space_id: Option<String>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

/// Minimal execution seam implemented by the NodeDaemon's accepted Wave 5
/// gateway client. It may block until reconciliation produces a terminal
/// receipt, but it must never translate ControlPlaneAccepted/TargetPersisted
/// into application success.
pub trait CollaborationRouteClient {
    fn route_and_reconcile(&self, operation: RoutedOperation) -> Result<RouteReceipt, FabricError>;
}

pub struct RemoteFabricCollaborationPort<'a, C> {
    client: &'a C,
    context: CollaborationFabricRouteContext,
    applied_at: String,
}

impl<'a, C> RemoteFabricCollaborationPort<'a, C> {
    pub fn new(
        client: &'a C,
        context: CollaborationFabricRouteContext,
        applied_at: impl Into<String>,
    ) -> Self {
        Self {
            client,
            context,
            applied_at: applied_at.into(),
        }
    }
}

impl<C: CollaborationRouteClient> crate::CollaborationFabricPort
    for RemoteFabricCollaborationPort<'_, C>
{
    fn dispatch(
        &self,
        operation: &RoutedBusinessOperation,
    ) -> Result<RoutedBusinessReceipt, CollaborationError> {
        let routed = route_collaboration_business_operation(operation, &self.context)
            .map_err(|error| collaboration_transport_error(operation, error))?;
        let receipt = self
            .client
            .route_and_reconcile(routed)
            .map_err(|error| collaboration_transport_error(operation, error))?;
        collaboration_receipt_from_fabric(operation, &receipt, &self.applied_at)
    }
}

fn actor_matches(actual: &ActorRef, expected: &ActorRef) -> bool {
    actual == expected && !actual.id.trim().is_empty()
}

fn invalid(message: impl Into<String>) -> FabricError {
    FabricError::none(FabricErrorCode::InvalidPayload, message)
}

/// Convert one frozen Wave 6 business operation into the accepted Wave 5
/// RoutedOperation. This is the only cross-crate route adapter: it preserves
/// the existing route journal, receipt, ordering, expiry and recovery model.
pub fn route_collaboration_business_operation(
    operation: &RoutedBusinessOperation,
    context: &CollaborationFabricRouteContext,
) -> Result<RoutedOperation, FabricError> {
    if operation.protocol_version != "agentfirm.fabric.v1"
        || operation.source_node_id.trim().is_empty()
        || operation.target_placement.node_id.trim().is_empty()
        || operation.target_placement.team_id.trim().is_empty()
        || operation.target_placement.team_revision == 0
        || operation.target_placement.placement_generation != 1
        || operation.required_capability != operation.kind.required_capability()
        || operation.payload_digest != canonical_json_fingerprint(&operation.payload)
        || !actor_matches(
            &operation.authenticated_actor,
            &context.resolved_business_actor,
        )
        || context.authenticated_actor.actor_kind != ActorKind::Service
        || context.authenticated_actor.actor_id != operation.source_node_id
        || context.source_gateway_generation == 0
        || context.source_node_daemon_id.trim().is_empty()
        || context.source_node_daemon_generation == 0
        || context.control_plane_generation == 0
        || context.source_execution_space_id.trim().is_empty()
        || context.expires_at_unix_ms <= context.created_at_unix_ms
    {
        return Err(invalid(
            "collaboration business operation disagrees with server-resolved actor, placement, capability, payload, or generation",
        ));
    }

    let business_actor_kind = match context.resolved_business_actor.kind {
        CoreActorKind::Human => "human",
        CoreActorKind::AgentMember => "agent_member",
        CoreActorKind::Service => "service",
        CoreActorKind::External => {
            return Err(invalid(
                "external actor cannot acquire cross-machine collaboration authority",
            ))
        }
    };
    let body = CollaborationBusinessReference {
        business_kind: operation.kind.wire_name().into(),
        required_capability: operation.required_capability.clone(),
        business_actor_kind: business_actor_kind.into(),
        business_actor_id: context.resolved_business_actor.id.clone(),
        target_team_id: operation.target_placement.team_id.clone(),
        target_team_revision: operation.target_placement.team_revision,
        placement_generation: operation.target_placement.placement_generation,
        expected_revision: operation.expected_revision,
        payload_digest: operation.payload_digest.clone(),
        payload: operation.payload.clone(),
    };
    let body_value = serde_json::to_value(body)
        .map_err(|error| invalid(format!("collaboration body encoding failed: {error}")))?;
    let body_digest = json_digest(&body_value)?;
    let mut authorization_context = BTreeMap::new();
    authorization_context.insert(
        "target_team_id".into(),
        operation.target_placement.team_id.clone(),
    );
    authorization_context.insert(
        "target_team_revision".into(),
        operation.target_placement.team_revision.to_string(),
    );
    authorization_context.insert(
        "placement_generation".into(),
        operation.target_placement.placement_generation.to_string(),
    );
    authorization_context.insert(
        "required_capability".into(),
        operation.required_capability.clone(),
    );
    authorization_context.insert("business_actor_kind".into(), business_actor_kind.into());
    authorization_context.insert(
        "business_actor_id".into(),
        context.resolved_business_actor.id.clone(),
    );

    let routed = RoutedOperation {
        id: operation.id.clone(),
        company_id: operation.company_id.clone(),
        kind: COLLABORATION_BUSINESS_OPERATION_KIND.into(),
        source_authority: OperationSourceAuthority::Node,
        source_node_id: Some(operation.source_node_id.clone()),
        target_node_id: operation.target_placement.node_id.clone(),
        source_gateway_generation: Some(context.source_gateway_generation),
        source_node_daemon_id: Some(context.source_node_daemon_id.clone()),
        source_node_daemon_generation: Some(context.source_node_daemon_generation),
        control_plane_generation: context.control_plane_generation,
        source_execution_space_id: Some(context.source_execution_space_id.clone()),
        target_execution_space_id: context.target_execution_space_id.clone(),
        actor: context.authenticated_actor.clone(),
        actor_runtime_generation: None,
        authorization_context,
        idempotency_key: operation.idempotency_key.clone(),
        ordering_key: operation.ordering_key.clone(),
        correlation_id: operation.id.clone(),
        causation_id: None,
        expected_target_revision: Some(operation.expected_revision),
        body_schema: COLLABORATION_BUSINESS_OPERATION_SCHEMA.into(),
        body: body_value,
        body_digest,
        priority: OperationPriority::Normal,
        created_at_unix_ms: context.created_at_unix_ms,
        expires_at_unix_ms: context.expires_at_unix_ms,
        protocol_version: FABRIC_PROTOCOL_VERSION,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
        canonicalization_version: FABRIC_CANONICALIZATION_VERSION.into(),
    };
    routed.closed_body()?;
    Ok(routed)
}

fn application_error(
    code: FabricErrorCode,
    message: impl Into<String>,
    operation_id: &str,
) -> FabricError {
    let mut error = FabricError::none(code, message);
    error.operation_id = Some(operation_id.into());
    error
}

/// Target Node application boundary for routed collaboration operations. The
/// route/inbox is already durably claimed when this is called. Native Work is
/// created through the existing WorkApplicationService store contract, so an
/// ack loss replays the same event/idempotency key instead of creating a
/// second Work.
pub fn apply_collaboration_target_operation(
    store: &crate::HarnessStore,
    operation: &RoutedOperation,
    occurred_at: &str,
) -> Result<(String, serde_json::Value, EffectCertainty), FabricError> {
    let reference = match operation.closed_body()? {
        firm_fabric::ClosedOperationBody::CollaborationBusiness(reference) => reference,
        _ => {
            return Err(application_error(
                FabricErrorCode::InvalidPayload,
                "target collaboration application requires the closed Wave6 envelope",
                &operation.id,
            ))
        }
    };
    if reference.business_kind != "target_work_create"
        || reference.required_capability != "collaboration.target_work_create"
    {
        return Err(application_error(
            FabricErrorCode::FeatureIncompatible,
            "this target Node application does not own the routed collaboration kind",
            &operation.id,
        ));
    }
    let payload =
        serde_json::from_value::<TargetWorkCreatePayload>(reference.payload).map_err(|error| {
            application_error(
                FabricErrorCode::InvalidPayload,
                format!("target_work_create payload is invalid: {error}"),
                &operation.id,
            )
        })?;
    if payload.delegation_id.trim().is_empty()
        || payload.requested_outcome.trim().is_empty()
        || payload.acceptance_contract.trim().is_empty()
        || payload.target_placement.team_id != reference.target_team_id
        || payload.target_placement.team_revision != reference.target_team_revision
        || payload.target_placement.placement_generation != reference.placement_generation
        || payload.target_placement.node_id != operation.target_node_id
    {
        return Err(application_error(
            FabricErrorCode::TargetNotPlaced,
            "target Work payload disagrees with exact routed Team placement",
            &operation.id,
        ));
    }
    let team_rows = store.teams().map_err(|error| {
        application_error(
            FabricErrorCode::StoreUnavailable,
            format!("target AgentTeam lookup failed: {error}"),
            &operation.id,
        )
    })?;
    let team_revision = team_rows
        .iter()
        .filter(|team| team.id == reference.target_team_id)
        .count() as u64;
    let team = team_rows
        .iter()
        .rev()
        .find(|team| team.id == reference.target_team_id)
        .ok_or_else(|| {
            application_error(
                FabricErrorCode::TargetNotPlaced,
                "target AgentTeam does not exist",
                &operation.id,
            )
        })?;
    if team_revision != reference.target_team_revision
        || team.node_id != operation.target_node_id
        || team.status != AgentTeamStatus::Active
        || reference.placement_generation != 1
        || reference.business_actor_kind != "agent_member"
        || reference.business_actor_id != team.host_agent_id
    {
        return Err(application_error(
            FabricErrorCode::NodeStaleGeneration,
            "target Team revision, immutable Node, status, or exact Host changed",
            &operation.id,
        ));
    }
    let mut latest_runs = BTreeMap::new();
    for run in store.team_runs().map_err(|error| {
        application_error(
            FabricErrorCode::StoreUnavailable,
            format!("target TeamRun lookup failed: {error}"),
            &operation.id,
        )
    })? {
        latest_runs.insert(run.id.clone(), run);
    }
    let live_runs = latest_runs
        .into_values()
        .filter(|run| {
            run.agent_team_id == team.id
                && !matches!(
                    run.status,
                    TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
                )
        })
        .collect::<Vec<_>>();
    if live_runs.len() != 1 {
        return Err(application_error(
            FabricErrorCode::TargetNotPlaced,
            "target AgentTeam must resolve to exactly one current TeamRun",
            &operation.id,
        ));
    }
    let run = &live_runs[0];
    let work_id = format!("remote-work:{}", payload.delegation_id);
    let event_id = format!("remote-work-created:{}", payload.delegation_id);
    let work = store
        .insert_work(
            Work {
                id: work_id.clone(),
                team_run_id: run.id.clone(),
                team_id: Some(team.id.clone()),
                parent_work_id: None,
                title: payload.requested_outcome,
                context_markdown: format!(
                    "Cross-Team delegation {} from source Work {}",
                    payload.delegation_id, payload.source_work_ref.work_id
                ),
                completion_criteria_markdown: payload.acceptance_contract,
                phase: WorkPhase::Open,
                condition: WorkCondition::Normal,
                resolution: None,
                owner_member_id: None,
                active_member_run_id: None,
                claim_mode: WorkClaimMode::TeamClaim,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: Vec::new(),
                priority: WorkPriority::Normal,
                created_by_actor: TeamActorRef {
                    kind: TeamActorKind::Host,
                    id: team.host_agent_id.clone(),
                    display_name: None,
                    authn_source: Some("remote_fabric_verified_source_node".into()),
                },
                created_by_member_id: None,
                result_summary: None,
                blocker_reason: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                github_links: Vec::new(),
                version: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            WorkCommandContext {
                event_id: event_id.clone(),
                performed_by_actor: TeamActorRef {
                    kind: TeamActorKind::Host,
                    id: team.host_agent_id.clone(),
                    display_name: None,
                    authn_source: Some("remote_fabric_verified_source_node".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: operation.idempotency_key.clone(),
                created_at: occurred_at.into(),
                duplicate_ok: true,
            },
        )
        .map_err(|error| {
            application_error(
                FabricErrorCode::ExpectedRevisionConflict,
                format!("native target Work creation failed closed: {error}"),
                &operation.id,
            )
        })?;
    let execution_space_id = operation
        .target_execution_space_id
        .as_ref()
        .ok_or_else(|| {
            application_error(
                FabricErrorCode::TargetNotPlaced,
                "target_work_create route lacks target Execution Space",
                &operation.id,
            )
        })?;
    let target_work_ref = firm_core::collaboration::RemoteWorkRef {
        schema_version: "agentfirm.remote-work-ref.v1".into(),
        execution_space_id: execution_space_id.clone(),
        node_id: team.node_id.clone(),
        team_id: team.id.clone(),
        team_revision,
        placement_generation: 1,
        work_id: work.id.clone(),
        work_revision: work.version,
        work_event_id: event_id,
        digest: canonical_json_fingerprint(&serde_json::to_value(&work).map_err(|error| {
            application_error(
                FabricErrorCode::StoreUnavailable,
                format!("target Work digest encoding failed: {error}"),
                &operation.id,
            )
        })?),
    };
    Ok((
        "agentfirm.collaboration.target_work_created.v1".into(),
        serde_json::json!({"target_work_ref": target_work_ref}),
        EffectCertainty::Applied,
    ))
}

fn collaboration_error(
    operation: &RoutedBusinessOperation,
    code: CollaborationErrorCode,
    message: impl Into<String>,
    effect_certainty: FabricEffectCertainty,
) -> CollaborationError {
    CollaborationError {
        code,
        message: message.into(),
        retryable: false,
        effect_certainty,
        resource_kind: "routed_operation".into(),
        resource_id: operation.id.clone(),
        current_revision: Some(operation.expected_revision),
    }
}

fn collaboration_transport_error(
    operation: &RoutedBusinessOperation,
    error: FabricError,
) -> CollaborationError {
    let (code, certainty) = if error.code == FabricErrorCode::RecoveryRequired
        || error.effect == EffectCertainty::Unknown
    {
        (
            CollaborationErrorCode::RecoveryRequired,
            FabricEffectCertainty::Unknown,
        )
    } else {
        (
            match error.code {
                FabricErrorCode::IdempotencyConflict => CollaborationErrorCode::IdempotencyConflict,
                FabricErrorCode::ExpectedRevisionConflict => {
                    CollaborationErrorCode::RevisionConflict
                }
                FabricErrorCode::UnauthorizedActor
                | FabricErrorCode::WrongCompany
                | FabricErrorCode::SourceMismatch => CollaborationErrorCode::UnauthorizedActor,
                FabricErrorCode::TargetOffline | FabricErrorCode::TargetNotPlaced => {
                    CollaborationErrorCode::TargetTeamUnavailable
                }
                FabricErrorCode::NodeStaleGeneration => {
                    CollaborationErrorCode::TargetTeamPlacementChanged
                }
                _ => CollaborationErrorCode::ProtocolMismatch,
            },
            FabricEffectCertainty::None,
        )
    };
    let mut translated = collaboration_error(operation, code, error.message, certainty);
    translated.retryable = error.retryable;
    translated
}

/// Translate only a terminal, generation-fenced Wave 5 receipt back to the
/// pure collaboration service. Accepted/persisted receipts are not business
/// success; recovery_required remains Unknown and is never folded.
pub fn collaboration_receipt_from_fabric(
    operation: &RoutedBusinessOperation,
    receipt: &RouteReceipt,
    applied_at: &str,
) -> Result<RoutedBusinessReceipt, CollaborationError> {
    if receipt.operation_id != operation.id
        || receipt.company_id != operation.company_id
        || receipt.target_node_id != operation.target_placement.node_id
    {
        return Err(collaboration_error(
            operation,
            CollaborationErrorCode::ProtocolMismatch,
            "route receipt is outside the exact collaboration operation scope",
            FabricEffectCertainty::None,
        ));
    }
    if receipt.kind == ReceiptKind::RecoveryRequired
        || receipt.application_effect == Some(EffectCertainty::Unknown)
    {
        return Err(collaboration_error(
            operation,
            CollaborationErrorCode::RecoveryRequired,
            "routed collaboration effect is unknown and requires reconciliation",
            FabricEffectCertainty::Unknown,
        ));
    }
    if receipt.kind != ReceiptKind::OperationApplied
        || receipt.application_effect != Some(EffectCertainty::Applied)
    {
        return Err(collaboration_error(
            operation,
            CollaborationErrorCode::ProtocolMismatch,
            "transport acceptance or target persistence is not collaboration business success",
            FabricEffectCertainty::None,
        ));
    }
    let result = receipt.result.clone().ok_or_else(|| {
        collaboration_error(
            operation,
            CollaborationErrorCode::ProtocolMismatch,
            "applied route receipt lacks an application result",
            FabricEffectCertainty::None,
        )
    })?;
    let expected_digest = canonical_json_fingerprint(&result);
    if receipt.result_digest.as_deref() != expected_digest.strip_prefix("sha256:") {
        return Err(collaboration_error(
            operation,
            CollaborationErrorCode::ProtocolMismatch,
            "applied route result digest is missing or forged",
            FabricEffectCertainty::None,
        ));
    }
    Ok(RoutedBusinessReceipt {
        operation_id: operation.id.clone(),
        kind: operation.kind,
        target_node_id: operation.target_placement.node_id.clone(),
        target_placement_generation: operation.target_placement.placement_generation,
        effect_certainty: FabricEffectCertainty::Applied,
        result_digest: expected_digest,
        result,
        applied_at: applied_at.into(),
    })
}
