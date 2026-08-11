use crate::node_gateway::require_current_gateway;
use crate::protocol::*;
use crate::store::{FabricState, FabricStoreLimits};
use crate::{
    canonical_digest, FabricError, FabricErrorCode, FABRIC_PROTOCOL_VERSION, FABRIC_SCHEMA_VERSION,
};

#[allow(clippy::type_complexity)]
pub(crate) fn accept_and_enqueue(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    operation: RoutedOperation,
    limits: FabricStoreLimits,
    now_unix_ms: u64,
) -> Result<(RoutedOperation, RouteAttempt, RouteReceipt, bool), FabricError> {
    validate_operation(state, company_id, &operation, now_unix_ms)?;
    let request_digest = canonical_digest(&operation)?;
    let idempotency_index = format!(
        "{}:{}:{}",
        company_id, operation.source_node_id, operation.idempotency_key
    );
    if let Some(existing_id) = state.operation_idempotency.get(&idempotency_index) {
        let existing = state.operations.get(existing_id).ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "idempotency index references a missing operation",
            )
        })?;
        if canonical_digest(existing)? != request_digest {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "same idempotency key was reused with a different request",
            ));
        }
        let attempt = state
            .attempts
            .values()
            .find(|attempt| attempt.operation_id == existing.id)
            .cloned()
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    "replayed operation has no durable route attempt",
                )
            })?;
        let receipt = receipt_for(
            state,
            &existing.id,
            ReceiptKind::ControlPlaneAccepted,
            attempt.target_gateway_generation,
        )?;
        return Ok((existing.clone(), attempt, receipt, true));
    }
    if state.operations.contains_key(&operation.id) {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "operation id already exists",
        ));
    }
    let rate_key = format!(
        "{}:{}:{}",
        company_id, operation.source_node_id, operation.actor.actor_id
    );
    let window_start = now_unix_ms - (now_unix_ms % 60_000);
    if let Some(window) = state.rate_windows.get(&rate_key) {
        if window.window_started_at_unix_ms == window_start
            && window.accepted_count >= limits.max_operations_per_minute_per_source_actor
        {
            let mut error = FabricError::none(
                FabricErrorCode::RateLimited,
                "source Node and actor exceeded the durable per-minute operation limit",
            );
            error.retryable = true;
            error.retry_after_ms = Some(window_start.saturating_add(60_000) - now_unix_ms);
            return Err(error);
        }
    }
    let gateway = state
        .gateway_leases
        .get(&operation.target_node_id)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::TargetOffline,
                "target Node has no gateway lease",
            )
        })?;
    if gateway.company_id != company_id
        || gateway.control_plane_generation != control_plane_generation
        || gateway.expires_at_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::TargetOffline,
            "target Node gateway is offline or stale",
        ));
    }
    let queued = state
        .attempts
        .values()
        .filter(|attempt| {
            attempt.target_node_id == operation.target_node_id
                && attempt.state != RouteAttemptState::Ended
        })
        .count();
    let queued_bytes = state
        .attempts
        .values()
        .filter(|attempt| {
            attempt.target_node_id == operation.target_node_id
                && attempt.state != RouteAttemptState::Ended
        })
        .filter_map(|attempt| state.operations.get(&attempt.operation_id))
        .map(|operation| serde_json::to_vec(operation).map_or(u64::MAX, |bytes| bytes.len() as u64))
        .sum::<u64>();
    let operation_bytes = serde_json::to_vec(&operation)
        .map_err(|error| {
            FabricError::none(
                FabricErrorCode::InvalidPayload,
                format!("operation encoding failed: {error}"),
            )
        })?
        .len() as u64;
    if queued >= limits.max_queued_operations_per_node
        || queued_bytes.saturating_add(operation_bytes) > limits.max_queued_bytes_per_node
    {
        return Err(FabricError::none(
            FabricErrorCode::QueueCapacity,
            "target Node queue capacity reached; operation was not accepted",
        ));
    }
    let route_seq = state
        .route_sequences
        .get(&operation.target_node_id)
        .copied()
        .unwrap_or(0)
        .saturating_add(1);
    let attempt = RouteAttempt {
        id: format!("route-attempt:{}:1", operation.id),
        company_id: company_id.into(),
        operation_id: operation.id.clone(),
        attempt_no: 1,
        target_node_id: operation.target_node_id.clone(),
        target_gateway_generation: gateway.gateway_generation,
        control_plane_generation,
        route_seq,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: now_unix_ms,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let receipt = RouteReceipt {
        id: receipt_id(
            &operation.id,
            ReceiptKind::ControlPlaneAccepted,
            gateway.gateway_generation,
        ),
        company_id: company_id.into(),
        operation_id: operation.id.clone(),
        target_node_id: operation.target_node_id.clone(),
        target_gateway_generation: gateway.gateway_generation,
        control_plane_generation,
        route_seq,
        kind: ReceiptKind::ControlPlaneAccepted,
        result_schema: None,
        result: None,
        result_digest: None,
        error: None,
        created_at_unix_ms: now_unix_ms,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state
        .operation_idempotency
        .insert(idempotency_index, operation.id.clone());
    state
        .route_sequences
        .insert(operation.target_node_id.clone(), route_seq);
    state
        .operations
        .insert(operation.id.clone(), operation.clone());
    state.outboxes.insert(
        operation.id.clone(),
        LocalRemoteOutbox {
            company_id: company_id.into(),
            node_id: operation.source_node_id.clone(),
            operation_id: operation.id.clone(),
            request_digest,
            local_state: LocalOutboxState::Accepted,
            gateway_generation: operation.source_gateway_generation,
            attempt_count: 1,
            last_attempt_at_unix_ms: Some(now_unix_ms),
            terminal_receipt_ref: None,
            schema_version: FABRIC_SCHEMA_VERSION.into(),
        },
    );
    state.attempts.insert(attempt.id.clone(), attempt.clone());
    state.receipts.insert(receipt.id.clone(), receipt.clone());
    let window = state
        .rate_windows
        .entry(rate_key)
        .or_insert(FabricRateWindow {
            company_id: company_id.into(),
            source_node_id: operation.source_node_id.clone(),
            actor_id: operation.actor.actor_id.clone(),
            window_started_at_unix_ms: window_start,
            accepted_count: 0,
            schema_version: FABRIC_SCHEMA_VERSION.into(),
        });
    if window.window_started_at_unix_ms != window_start {
        window.window_started_at_unix_ms = window_start;
        window.accepted_count = 0;
    }
    window.accepted_count = window.accepted_count.saturating_add(1);
    Ok((operation, attempt, receipt, false))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn persist_target_inbox(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    operation_id: &str,
    request_digest: &str,
    route_seq: u64,
    now_unix_ms: u64,
) -> Result<(LocalRemoteInbox, RouteReceipt, bool), FabricError> {
    require_current_gateway(
        state,
        company_id,
        control_plane_generation,
        node_id,
        gateway_generation,
        now_unix_ms,
    )?;
    let operation = state.operations.get(operation_id).cloned().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::OperationUnknown,
            "routed operation does not exist",
        )
    })?;
    if operation.target_node_id != node_id
        || operation.company_id != company_id
        || canonical_digest(&operation)? != request_digest
    {
        return Err(FabricError::none(
            FabricErrorCode::SourceMismatch,
            "target inbox request does not match the routed operation",
        ));
    }
    let attempt_id = state
        .attempts
        .values()
        .find(|attempt| {
            attempt.operation_id == operation_id
                && attempt.route_seq == route_seq
                && attempt.target_gateway_generation == gateway_generation
        })
        .map(|attempt| attempt.id.clone())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "target persistence acknowledgement has no matching route attempt",
            )
        })?;
    let attempt = state.attempts.get_mut(&attempt_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "routed operation has no attempt",
        )
    })?;
    if attempt.route_seq != route_seq
        || attempt.target_gateway_generation != gateway_generation
        || attempt.control_plane_generation != control_plane_generation
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "target persistence acknowledgement is stale",
        ));
    }
    if let Some(existing) = state.inboxes.get(operation_id) {
        if existing.request_digest != request_digest
            || existing.route_seq != route_seq
            || existing.gateway_generation != gateway_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "duplicate inbox persistence changed the request",
            ));
        }
        let receipt = receipt_for(
            state,
            operation_id,
            ReceiptKind::TargetPersisted,
            gateway_generation,
        )?;
        return Ok((existing.clone(), receipt, true));
    }
    attempt.state = RouteAttemptState::TargetPersisted;
    attempt.effect = EffectCertainty::None;
    let inbox = LocalRemoteInbox {
        company_id: company_id.into(),
        node_id: node_id.into(),
        operation_id: operation_id.into(),
        route_seq,
        request_digest: request_digest.into(),
        state: LocalInboxState::Persisted,
        gateway_generation,
        attempt_count: 1,
        claim_generation: None,
        result_digest: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let receipt = RouteReceipt {
        id: receipt_id(
            operation_id,
            ReceiptKind::TargetPersisted,
            gateway_generation,
        ),
        company_id: company_id.into(),
        operation_id: operation_id.into(),
        target_node_id: node_id.into(),
        target_gateway_generation: gateway_generation,
        control_plane_generation,
        route_seq,
        kind: ReceiptKind::TargetPersisted,
        result_schema: None,
        result: None,
        result_digest: None,
        error: None,
        created_at_unix_ms: now_unix_ms,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state.inboxes.insert(operation_id.into(), inbox.clone());
    state.receipts.insert(receipt.id.clone(), receipt.clone());
    Ok((inbox, receipt, false))
}

pub(crate) fn retry_operation(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    operation_id: &str,
    now_unix_ms: u64,
) -> Result<(RouteAttempt, RouteReceipt, bool), FabricError> {
    let operation = state.operations.get(operation_id).cloned().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::OperationUnknown,
            "routed operation does not exist",
        )
    })?;
    if operation.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "routed operation belongs to another Company",
        ));
    }
    if operation.expires_at_unix_ms <= now_unix_ms {
        return Err(FabricError::none(
            FabricErrorCode::OperationExpired,
            "routed operation expired before retry",
        ));
    }
    if let Some(inbox) = state.inboxes.get(operation_id) {
        if matches!(
            inbox.state,
            LocalInboxState::Applied | LocalInboxState::Rejected
        ) {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "terminal operation cannot be retried",
            ));
        }
        return Err(FabricError::unknown(
            operation_id,
            "target already persisted the operation; reconcile instead of blind retry",
        ));
    }
    let gateway = state
        .gateway_leases
        .get(&operation.target_node_id)
        .cloned()
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::TargetOffline,
                "target Node has no gateway lease",
            )
        })?;
    if gateway.company_id != company_id
        || gateway.control_plane_generation != control_plane_generation
        || gateway.expires_at_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::TargetOffline,
            "target Node gateway is offline or stale",
        ));
    }
    let mut attempts = state
        .attempts
        .values()
        .filter(|attempt| attempt.operation_id == operation_id)
        .cloned()
        .collect::<Vec<_>>();
    attempts.sort_by_key(|attempt| attempt.attempt_no);
    let prior = attempts.last().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "routed operation has no prior attempt",
        )
    })?;
    if prior.target_gateway_generation == gateway.gateway_generation {
        let receipt = receipt_for(
            state,
            operation_id,
            ReceiptKind::ControlPlaneAccepted,
            gateway.gateway_generation,
        )?;
        return Ok((prior.clone(), receipt, true));
    }
    if prior.effect != EffectCertainty::None
        || !matches!(
            prior.state,
            RouteAttemptState::Queued | RouteAttemptState::Sent
        )
    {
        return Err(FabricError::unknown(
            operation_id,
            "prior route attempt may have produced an effect; reconcile is required",
        ));
    }
    let prior_id = prior.id.clone();
    if let Some(prior_mut) = state.attempts.get_mut(&prior_id) {
        prior_mut.state = RouteAttemptState::Ended;
        prior_mut.error_code = Some(FabricErrorCode::NodeStaleGeneration);
        prior_mut.ended_at_unix_ms = Some(now_unix_ms);
    }
    let route_seq = state
        .route_sequences
        .get(&operation.target_node_id)
        .copied()
        .unwrap_or(0)
        .saturating_add(1);
    let attempt_no = prior.attempt_no.saturating_add(1);
    let attempt = RouteAttempt {
        id: format!("route-attempt:{operation_id}:{attempt_no}"),
        company_id: company_id.into(),
        operation_id: operation_id.into(),
        attempt_no,
        target_node_id: operation.target_node_id.clone(),
        target_gateway_generation: gateway.gateway_generation,
        control_plane_generation,
        route_seq,
        state: RouteAttemptState::Queued,
        error_code: None,
        effect: EffectCertainty::None,
        started_at_unix_ms: now_unix_ms,
        ended_at_unix_ms: None,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    let receipt = RouteReceipt {
        id: receipt_id(
            operation_id,
            ReceiptKind::ControlPlaneAccepted,
            gateway.gateway_generation,
        ),
        company_id: company_id.into(),
        operation_id: operation_id.into(),
        target_node_id: operation.target_node_id.clone(),
        target_gateway_generation: gateway.gateway_generation,
        control_plane_generation,
        route_seq,
        kind: ReceiptKind::ControlPlaneAccepted,
        result_schema: None,
        result: None,
        result_digest: None,
        error: None,
        created_at_unix_ms: now_unix_ms,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state
        .route_sequences
        .insert(operation.target_node_id, route_seq);
    state.attempts.insert(attempt.id.clone(), attempt.clone());
    state.receipts.insert(receipt.id.clone(), receipt.clone());
    if let Some(outbox) = state.outboxes.get_mut(operation_id) {
        outbox.attempt_count = outbox.attempt_count.saturating_add(1);
        outbox.last_attempt_at_unix_ms = Some(now_unix_ms);
    }
    Ok((attempt, receipt, false))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_application_result(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    operation_id: &str,
    result_schema: &str,
    result: serde_json::Value,
    applied: bool,
    now_unix_ms: u64,
) -> Result<(LocalRemoteInbox, RouteReceipt, bool), FabricError> {
    require_current_gateway(
        state,
        company_id,
        control_plane_generation,
        node_id,
        gateway_generation,
        now_unix_ms,
    )?;
    let result_digest = canonical_digest(&result)?;
    let receipt_kind = if applied {
        ReceiptKind::OperationApplied
    } else {
        ReceiptKind::OperationRejected
    };
    if let Some(existing_receipt) = state
        .receipts
        .get(&receipt_id(operation_id, receipt_kind, gateway_generation))
        .cloned()
    {
        if existing_receipt.result_digest.as_deref() != Some(result_digest.as_str())
            || existing_receipt.result_schema.as_deref() != Some(result_schema)
        {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "application result replay changed its fingerprint",
            ));
        }
        let inbox = state.inboxes.get(operation_id).cloned().ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "terminal receipt has no inbox record",
            )
        })?;
        return Ok((inbox, existing_receipt, true));
    }
    let inbox = state.inboxes.get_mut(operation_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::OperationUnknown,
            "operation was not persisted in the target inbox",
        )
    })?;
    if inbox.node_id != node_id
        || inbox.gateway_generation != gateway_generation
        || !matches!(
            inbox.state,
            LocalInboxState::Persisted | LocalInboxState::Claimed
        )
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "application result does not match the current persisted inbox generation",
        ));
    }
    inbox.state = if applied {
        LocalInboxState::Applied
    } else {
        LocalInboxState::Rejected
    };
    inbox.result_digest = Some(result_digest.clone());
    let inbox = inbox.clone();
    let attempt_id = state
        .attempts
        .values()
        .find(|attempt| {
            attempt.operation_id == operation_id
                && attempt.route_seq == inbox.route_seq
                && attempt.target_gateway_generation == gateway_generation
        })
        .map(|attempt| attempt.id.clone())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "terminal result has no matching route attempt",
            )
        })?;
    let attempt = state.attempts.get_mut(&attempt_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "terminal result has no route attempt",
        )
    })?;
    attempt.state = RouteAttemptState::Ended;
    attempt.effect = if applied {
        EffectCertainty::Applied
    } else {
        EffectCertainty::None
    };
    attempt.ended_at_unix_ms = Some(now_unix_ms);
    let receipt = RouteReceipt {
        id: receipt_id(operation_id, receipt_kind, gateway_generation),
        company_id: company_id.into(),
        operation_id: operation_id.into(),
        target_node_id: node_id.into(),
        target_gateway_generation: gateway_generation,
        control_plane_generation,
        route_seq: inbox.route_seq,
        kind: receipt_kind,
        result_schema: Some(result_schema.into()),
        result: Some(result),
        result_digest: Some(result_digest),
        error: None,
        created_at_unix_ms: now_unix_ms,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state.receipts.insert(receipt.id.clone(), receipt.clone());
    if let Some(outbox) = state.outboxes.get_mut(operation_id) {
        outbox.local_state = LocalOutboxState::Terminal;
        outbox.terminal_receipt_ref = Some(receipt.id.clone());
    }
    Ok((inbox, receipt, false))
}

