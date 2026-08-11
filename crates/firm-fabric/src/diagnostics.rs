use crate::protocol::{
    EffectCertainty, NodeAdministrativeStatus, NodeConnectionStatus, ReceiptKind, RouteAttemptState,
};
use crate::{FabricError, FabricStore};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NodeFabricDiagnostics {
    pub node_id: String,
    pub administrative_status: NodeAdministrativeStatus,
    pub connection_status: NodeConnectionStatus,
    pub gateway_generation: Option<u64>,
    pub control_plane_generation: Option<u64>,
    pub certificate_expires_at_unix_ms: Option<u64>,
    pub queued_operations: usize,
    pub oldest_queued_age_ms: u64,
    pub gateway_lease_age_ms: Option<u64>,
    pub recovery_required_operations: Vec<String>,
    pub last_assigned_route_seq: u64,
    pub last_persisted_route_seq: u64,
    pub reconcile_lag: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FabricDiagnostics {
    pub company_id: String,
    pub store_revision: u64,
    pub current_control_plane_generation: Option<u64>,
    pub control_plane_online: bool,
    pub nodes: Vec<NodeFabricDiagnostics>,
    pub recovery_required_count: usize,
}

pub fn inspect_fabric(
    store: &FabricStore,
    company_id: &str,
    now_unix_ms: u64,
) -> Result<FabricDiagnostics, FabricError> {
    let state = store.snapshot()?;
    let control_plane = state.control_plane_leases.get(company_id);
    let current_generation = control_plane.map(|lease| lease.control_plane_generation);
    let mut nodes = state
        .nodes
        .values()
        .filter(|node| node.company_id == company_id)
        .map(|node| {
            let gateway = state.gateway_leases.get(&node.id);
            let recovery_required_operations = state
                .attempts
                .values()
                .filter(|attempt| {
                    attempt.company_id == company_id
                        && attempt.target_node_id == node.id
                        && attempt.effect == EffectCertainty::Unknown
                })
                .map(|attempt| attempt.operation_id.clone())
                .collect::<Vec<_>>();
            let queued_operations = state
                .attempts
                .values()
                .filter(|attempt| {
                    attempt.company_id == company_id
                        && attempt.target_node_id == node.id
                        && attempt.state != RouteAttemptState::Ended
                })
                .count();
            let oldest_queued_age_ms = state
                .attempts
                .values()
                .filter(|attempt| {
                    attempt.company_id == company_id
                        && attempt.target_node_id == node.id
                        && attempt.state != RouteAttemptState::Ended
                })
                .map(|attempt| now_unix_ms.saturating_sub(attempt.started_at_unix_ms))
                .max()
                .unwrap_or_default();
            let connection_status = node.connection_status(
                gateway,
                current_generation.unwrap_or_default(),
                now_unix_ms,
            );
            let last_assigned_route_seq = state
                .route_sequences
                .get(&node.id)
                .copied()
                .unwrap_or_default();
            let last_persisted_route_seq = state
                .receipts
                .values()
                .filter(|receipt| {
                    receipt.company_id == company_id
                        && receipt.target_node_id == node.id
                        && receipt.kind == ReceiptKind::TargetPersisted
                })
                .map(|receipt| receipt.route_seq)
                .max()
                .unwrap_or_default();
            NodeFabricDiagnostics {
                node_id: node.id.clone(),
                administrative_status: node.administrative_status,
                connection_status,
                gateway_generation: gateway.map(|lease| lease.gateway_generation),
                control_plane_generation: gateway.map(|lease| lease.control_plane_generation),
                certificate_expires_at_unix_ms: state
                    .certificates
                    .get(&node.certificate_serial)
                    .map(|certificate| certificate.expires_at_unix_ms),
                queued_operations,
                oldest_queued_age_ms,
                gateway_lease_age_ms: gateway
                    .map(|lease| now_unix_ms.saturating_sub(lease.last_heartbeat_at_unix_ms)),
                recovery_required_operations,
                last_assigned_route_seq,
                last_persisted_route_seq,
                reconcile_lag: last_assigned_route_seq.saturating_sub(last_persisted_route_seq),
            }
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    let recovery_required_count = nodes
        .iter()
        .map(|node| node.recovery_required_operations.len())
        .sum();
    Ok(FabricDiagnostics {
        company_id: company_id.into(),
        store_revision: state.revision,
        current_control_plane_generation: current_generation,
        control_plane_online: control_plane
            .is_some_and(|lease| lease.expires_at_unix_ms > now_unix_ms),
        nodes,
        recovery_required_count,
    })
}
