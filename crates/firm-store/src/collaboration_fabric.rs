use crate::canonical_json_fingerprint;
use firm_core::agentfirm_api::{ActorKind as CoreActorKind, ActorRef};
use firm_core::collaboration::{
    FabricEffectCertainty, FabricError as CollaborationError,
    FabricErrorCode as CollaborationErrorCode, RoutedBusinessOperation, RoutedBusinessReceipt,
};
use firm_fabric::{
    json_digest, ActorKind, AuthenticatedActor, CollaborationBusinessReference, EffectCertainty,
    FabricError, FabricErrorCode, OperationPriority, OperationSourceAuthority, ReceiptKind,
    RouteReceipt, RoutedOperation, COLLABORATION_BUSINESS_OPERATION_KIND,
    COLLABORATION_BUSINESS_OPERATION_SCHEMA, FABRIC_CANONICALIZATION_VERSION,
    FABRIC_PROTOCOL_VERSION, FABRIC_SCHEMA_VERSION,
};
use std::collections::BTreeMap;

/// Server-resolved Wave 5 route authority. None of these fields may be copied
/// from the public collaboration request body.
#[derive(Debug, Clone)]
pub struct CollaborationFabricRouteContext {
    pub authenticated_actor: AuthenticatedActor,
    pub source_gateway_generation: u64,
    pub source_node_daemon_id: String,
    pub source_node_daemon_generation: u64,
    pub control_plane_generation: u64,
    pub source_execution_space_id: String,
    pub target_execution_space_id: Option<String>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

fn actor_matches(core: &ActorRef, fabric: &AuthenticatedActor) -> bool {
    let kind = match core.kind {
        CoreActorKind::Human => Some(ActorKind::Human),
        CoreActorKind::AgentMember => Some(ActorKind::AgentMember),
        CoreActorKind::Service => Some(ActorKind::Service),
        CoreActorKind::External => None,
    };
    core.id == fabric.actor_id && kind == Some(fabric.actor_kind)
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
        || !actor_matches(&operation.authenticated_actor, &context.authenticated_actor)
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

    let body = CollaborationBusinessReference {
        business_kind: operation.kind.wire_name().into(),
        required_capability: operation.required_capability.clone(),
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