pub(crate) fn mark_unknown(
    state: &mut FabricState,
    company_id: &str,
    operation_id: &str,
) -> Result<(), FabricError> {
    let inbox = state.inboxes.get_mut(operation_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::OperationUnknown,
            "operation has no target inbox",
        )
    })?;
    if inbox.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "operation belongs to another Company",
        ));
    }
    if matches!(
        inbox.state,
        LocalInboxState::Applied | LocalInboxState::Rejected
    ) {
        return Ok(());
    }
    inbox.state = LocalInboxState::RecoveryRequired;
    if let Some(attempt) = state
        .attempts
        .get_mut(&format!("route-attempt:{operation_id}:1"))
    {
        attempt.effect = EffectCertainty::Unknown;
    }
    Ok(())
}

fn validate_operation(
    state: &FabricState,
    company_id: &str,
    operation: &RoutedOperation,
    now_unix_ms: u64,
) -> Result<(), FabricError> {
    if operation.company_id != company_id || operation.actor.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "operation or actor belongs to another Company",
        ));
    }
    operation
        .actor
        .require_company_and_role(company_id, "fabric_submit", now_unix_ms)?;
    if operation.protocol_version != FABRIC_PROTOCOL_VERSION
        || operation.schema_version != FABRIC_SCHEMA_VERSION
    {
        return Err(FabricError::none(
            FabricErrorCode::ProtocolIncompatible,
            "operation protocol or schema is incompatible",
        ));
    }
    operation.validate_digest()?;
    if operation.control_plane_generation
        != state
            .control_plane_leases
            .get(company_id)
            .map(|lease| lease.control_plane_generation)
            .unwrap_or_default()
    {
        return Err(FabricError::none(
            FabricErrorCode::ControlPlaneStaleGeneration,
            "operation was created for a stale Control Plane generation",
        ));
    }
    if operation.expires_at_unix_ms <= now_unix_ms {
        return Err(FabricError::none(
            FabricErrorCode::OperationExpired,
            "operation expired before acceptance",
        ));
    }
    if !matches!(
        operation.kind.as_str(),
        "fabric.probe.v1"
            | "fabric.reconcile_probe.v1"
            | "runtime_command.reference.v1"
            | "message.reference.v1"
            | "delivery_intent.reference.v1"
            | "artifact.reference.v1"
    ) {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "operation kind is not in the versioned Remote Fabric registry",
        ));
    }
    let required_capability = match operation.kind.as_str() {
        "fabric.probe.v1" | "fabric.reconcile_probe.v1" => "durable-routing",
        "runtime_command.reference.v1" => "remote-runtime",
        "message.reference.v1" | "delivery_intent.reference.v1" => "remote-message",
        "artifact.reference.v1" => "artifact-transfer",
        _ => unreachable!("closed operation registry was checked above"),
    };
    for node_id in [&operation.source_node_id, &operation.target_node_id] {
        let node = state.nodes.get(node_id).ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::TargetNotPlaced,
                format!("operation Node {node_id} is not enrolled"),
            )
        })?;
        if node.company_id != company_id
            || node.administrative_status == NodeAdministrativeStatus::Revoked
        {
            return Err(FabricError::none(
                FabricErrorCode::NodeRevoked,
                "operation source or target Node is revoked or foreign",
            ));
        }
        if !node.allowed_capabilities.contains(required_capability) {
            return Err(FabricError::none(
                FabricErrorCode::FeatureIncompatible,
                format!("operation Node {node_id} lacks {required_capability} capability"),
            ));
        }
    }
    require_current_gateway(
        state,
        company_id,
        operation.control_plane_generation,
        &operation.source_node_id,
        operation.source_gateway_generation,
        now_unix_ms,
    )?;
    Ok(())
}

pub(crate) fn receipt_id(operation_id: &str, kind: ReceiptKind, gateway_generation: u64) -> String {
    format!("receipt:{operation_id}:{kind:?}:{gateway_generation}")
}

fn receipt_for(
    state: &FabricState,
    operation_id: &str,
    kind: ReceiptKind,
    gateway_generation: u64,
) -> Result<RouteReceipt, FabricError> {
    state
        .receipts
        .get(&receipt_id(operation_id, kind, gateway_generation))
        .cloned()
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "durable receipt is missing",
            )
        })
}
