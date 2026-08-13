use crate::node_gateway::require_current_gateway;
use crate::protocol::*;
use crate::store::{FabricState, FabricStoreLimits};
use crate::{
    canonical_digest, FabricError, FabricErrorCode, FABRIC_CANONICALIZATION_VERSION,
    FABRIC_PROTOCOL_VERSION, FABRIC_SCHEMA_VERSION,
};

fn operation_source_key(operation: &RoutedOperation) -> String {
    match operation.source_authority {
        OperationSourceAuthority::Node => format!(
            "node:{}",
            operation.source_node_id.as_deref().unwrap_or("missing")
        ),
        OperationSourceAuthority::ControlPlane => "control_plane".into(),
    }
}

fn queue_capacity_exceeded(
    queued_operations: usize,
    queued_bytes: u64,
    operation_bytes: u64,
    limits: FabricStoreLimits,
) -> bool {
    queued_operations >= limits.max_queued_operations_per_node
        || queued_bytes.saturating_add(operation_bytes) > limits.max_queued_bytes_per_node
}

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
    let source_authority_key = operation_source_key(&operation);
    let idempotency_index = format!(
        "{}:{}:{}",
        company_id, source_authority_key, operation.idempotency_key
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
        company_id, source_authority_key, operation.actor.actor_id
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
    if queue_capacity_exceeded(queued, queued_bytes, operation_bytes, limits) {
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
    let ordering_index = format!("{}:{}", operation.target_node_id, operation.ordering_key);
    let ordering_seq = state
        .ordering_sequences
        .get(&ordering_index)
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
        ordering_seq,
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
        application_effect: None,
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
        .ordering_sequences
        .insert(ordering_index, ordering_seq);
    state
        .operations
        .insert(operation.id.clone(), operation.clone());
    state.attempts.insert(attempt.id.clone(), attempt.clone());
    state.receipts.insert(receipt.id.clone(), receipt.clone());
    let window = state
        .rate_windows
        .entry(rate_key)
        .or_insert(FabricRateWindow {
            company_id: company_id.into(),
            source_authority_key,
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
    if state.receipts.values().any(|receipt| {
        receipt.operation_id == operation_id
            && matches!(
                receipt.kind,
                ReceiptKind::OperationApplied | ReceiptKind::OperationRejected
            )
    }) {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "terminal operation cannot be retried",
        ));
    }
    if state.receipts.values().any(|receipt| {
        receipt.operation_id == operation_id && receipt.kind == ReceiptKind::TargetPersisted
    }) {
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
        ordering_seq: prior.ordering_seq,
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
        application_effect: None,
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
    Ok((attempt, receipt, false))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_target_persisted(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    node_daemon_id: &str,
    node_daemon_generation: u64,
    operation_id: &str,
    request_digest: &str,
    route_seq: u64,
    now_unix_ms: u64,
) -> Result<(RouteAttempt, RouteReceipt, bool), FabricError> {
    require_current_gateway(
        state,
        company_id,
        control_plane_generation,
        node_id,
        gateway_generation,
        node_daemon_id,
        node_daemon_generation,
        now_unix_ms,
    )?;
    let operation = state.operations.get(operation_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::OperationUnknown,
            "target persisted receipt references an unknown operation",
        )
    })?;
    if operation.company_id != company_id
        || operation.target_node_id != node_id
        || canonical_digest(operation)? != request_digest
    {
        return Err(FabricError::none(
            FabricErrorCode::SourceMismatch,
            "target persisted receipt does not match the routed operation",
        ));
    }
    let attempt_id = state
        .attempts
        .values()
        .find(|attempt| {
            attempt.operation_id == operation_id
                && attempt.route_seq == route_seq
                && attempt.target_gateway_generation == gateway_generation
                && attempt.control_plane_generation == control_plane_generation
        })
        .map(|attempt| attempt.id.clone())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "target persisted receipt has no matching route attempt",
            )
        })?;
    let receipt_key = receipt_id(
        operation_id,
        ReceiptKind::TargetPersisted,
        gateway_generation,
    );
    if let Some(receipt) = state.receipts.get(&receipt_key).cloned() {
        let attempt = state.attempts.get(&attempt_id).cloned().ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "persisted receipt has no route attempt",
            )
        })?;
        return Ok((attempt, receipt, true));
    }
    let attempt = state.attempts.get_mut(&attempt_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "target persisted attempt disappeared",
        )
    })?;
    if !matches!(
        attempt.state,
        RouteAttemptState::Queued | RouteAttemptState::Sent
    ) {
        return Err(FabricError::none(
            FabricErrorCode::IdempotencyConflict,
            "target persisted receipt cannot regress route attempt state",
        ));
    }
    attempt.state = RouteAttemptState::TargetPersisted;
    let attempt = attempt.clone();
    let receipt = RouteReceipt {
        id: receipt_key,
        company_id: company_id.into(),
        operation_id: operation_id.into(),
        target_node_id: node_id.into(),
        target_gateway_generation: gateway_generation,
        control_plane_generation,
        route_seq,
        kind: ReceiptKind::TargetPersisted,
        application_effect: None,
        result_schema: None,
        result: None,
        result_digest: None,
        error: None,
        created_at_unix_ms: now_unix_ms,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state.receipts.insert(receipt.id.clone(), receipt.clone());
    Ok((attempt, receipt, false))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_application_receipt(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    node_daemon_id: &str,
    node_daemon_generation: u64,
    operation_id: &str,
    result_schema: &str,
    result: serde_json::Value,
    effect: EffectCertainty,
    now_unix_ms: u64,
) -> Result<(RouteAttempt, RouteReceipt, bool), FabricError> {
    require_current_gateway(
        state,
        company_id,
        control_plane_generation,
        node_id,
        gateway_generation,
        node_daemon_id,
        node_daemon_generation,
        now_unix_ms,
    )?;
    let operation = state.operations.get(operation_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::OperationUnknown,
            "application receipt references an unknown operation",
        )
    })?;
    if operation.company_id != company_id || operation.target_node_id != node_id {
        return Err(FabricError::none(
            FabricErrorCode::SourceMismatch,
            "application receipt does not match operation target authority",
        ));
    }
    let result_digest = canonical_digest(&result)?;
    let kind = match effect {
        EffectCertainty::Applied => ReceiptKind::OperationApplied,
        EffectCertainty::NotApplied => ReceiptKind::OperationRejected,
        EffectCertainty::Unknown => ReceiptKind::RecoveryRequired,
        EffectCertainty::None => {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "target result must prove applied, not_applied, or unknown",
            ));
        }
    };
    let receipt_key = receipt_id(operation_id, kind, gateway_generation);
    if let Some(receipt) = state.receipts.get(&receipt_key).cloned() {
        if receipt.result_digest.as_deref() != Some(result_digest.as_str())
            || receipt.result_schema.as_deref() != Some(result_schema)
            || receipt.application_effect != Some(effect)
        {
            return Err(FabricError::none(
                FabricErrorCode::IdempotencyConflict,
                "application receipt replay changed its fingerprint",
            ));
        }
        let attempt = state
            .attempts
            .values()
            .find(|attempt| {
                attempt.operation_id == operation_id
                    && attempt.target_gateway_generation == gateway_generation
            })
            .cloned()
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    "terminal receipt has no route attempt",
                )
            })?;
        return Ok((attempt, receipt, true));
    }
    let attempt_id = state
        .attempts
        .values()
        .find(|attempt| {
            attempt.operation_id == operation_id
                && attempt.target_gateway_generation == gateway_generation
                && attempt.control_plane_generation == control_plane_generation
        })
        .map(|attempt| attempt.id.clone())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "application receipt has no matching route attempt",
            )
        })?;
    let attempt = state.attempts.get_mut(&attempt_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "application receipt attempt disappeared",
        )
    })?;
    let expired_not_applied = effect == EffectCertainty::NotApplied
        && operation.expires_at_unix_ms <= now_unix_ms
        && matches!(
            attempt.state,
            RouteAttemptState::Queued | RouteAttemptState::Sent
        )
        && result_schema == "agentfirm.remote_fabric.expired.v1"
        && serde_json::from_value::<FabricError>(result.clone())
            .is_ok_and(|error| error.code == FabricErrorCode::OperationExpired);
    if attempt.state != RouteAttemptState::TargetPersisted && !expired_not_applied {
        return Err(FabricError::none(
            FabricErrorCode::ExpectedRevisionConflict,
            "application receipt requires prior target_persisted or exact expired NotApplied proof",
        ));
    }
    attempt.state = if effect == EffectCertainty::Unknown {
        RouteAttemptState::TargetPersisted
    } else {
        RouteAttemptState::Ended
    };
    // RouteAttempt records transport progress only. Application truth belongs
    // exclusively to the generation-fenced target receipt below.
    attempt.effect = EffectCertainty::None;
    attempt.ended_at_unix_ms = (effect != EffectCertainty::Unknown).then_some(now_unix_ms);
    let attempt = attempt.clone();
    let receipt = RouteReceipt {
        id: receipt_key,
        company_id: company_id.into(),
        operation_id: operation_id.into(),
        target_node_id: node_id.into(),
        target_gateway_generation: gateway_generation,
        control_plane_generation,
        route_seq: attempt.route_seq,
        kind,
        application_effect: Some(effect),
        result_schema: Some(result_schema.into()),
        result: Some(result),
        result_digest: Some(result_digest),
        error: expired_not_applied.then(|| {
            FabricError::none(
                FabricErrorCode::OperationExpired,
                "routed operation expired before target inbox persistence",
            )
        }),
        created_at_unix_ms: now_unix_ms,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state.receipts.insert(receipt.id.clone(), receipt.clone());
    Ok((attempt, receipt, false))
}

