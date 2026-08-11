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
    operation_ids: &BTreeSet<String>,
    now_unix_ms: u64,
) -> Result<Vec<RouteReceipt>, FabricError> {
    require_current_gateway(
        state,
        company_id,
        control_plane_generation,
        node_id,
        gateway_generation,
        now_unix_ms,
    )?;
    let mut results = Vec::new();
    for operation_id in operation_ids {
        let operation = state.operations.get(operation_id).ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::OperationUnknown,
                format!("operation {operation_id} is unknown"),
            )
        })?;
        if operation.company_id != company_id || operation.target_node_id != node_id {
            return Err(FabricError::none(
                FabricErrorCode::SourceMismatch,
                "reconcile request does not own the operation target",
            ));
        }
        let terminal = state
            .receipts
            .values()
            .filter(|receipt| {
                receipt.operation_id == *operation_id
                    && receipt.target_gateway_generation == gateway_generation
                    && matches!(
                        receipt.kind,
                        ReceiptKind::OperationApplied | ReceiptKind::OperationRejected
                    )
            })
            .cloned()
            .collect::<Vec<_>>();
        if terminal.is_empty() {
            let inbox = state.inboxes.get(operation_id);
            if inbox.is_some_and(|inbox| inbox.state == LocalInboxState::RecoveryRequired) {
                return Err(FabricError::unknown(
                    operation_id,
                    "application effect is unknown; blind replay is forbidden",
                ));
            }
            return Err(FabricError::none(
                FabricErrorCode::OperationUnknown,
                "operation has no terminal application receipt",
            ));
        }
        results.extend(terminal);
    }
    Ok(results)
}
