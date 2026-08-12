use std::collections::BTreeSet;

use crate::node_gateway::require_current_gateway;
use crate::protocol::*;
use crate::store::FabricState;
use crate::{FabricError, FabricErrorCode};

#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    node_daemon_id: &str,
    node_daemon_generation: u64,
    operation_ids: &BTreeSet<String>,
    now_unix_ms: u64,
) -> Result<Vec<RouteReceipt>, FabricError> {
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
    let mut results = Vec::new();
    for operation_id in operation_ids {
        let Some(operation) = state.operations.get(operation_id) else {
            // An empty result is the current generation-fenced proof that the
            // Control Plane has never accepted this id. The source may then
            // rebind its durable pre-acceptance outbox to the successor
            // gateway generation without creating a second route truth.
            continue;
        };
        let owns_route = operation.target_node_id == node_id
            || operation.source_node_id.as_deref() == Some(node_id);
        if operation.company_id != company_id || !owns_route {
            return Err(FabricError::none(
                FabricErrorCode::SourceMismatch,
                "reconcile request does not own the operation source or target",
            ));
        }
        let operation_receipts = state
            .receipts
            .values()
            .filter(|receipt| receipt.operation_id == *operation_id)
            .cloned()
            .collect::<Vec<_>>();
        let terminal = operation_receipts
            .iter()
            .filter(|receipt| {
                matches!(
                    receipt.kind,
                    ReceiptKind::RecoveryRequired
                        | ReceiptKind::OperationApplied
                        | ReceiptKind::OperationRejected
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        if terminal.is_empty() {
            let unknown = state.attempts.values().any(|attempt| {
                attempt.operation_id == *operation_id
                    && attempt.target_node_id == node_id
                    && attempt.effect == EffectCertainty::Unknown
            });
            if unknown {
                return Err(FabricError::unknown(
                    operation_id,
                    "application effect is unknown; blind replay is forbidden",
                ));
            }
            results.extend(operation_receipts);
            continue;
        }
        results.extend(terminal);
    }
    Ok(results)
}