pub(crate) fn mark_unknown(
    state: &mut FabricState,
    company_id: &str,
    operation_id: &str,
) -> Result<(), FabricError> {
    let operation = state.operations.get(operation_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::OperationUnknown,
            "operation does not exist",
        )
    })?;
    if operation.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "operation belongs to another Company",
        ));
    }
    if state.receipts.values().any(|receipt| {
        receipt.operation_id == operation_id
            && matches!(
                receipt.kind,
                ReceiptKind::OperationApplied | ReceiptKind::OperationRejected
            )
    }) {
        return Ok(());
    }
    let attempt_id = state
        .attempts
        .values()
        .filter(|attempt| attempt.operation_id == operation_id)
        .max_by_key(|attempt| attempt.attempt_no)
        .map(|attempt| attempt.id.clone())
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::OperationUnknown,
                "operation has no route attempt",
            )
        })?;
    let attempt = state.attempts.get_mut(&attempt_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::StoreUnavailable,
            "route attempt disappeared before recovery marking",
        )
    })?;
    attempt.effect = EffectCertainty::Unknown;
    Ok(())
}

/// Settle expired, never-persisted offline work when a successor Gateway
/// reconnects. The Control Plane may prove only `not_applied`: no target inbox
/// or native application authority was ever reached. This terminal receipt is
/// durable reconciliation truth rather than a silent omission from the next
/// delivery batch.
pub(crate) fn expire_unapplied_for_successor(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    successor_gateway_generation: u64,
    now_unix_ms: u64,
) -> Result<Vec<RouteReceipt>, FabricError> {
    let operation_ids = state
        .operations
        .values()
        .filter(|operation| {
            operation.company_id == company_id
                && operation.target_node_id == node_id
                && operation.expires_at_unix_ms <= now_unix_ms
        })
        .filter_map(|operation| {
            let latest = state
                .attempts
                .values()
                .filter(|attempt| attempt.operation_id == operation.id)
                .max_by_key(|attempt| attempt.attempt_no)?;
            let already_settled = state.receipts.values().any(|receipt| {
                receipt.operation_id == operation.id
                    && matches!(
                        receipt.kind,
                        ReceiptKind::TargetPersisted
                            | ReceiptKind::RecoveryRequired
                            | ReceiptKind::OperationApplied
                            | ReceiptKind::OperationRejected
                    )
            });
            (latest.effect == EffectCertainty::None
                && matches!(
                    latest.state,
                    RouteAttemptState::Queued | RouteAttemptState::Sent
                )
                && !already_settled)
                .then(|| operation.id.clone())
        })
        .collect::<Vec<_>>();
    let mut receipts = Vec::new();
    for operation_id in operation_ids {
        let attempt_id = state
            .attempts
            .values()
            .filter(|attempt| attempt.operation_id == operation_id)
            .max_by_key(|attempt| attempt.attempt_no)
            .map(|attempt| attempt.id.clone())
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::OperationUnknown,
                    "expired operation has no route attempt",
                )
            })?;
        let attempt = state.attempts.get_mut(&attempt_id).ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "expired route attempt disappeared",
            )
        })?;
        attempt.state = RouteAttemptState::Ended;
        attempt.error_code = Some(FabricErrorCode::OperationExpired);
        attempt.effect = EffectCertainty::None;
        attempt.ended_at_unix_ms = Some(now_unix_ms);
        let error = FabricError::none(
            FabricErrorCode::OperationExpired,
            "offline routed operation expired before target persistence",
        );
        let result = serde_json::to_value(&error).map_err(|encode_error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("expired operation receipt could not be encoded: {encode_error}"),
            )
        })?;
        let receipt = RouteReceipt {
            id: receipt_id(
                &operation_id,
                ReceiptKind::OperationRejected,
                successor_gateway_generation,
            ),
            company_id: company_id.into(),
            operation_id: operation_id.clone(),
            target_node_id: node_id.into(),
            target_gateway_generation: successor_gateway_generation,
            control_plane_generation,
            route_seq: attempt.route_seq,
            kind: ReceiptKind::OperationRejected,
            application_effect: Some(EffectCertainty::NotApplied),
            result_schema: Some("agentfirm.remote_fabric.expired.v1".into()),
            result_digest: Some(canonical_digest(&result)?),
            result: Some(result),
            error: Some(error),
            created_at_unix_ms: now_unix_ms,
            schema_version: FABRIC_SCHEMA_VERSION.into(),
        };
        state.receipts.insert(receipt.id.clone(), receipt.clone());
        receipts.push(receipt);
    }
    Ok(receipts)
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
        || operation.canonicalization_version != FABRIC_CANONICALIZATION_VERSION
    {
        return Err(FabricError::none(
            FabricErrorCode::ProtocolIncompatible,
            "operation protocol or schema is incompatible",
        ));
    }
    operation.validate_digest()?;
    operation.closed_body()?;
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
        PROBE_OPERATION_KIND
            | RECONCILE_PROBE_OPERATION_KIND
            | RUNTIME_COMMAND_REFERENCE_KIND
            | MESSAGE_REFERENCE_KIND
            | DELIVERY_INTENT_REFERENCE_KIND
            | ARTIFACT_REFERENCE_KIND
            | COLLABORATION_BUSINESS_OPERATION_KIND
    ) {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "operation kind is not in the versioned Remote Fabric registry",
        ));
    }
    let required_capability = match operation.kind.as_str() {
        PROBE_OPERATION_KIND | RECONCILE_PROBE_OPERATION_KIND => "durable-routing",
        RUNTIME_COMMAND_REFERENCE_KIND => "remote-runtime",
        MESSAGE_REFERENCE_KIND | DELIVERY_INTENT_REFERENCE_KIND => "remote-message",
        ARTIFACT_REFERENCE_KIND => "artifact-transfer",
        COLLABORATION_BUSINESS_OPERATION_KIND => "cross-team-collaboration",
        _ => unreachable!("closed operation registry was checked above"),
    };
    let mut node_ids = vec![operation.target_node_id.as_str()];
    match operation.source_authority {
        OperationSourceAuthority::Node => {
            let source_node_id = operation.source_node_id.as_deref().ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::SourceMismatch,
                    "node-authority operation requires source_node_id",
                )
            })?;
            if operation.source_gateway_generation.is_none() {
                return Err(FabricError::none(
                    FabricErrorCode::SourceMismatch,
                    "node-authority operation requires source gateway generation",
                ));
            }
            if operation.source_node_daemon_id.is_none()
                || operation.source_node_daemon_generation.is_none()
            {
                return Err(FabricError::none(
                    FabricErrorCode::SourceMismatch,
                    "node-authority operation requires exact NodeDaemon id and generation",
                ));
            }
            node_ids.push(source_node_id);
        }
        OperationSourceAuthority::ControlPlane => {
            if operation.source_node_id.is_some()
                || operation.source_gateway_generation.is_some()
                || operation.source_node_daemon_id.is_some()
                || operation.source_node_daemon_generation.is_some()
                || operation.source_execution_space_id.is_some()
            {
                return Err(FabricError::none(
                    FabricErrorCode::SourceMismatch,
                    "Control Plane operation cannot claim Node source authority",
                ));
            }
        }
    }
    for node_id in node_ids {
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
    if state
        .nodes
        .get(&operation.target_node_id)
        .is_some_and(|node| node.administrative_status == NodeAdministrativeStatus::Draining)
    {
        return Err(FabricError::none(
            FabricErrorCode::TargetNotPlaced,
            "draining Node does not accept new routed operations",
        ));
    }
    if operation.source_authority == OperationSourceAuthority::Node {
        require_current_gateway(
            state,
            company_id,
            operation.control_plane_generation,
            operation
                .source_node_id
                .as_deref()
                .expect("validated Node source id"),
            operation
                .source_gateway_generation
                .expect("validated Node gateway generation"),
            operation
                .source_node_daemon_id
                .as_deref()
                .expect("validated NodeDaemon id"),
            operation
                .source_node_daemon_generation
                .expect("validated NodeDaemon generation"),
            now_unix_ms,
        )?;
    }
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

#[cfg(test)]
mod tests {
    use super::queue_capacity_exceeded;
    use crate::store::FabricStoreLimits;

    #[test]
    fn default_offline_queue_boundaries_are_exact() {
        let limits = FabricStoreLimits::default();
        assert!(!queue_capacity_exceeded(9_999, 0, 1, limits));
        assert!(queue_capacity_exceeded(10_000, 0, 1, limits));

        let one_gib = 1024_u64 * 1024 * 1024;
        assert!(!queue_capacity_exceeded(0, one_gib - 1, 1, limits));
        assert!(queue_capacity_exceeded(0, one_gib, 1, limits));
        assert!(queue_capacity_exceeded(0, u64::MAX, 1, limits));
    }
}
