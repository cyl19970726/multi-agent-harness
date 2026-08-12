use crate::protocol::*;
use crate::store::FabricState;
use crate::transport::VerifiedMtlsPeer;
use crate::{FabricError, FabricErrorCode, FABRIC_PROTOCOL_VERSION, FABRIC_SCHEMA_VERSION};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

pub(crate) fn connect(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    peer: &VerifiedMtlsPeer,
    hello: &NodeHello,
    proof: &NodeHelloProof,
    now_unix_ms: u64,
) -> Result<NodeWelcome, FabricError> {
    verify_hello_proof(company_id, control_plane_generation, hello, proof)?;
    connect_verified(
        state,
        company_id,
        control_plane_generation,
        peer,
        hello,
        now_unix_ms,
    )
}

pub(crate) fn connect_mtls(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    peer: &VerifiedMtlsPeer,
    hello: &NodeHello,
    now_unix_ms: u64,
) -> Result<NodeWelcome, FabricError> {
    connect_verified(
        state,
        company_id,
        control_plane_generation,
        peer,
        hello,
        now_unix_ms,
    )
}

fn connect_verified(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    peer: &VerifiedMtlsPeer,
    hello: &NodeHello,
    now_unix_ms: u64,
) -> Result<NodeWelcome, FabricError> {
    peer.validate_node_hello(hello)?;
    if hello.company_id != company_id {
        return Err(FabricError::none(
            FabricErrorCode::WrongCompany,
            "NodeHello belongs to another Company",
        ));
    }
    if hello.node_daemon_id.trim().is_empty() || hello.node_daemon_generation == 0 {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "NodeGateway requires an exact current NodeDaemon parent generation",
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
    if !hello.features.is_subset(&node.allowed_capabilities) {
        return Err(FabricError::none(
            FabricErrorCode::FeatureIncompatible,
            "NodeHello advertises capabilities that enrollment did not authorize",
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
    if prior.as_ref().is_some_and(|lease| {
        lease.expires_at_unix_ms > now_unix_ms
            && lease.control_plane_generation == control_plane_generation
    }) {
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
        node_daemon_id: hello.node_daemon_id.clone(),
        node_daemon_generation: hello.node_daemon_generation,
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
    rebind_effect_none_attempts_to_successor(
        state,
        company_id,
        control_plane_generation,
        &hello.node_id,
        gateway_generation,
        now_unix_ms,
    )?;
    if let Some(node) = state.nodes.get_mut(&hello.node_id) {
        node.last_seen_at_unix_ms = Some(now_unix_ms);
        node.updated_at_unix_ms = now_unix_ms;
    }
    // The Node-local inbox is the sole source of unresolved application effects.
    // The Control Plane must never fabricate or mirror this local recovery truth.
    let required_reconcile_ids = hello.unresolved_operation_ids.clone();
    Ok(NodeWelcome {
        company_id: company_id.into(),
        node_id: hello.node_id.clone(),
        accepted_protocol_version: FABRIC_PROTOCOL_VERSION,
        lease_id: lease.lease_id,
        gateway_generation,
        node_daemon_id: hello.node_daemon_id.clone(),
        node_daemon_generation: hello.node_daemon_generation,
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

fn rebind_effect_none_attempts_to_successor(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    now_unix_ms: u64,
) -> Result<(), FabricError> {
    let operation_ids = state
        .operations
        .values()
        .filter(|operation| {
            operation.company_id == company_id
                && operation.target_node_id == node_id
                && operation.expires_at_unix_ms > now_unix_ms
        })
        .filter_map(|operation| {
            let mut attempts = state
                .attempts
                .values()
                .filter(|attempt| attempt.operation_id == operation.id)
                .collect::<Vec<_>>();
            attempts.sort_by_key(|attempt| attempt.attempt_no);
            let latest = attempts.last()?;
            let has_persisted_or_terminal = state.receipts.values().any(|receipt| {
                receipt.operation_id == operation.id
                    && matches!(
                        receipt.kind,
                        ReceiptKind::TargetPersisted
                            | ReceiptKind::OperationApplied
                            | ReceiptKind::OperationRejected
                    )
            });
            (latest.target_gateway_generation != gateway_generation
                && latest.effect == EffectCertainty::None
                && matches!(
                    latest.state,
                    RouteAttemptState::Queued | RouteAttemptState::Sent
                )
                && !has_persisted_or_terminal)
                .then(|| operation.id.clone())
        })
        .collect::<Vec<_>>();
    for operation_id in operation_ids {
        crate::router::retry_operation(
            state,
            company_id,
            control_plane_generation,
            &operation_id,
            now_unix_ms,
        )?;
    }
    Ok(())
}

pub fn node_hello_challenge(
    company_id: &str,
    control_plane_generation: u64,
    hello: &NodeHello,
) -> Result<String, FabricError> {
    Ok(format!(
        "agentfirm.remote_fabric.v1:hello:{company_id}:{control_plane_generation}:{}",
        crate::json_digest(hello)?
    ))
}

fn verify_hello_proof(
    company_id: &str,
    control_plane_generation: u64,
    hello: &NodeHello,
    proof: &NodeHelloProof,
) -> Result<(), FabricError> {
    let expected = node_hello_challenge(company_id, control_plane_generation, hello)?;
    if proof.challenge != expected {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "NodeHello proof does not bind the exact connection challenge",
        ));
    }
    let public_key: [u8; 32] = proof.public_key.as_slice().try_into().map_err(|_| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "NodeHello Ed25519 public key must contain exactly 32 bytes",
        )
    })?;
    let signature: [u8; 64] = proof.signature.as_slice().try_into().map_err(|_| {
        FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "NodeHello Ed25519 signature must contain exactly 64 bytes",
        )
    })?;
    if crate::sha256_hex(public_key) != hello.public_key_fingerprint {
        return Err(FabricError::none(
            FabricErrorCode::UnauthorizedActor,
            "NodeHello public key does not match its certificate fingerprint",
        ));
    }
    VerifyingKey::from_bytes(&public_key)
        .and_then(|key| key.verify(expected.as_bytes(), &Signature::from_bytes(&signature)))
        .map_err(|_| {
            FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "NodeHello proof-of-possession signature is invalid",
            )
        })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn heartbeat(
    state: &mut FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    node_daemon_id: &str,
    node_daemon_generation: u64,
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
        || lease.node_daemon_id != node_daemon_id
        || lease.node_daemon_generation != node_daemon_generation
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn require_current_gateway<'a>(
    state: &'a FabricState,
    company_id: &str,
    control_plane_generation: u64,
    node_id: &str,
    gateway_generation: u64,
    node_daemon_id: &str,
    node_daemon_generation: u64,
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
        || lease.node_daemon_id != node_daemon_id
        || lease.node_daemon_generation != node_daemon_generation
        || lease.expires_at_unix_ms <= now_unix_ms
    {
        return Err(FabricError::none(
            FabricErrorCode::NodeStaleGeneration,
            "gateway or Control Plane generation is stale",
        ));
    }
    Ok(lease)
}
