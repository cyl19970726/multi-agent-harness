use std::collections::BTreeSet;

use crate::protocol::*;
use crate::store::FabricState;
use crate::{FabricError, FabricErrorCode, FABRIC_PROTOCOL_VERSION, FABRIC_SCHEMA_VERSION};

pub(crate) fn connect(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    hello: &NodeHello,
    now_unix_ms: u64,
) -> Result<NodeWelcome, FabricError> {
    if hello.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "NodeHello belongs to another Company",
        ));
    }
    if hello.protocol_min > FABRIC_PROTOCOL_VERSION || hello.protocol_max < FABRIC_PROTOCOL_VERSION
    {
        return Err(FabricError::none(
            FabricErrorCode::ProtocolIncompatible,
            "Node and Control Plane have no common protocol major",
        ));
    }
    let node = state.nodes.get(&hello.node_id).cloned().ok_or_else(|| {
        FabricError::none(FabricErrorCode::SourceMismatch, "Node is not enrolled")
    })?;
    if node.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "Node belongs to another Company",
        ));
    }
    if node.administrative_status == NodeAdministrativeStatus::Revoked {
        return Err(FabricError::none(
            FabricErrorCode::NodeRevoked,
            "revoked Node cannot connect",
        ));
    }
    if node.schema_bundle_digest != hello.schema_bundle_digest {
        return Err(FabricError::none(
            FabricErrorCode::SchemaIncompatible,
            "Node schema bundle does not match enrolled contract",
        ));
    }
    if node.certificate_serial != hello.certificate_serial
        || node.public_key_fingerprint != hello.public_key_fingerprint
        || state
            .revoked_certificate_serials
            .contains(&hello.certificate_serial)
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "mTLS certificate binding is invalid or revoked",
        ));
    }
    let certificate = state
        .certificates
        .get(&hello.certificate_serial)
        .ok_or_else(|| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "mTLS certificate is unknown",
            )
        })?;
    if certificate.company_id != company_id
        || certificate.node_id != node.id
        || certificate.public_key_fingerprint != hello.public_key_fingerprint
        || certificate.expires_at_unix_ms <= now_unix_ms
        || certificate.revoked_at_unix_ms.is_some()
    {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "mTLS certificate claim or lifetime is invalid",
        ));
    }
    let prior = state.gateway_leases.get(&hello.node_id).cloned();
    if prior
        .as_ref()
        .is_some_and(|lease| lease.expires_at_unix_ms > now_unix_ms)
    {
        return Err(FabricError::none(
            FabricErrorCode::LeaseConflict,
            "Node already has an active gateway generation",
        ));
    }
    let gateway_generation = prior
        .as_ref()
        .map_or(1, |lease| lease.gateway_generation.saturating_add(1));
    let lease = NodeGatewayLease {
        company_id: company_id.into(),
        node_id: hello.node_id.clone(),
        lease_id: format!("gateway-lease:{}:{}", hello.node_id, gateway_generation),
        gateway_generation,
        instance_id: hello.instance_id.clone(),
        revision: prior
            .as_ref()
            .map_or(1, |lease| lease.revision.saturating_add(1)),
        control_plane_generation,
        acquired_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: now_unix_ms.saturating_add(30_000),
        last_heartbeat_at_unix_ms: now_unix_ms,
        protocol_version: FABRIC_PROTOCOL_VERSION,
        build_sha: hello.build_sha.clone(),
        schema_bundle_digest: hello.schema_bundle_digest.clone(),
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    };
    state
        .gateway_leases
        .insert(hello.node_id.clone(), lease.clone());
    if let Some(node) = state.nodes.get_mut(&hello.node_id) {
        node.last_seen_at_unix_ms = Some(now_unix_ms);
        node.updated_at_unix_ms = now_unix_ms;
    }
    let required_reconcile_ids = state
        .inboxes
        .values()
        .filter(|inbox| {
            inbox.node_id == hello.node_id && inbox.state == LocalInboxState::RecoveryRequired
        })
        .map(|inbox| inbox.operation_id.clone())
        .chain(hello.unresolved_operation_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    Ok(NodeWelcome {
        company_id: company_id.into(),
        node_id: hello.node_id.clone(),
        accepted_protocol_version: FABRIC_PROTOCOL_VERSION,
        lease_id: lease.lease_id,
        gateway_generation,
        control_plane_generation,
        next_route_seq: state
            .route_sequences
            .get(&hello.node_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(1),
        required_reconcile_ids,
        schema_version: FABRIC_SCHEMA_VERSION.into(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn heartbeat(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    expected_revision: u64,
    now_unix_ms: u64,
) -> Result<NodeGatewayLease, FabricError> {
    let lease = state.gateway_leases.get(node_id).cloned().ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "Node has no gateway lease",
        )
    })?;
    if lease.company_id != company_id
        || lease.control_plane_generation != control_plane_generation
        || lease.gateway_generation != gateway_generation
        || lease.expires_at_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "gateway generation is stale",
        ));
    }
    if lease.revision != expected_revision {
        return Err(crate::control_plane::revision_conflict(
            "gateway lease revision mismatch",
            expected_revision,
            lease.revision,
        ));
    }
    let node = state
        .nodes
        .get(node_id)
        .ok_or_else(|| FabricError::none(FabricErrorCode::SourceMismatch, "Node does not exist"))?;
    if node.administrative_status == NodeAdministrativeStatus::Revoked {
        return Err(FabricError::none(
            FabricErrorCode::NodeRevoked,
            "revoked Node cannot heartbeat",
        ));
    }
    let mut next = lease;
    next.revision = next.revision.saturating_add(1);
    next.last_heartbeat_at_unix_ms = now_unix_ms;
    next.expires_at_unix_ms = now_unix_ms.saturating_add(30_000);
    state.gateway_leases.insert(node_id.into(), next.clone());
    if let Some(node) = state.nodes.get_mut(node_id) {
        node.last_seen_at_unix_ms = Some(now_unix_ms);
        node.updated_at_unix_ms = now_unix_ms;
    }
    Ok(next)
}

pub(crate) fn require_current_gateway<'a>(
    state: &'a FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    now_unix_ms: u64,
) -> Result<&'a NodeGatewayLease, FabricError> {
    let lease = state.gateway_leases.get(node_id).ok_or_else(|| {
        FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "Node has no current gateway lease",
        )
    })?;
    if lease.company_id != company_id
        || lease.control_plane_generation != control_plane_generation
        || lease.gateway_generation != gateway_generation
        || lease.expires_at_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "gateway or Control Plane generation is stale",
        ));
    }
    Ok(lease)
}
