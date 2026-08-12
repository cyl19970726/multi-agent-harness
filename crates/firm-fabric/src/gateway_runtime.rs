//! Authenticated application protocol for one outbound Node Gateway session.
//!
//! The TLS layer resolves machine identity. This module then binds every
//! application frame to the exact Company, NodeDaemon parent, gateway and
//! Control Plane generations before touching a durable journal.

use std::collections::BTreeSet;
use std::io::{Read, Write};

use tungstenite::WebSocket;

use crate::artifacts::ArtifactKeyBackend;
use crate::control_plane::ControlPlane;
use crate::protocol::*;
use crate::transport::{
    connect_outbound_mtls, connect_outbound_mtls_material, set_node_gateway_read_timeout,
    NodeFabricConfig, NodeGatewaySocket, NodeTlsIdentityFiles, NodeTlsIdentityMaterial,
};
use crate::transport::{read_frame, write_frame, FabricSessionFence, VerifiedMtlsPeer};
use crate::NodeLocalFabricStore;
use crate::{canonical_digest, FabricError, FabricErrorCode};

/// Serve one already mTLS-authenticated outbound connection until the peer
/// closes it. A connection owns no authority beyond the exact durable leases.
pub fn serve_control_plane_session<S, K>(
    socket: &mut WebSocket<S>,
    peer: &VerifiedMtlsPeer,
    control_plane: &ControlPlane<'_, K>,
    control_plane_generation: u64,
) -> Result<(), FabricError>
where
    S: Read + Write,
    K: ArtifactKeyBackend,
{
    let hello_frame = read_frame(socket)?;
    let hello = match &hello_frame.payload {
        FabricPayload::Hello(hello) => hello.clone(),
        _ => {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "first Fabric frame must be hello",
            ))
        }
    };
    if hello_frame.company_id != peer.company_id
        || hello_frame.node_id != peer.node_id
        || hello_frame.node_daemon_id != hello.node_daemon_id
        || hello_frame.node_daemon_generation != hello.node_daemon_generation
    {
        return Err(FabricError::none(
            FabricErrorCode::SourceMismatch,
            "pre-lease Hello frame disagrees with mTLS or NodeDaemon identity",
        ));
    }
    let now = now_unix_ms()?;
    let welcome =
        control_plane.connect_gateway_mtls(control_plane_generation, peer, &hello, now)?;
    let session = FabricSessionFence {
        company_id: welcome.company_id.clone(),
        node_id: welcome.node_id.clone(),
        gateway_generation: welcome.gateway_generation,
        node_daemon_id: welcome.node_daemon_id.clone(),
        node_daemon_generation: welcome.node_daemon_generation,
        control_plane_generation: welcome.control_plane_generation,
    };
    send_payload(
        socket,
        &session,
        &hello_frame.correlation_id,
        FabricPayload::Welcome(welcome),
        now,
    )?;

    loop {
        let frame = match read_frame(socket) {
            Ok(frame) => frame,
            Err(error) if error.code == FabricErrorCode::TargetOffline => return Ok(()),
            Err(error) => return Err(error),
        };
        session.validate_frame(&frame)?;
        let now = now_unix_ms()?;
        match frame.payload {
            FabricPayload::Heartbeat { .. } => {
                let current = control_plane.store().snapshot()?;
                let lease = current
                    .gateway_leases
                    .get(&session.node_id)
                    .ok_or_else(|| {
                        FabricError::none(
                            FabricErrorCode::NodeStaleGeneration,
                            "gateway heartbeat has no durable lease",
                        )
                    })?;
                control_plane.heartbeat_gateway(
                    control_plane_generation,
                    &session.node_id,
                    session.gateway_generation,
                    &session.node_daemon_id,
                    session.node_daemon_generation,
                    lease.revision,
                    now,
                )?;
                send_payload(
                    socket,
                    &session,
                    &frame.correlation_id,
                    FabricPayload::HeartbeatAck {
                        observed_at_unix_ms: now,
                    },
                    now,
                )?;
                if !send_pending(socket, control_plane, &session, now)? {
                    send_payload(
                        socket,
                        &session,
                        &frame.correlation_id,
                        FabricPayload::PendingBatchComplete {
                            observed_at_unix_ms: now,
                        },
                        now,
                    )?;
                }
            }
            FabricPayload::OperationSubmit(operation) => {
                let actor = authenticated_node_actor(peer, now);
                let (_, _, receipt, _) = control_plane.accept_operation(
                    control_plane_generation,
                    &session,
                    &actor,
                    *operation,
                    now,
                )?;
                send_payload(
                    socket,
                    &session,
                    &frame.correlation_id,
                    FabricPayload::Receipt(Box::new(receipt)),
                    now,
                )?;
            }
            FabricPayload::TargetPersisted(claim) => {
                let (_, receipt, _) = control_plane.record_target_persisted(
                    control_plane_generation,
                    &session.node_id,
                    session.gateway_generation,
                    &session.node_daemon_id,
                    session.node_daemon_generation,
                    &claim.operation_id,
                    &claim.request_digest,
                    claim.route_seq,
                    now,
                )?;
                send_payload(
                    socket,
                    &session,
                    &frame.correlation_id,
                    FabricPayload::Receipt(Box::new(receipt)),
                    now,
                )?;
            }
            FabricPayload::OperationResult(result) => {
                let (_, receipt, _) = control_plane.record_application_receipt(
                    control_plane_generation,
                    &session.node_id,
                    session.gateway_generation,
                    &session.node_daemon_id,
                    session.node_daemon_generation,
                    &result.operation_id,
                    &result.result_schema,
                    result.result,
                    result.effect,
                    now,
                )?;
                send_payload(
                    socket,
                    &session,
                    &frame.correlation_id,
                    FabricPayload::Receipt(Box::new(receipt)),
                    now,
                )?;
            }
            FabricPayload::ReconcileRequest { operation_ids } => {
                let receipts = control_plane.reconcile(
                    control_plane_generation,
                    &session.node_id,
                    session.gateway_generation,
                    &session.node_daemon_id,
                    session.node_daemon_generation,
                    &operation_ids,
                    now,
                )?;
                send_payload(
                    socket,
                    &session,
                    &frame.correlation_id,
                    FabricPayload::ReconcileResult { receipts },
                    now,
                )?;
            }
            FabricPayload::ArtifactCapabilityRequest(request) => {
                if request.purpose != ArtifactCapabilityPurpose::Download {
                    return Err(FabricError::none(
                        FabricErrorCode::CapabilityInvalid,
                        "Node gateway v1 may request only a self-bound download capability",
                    ));
                }
                let mut actor = authenticated_node_actor(peer, now);
                actor.role_bindings.insert("artifact_read".into());
                let capability = control_plane.issue_download_capability(
                    &actor,
                    control_plane_generation,
                    &request.artifact_id,
                    &session.node_id,
                    now,
                )?;
                send_payload(
                    socket,
                    &session,
                    &frame.correlation_id,
                    FabricPayload::ArtifactCapabilityResponse(capability),
                    now,
                )?;
            }
            FabricPayload::Hello(_)
            | FabricPayload::Welcome(_)
            | FabricPayload::RoutedOperation(_)
            | FabricPayload::Receipt(_)
            | FabricPayload::HeartbeatAck { .. }
            | FabricPayload::PendingBatchComplete { .. }
            | FabricPayload::ReconcileResult { .. }
            | FabricPayload::ArtifactCapabilityResponse(_)
            | FabricPayload::LeaseFence { .. }
            | FabricPayload::Drain { .. }
            | FabricPayload::ProtocolShutdown { .. } => {
                return Err(FabricError::none(
                    FabricErrorCode::ProtocolIncompatible,
                    "Node sent a server-authority Fabric frame",
                ))
            }
        }
    }
}

fn authenticated_node_actor(peer: &VerifiedMtlsPeer, now_unix_ms: u64) -> AuthenticatedActor {
    AuthenticatedActor {
        company_id: peer.company_id.clone(),
        actor_id: peer.node_id.clone(),
        actor_kind: ActorKind::Service,
        role_bindings: BTreeSet::from(["fabric_submit".into()]),
        session_id: format!("mtls:{}", peer.certificate_serial),
        issued_at_unix_ms: now_unix_ms,
        expires_at_unix_ms: now_unix_ms.saturating_add(30_000),
    }
}

fn send_pending<S, K>(
    socket: &mut WebSocket<S>,
    control_plane: &ControlPlane<'_, K>,
    session: &FabricSessionFence,
    now_unix_ms: u64,
) -> Result<bool, FabricError>
where
    S: Read + Write,
    K: ArtifactKeyBackend,
{
    let state = control_plane.store().snapshot()?;
    let mut attempts = state
        .attempts
        .values()
        .filter(|attempt| {
            attempt.target_node_id == session.node_id
                && attempt.target_gateway_generation == session.gateway_generation
                && attempt.control_plane_generation == session.control_plane_generation
                && matches!(
                    attempt.state,
                    RouteAttemptState::Queued | RouteAttemptState::Sent
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    attempts.sort_by_key(|attempt| attempt.route_seq);
    if let Some(attempt) = attempts.into_iter().next() {
        let operation = state
            .operations
            .get(&attempt.operation_id)
            .cloned()
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::StoreUnavailable,
                    "route attempt has no canonical operation",
                )
            })?;
        let correlation_id = operation.correlation_id.clone();
        send_payload(
            socket,
            session,
            &correlation_id,
            FabricPayload::RoutedOperation(Box::new(RoutedOperationDelivery {
                operation,
                attempt,
            })),
            now_unix_ms,
        )?;
        return Ok(true);
    }
    Ok(false)
}

pub fn send_payload<S: Read + Write>(
    socket: &mut WebSocket<S>,
    session: &FabricSessionFence,
    correlation_id: &str,
    payload: FabricPayload,
    now_unix_ms: u64,
) -> Result<(), FabricError> {
    let payload_kind = match &payload {
        FabricPayload::Hello(_) => "hello",
        FabricPayload::Welcome(_) => "welcome",
        FabricPayload::OperationSubmit(_) => "operation-submit",
        FabricPayload::RoutedOperation(_) => "routed-operation",
        FabricPayload::TargetPersisted(_) => "target-persisted",
        FabricPayload::OperationResult(_) => "operation-result",
        FabricPayload::Receipt(_) => "receipt",
        FabricPayload::Heartbeat { .. } => "heartbeat",
        FabricPayload::HeartbeatAck { .. } => "heartbeat-ack",
        FabricPayload::PendingBatchComplete { .. } => "pending-batch-complete",
        FabricPayload::ReconcileRequest { .. } => "reconcile-request",
        FabricPayload::ReconcileResult { .. } => "reconcile-result",
        FabricPayload::ArtifactCapabilityRequest(_) => "artifact-capability-request",
        FabricPayload::ArtifactCapabilityResponse(_) => "artifact-capability-response",
        FabricPayload::LeaseFence { .. } => "lease-fence",
        FabricPayload::Drain { .. } => "drain",
        FabricPayload::ProtocolShutdown { .. } => "protocol-shutdown",
    };
    let frame = FabricFrame::new(
        format!(
            "frame:{payload_kind}:{}",
            canonical_digest(&(correlation_id, now_unix_ms, payload_kind))?
        ),
        &session.company_id,
        &session.node_id,
        session.gateway_generation,
        &session.node_daemon_id,
        session.node_daemon_generation,
        session.control_plane_generation,
        now_unix_ms,
        correlation_id,
        payload,
    )?;
    write_frame(socket, &frame)
}

pub fn hello_frame(hello: NodeHello, now_unix_ms: u64) -> Result<FabricFrame, FabricError> {
    let company_id = hello.company_id.clone();
    let node_id = hello.node_id.clone();
    let node_daemon_id = hello.node_daemon_id.clone();
    FabricFrame::new(
        format!("frame:hello:{}", hello.instance_id),
        company_id,
        node_id,
        0,
        node_daemon_id,
        hello.node_daemon_generation,
        0,
        now_unix_ms,
        format!("connect:{}", hello.instance_id),
        FabricPayload::Hello(hello),
    )
}

pub fn now_unix_ms() -> Result<u64, FabricError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .map_err(|_| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "system clock is before UNIX epoch",
            )
        })
}

pub trait NodeApplication {
    fn apply(
        &mut self,
        operation: &RoutedOperation,
    ) -> Result<(String, serde_json::Value, EffectCertainty), FabricError>;
}

/// Live outbound gateway connection. The Node has no listener; all target
/// work arrives through this authenticated socket and is journaled before the
/// application callback is allowed to run.
pub struct NodeGatewayConnection {
    socket: NodeGatewaySocket,
    pub session: FabricSessionFence,
}

impl NodeGatewayConnection {
    pub fn connect(
        config: &NodeFabricConfig,
        tls: &NodeTlsIdentityFiles,
        hello: NodeHello,
    ) -> Result<Self, FabricError> {
        let socket = connect_outbound_mtls(config, tls)?;
        Self::establish(config, socket, hello)
    }

    pub fn connect_with_material(
        config: &NodeFabricConfig,
        tls: &NodeTlsIdentityMaterial,
        hello: NodeHello,
    ) -> Result<Self, FabricError> {
        let socket = connect_outbound_mtls_material(config, tls)?;
        Self::establish(config, socket, hello)
    }

    fn establish(
        config: &NodeFabricConfig,
        mut socket: NodeGatewaySocket,
        hello: NodeHello,
    ) -> Result<Self, FabricError> {
        if hello.company_id != config.company_id || hello.node_id != config.node_id {
            return Err(FabricError::none(
                FabricErrorCode::SourceMismatch,
                "NodeHello disagrees with the outbound gateway configuration",
            ));
        }
        let now = now_unix_ms()?;
        write_frame(&mut socket, &hello_frame(hello, now)?)?;
        let welcome_frame = read_frame(&mut socket)?;
        let welcome = match &welcome_frame.payload {
            FabricPayload::Welcome(welcome) => welcome.clone(),
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::ProtocolIncompatible,
                    "Control Plane did not answer Hello with Welcome",
                ))
            }
        };
        let session = FabricSessionFence {
            company_id: welcome.company_id,
            node_id: welcome.node_id,
            gateway_generation: welcome.gateway_generation,
            node_daemon_id: welcome.node_daemon_id,
            node_daemon_generation: welcome.node_daemon_generation,
            control_plane_generation: welcome.control_plane_generation,
        };
        session.validate_frame(&welcome_frame)?;
        Ok(Self { socket, session })
    }

    pub fn heartbeat(&mut self) -> Result<(), FabricError> {
        let now = now_unix_ms()?;
        send_payload(
            &mut self.socket,
            &self.session,
            &format!("heartbeat:{}", self.session.node_id),
            FabricPayload::Heartbeat {
                observed_at_unix_ms: now,
            },
            now,
        )?;
        let response = read_frame(&mut self.socket)?;
        self.session.validate_frame(&response)?;
        match response.payload {
            FabricPayload::HeartbeatAck { .. } => Ok(()),
            FabricPayload::LeaseFence { reason } => Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                reason,
            )),
            _ => Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "heartbeat response was not HeartbeatAck",
            )),
        }
    }

    pub fn submit_operation(
        &mut self,
        local_store: &NodeLocalFabricStore,
        authenticated_actor: &AuthenticatedActor,
        operation: RoutedOperation,
    ) -> Result<RouteReceipt, FabricError> {
        local_store.prepare_outbox(
            &self.session,
            authenticated_actor,
            &operation,
            now_unix_ms()?,
        )?;
        local_store.mark_outbox_submitted(
            &self.session,
            authenticated_actor,
            &operation,
            now_unix_ms()?,
        )?;
        let correlation_id = operation.correlation_id.clone();
        send_payload(
            &mut self.socket,
            &self.session,
            &correlation_id,
            FabricPayload::OperationSubmit(Box::new(operation)),
            now_unix_ms()?,
        )?;
        let receipt = self.read_receipt()?;
        local_store.mark_outbox_receipt(&receipt)?;
        Ok(receipt)
    }

    pub fn reconcile_operations(
        &mut self,
        local_store: &NodeLocalFabricStore,
        operation_ids: BTreeSet<String>,
    ) -> Result<Vec<RouteReceipt>, FabricError> {
        let now = now_unix_ms()?;
        send_payload(
            &mut self.socket,
            &self.session,
            &format!("reconcile:{}", self.session.node_id),
            FabricPayload::ReconcileRequest { operation_ids },
            now,
        )?;
        let frame = read_frame(&mut self.socket)?;
        self.session.validate_frame(&frame)?;
        let receipts = match frame.payload {
            FabricPayload::ReconcileResult { receipts } => receipts,
            FabricPayload::LeaseFence { reason } => {
                return Err(FabricError::none(
                    FabricErrorCode::NodeStaleGeneration,
                    reason,
                ))
            }
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::ProtocolIncompatible,
                    "reconciliation response was not ReconcileResult",
                ))
            }
        };
        for receipt in &receipts {
            if local_store
                .snapshot()?
                .outboxes
                .contains_key(&receipt.operation_id)
            {
                local_store.mark_outbox_receipt(receipt)?;
            }
        }
        Ok(receipts)
    }

    pub fn request_artifact_download(
        &mut self,
        artifact_id: &str,
    ) -> Result<ArtifactCapability, FabricError> {
        let now = now_unix_ms()?;
        send_payload(
            &mut self.socket,
            &self.session,
            &format!("artifact-capability:{artifact_id}"),
            FabricPayload::ArtifactCapabilityRequest(ArtifactCapabilityRequest {
                artifact_id: artifact_id.into(),
                purpose: ArtifactCapabilityPurpose::Download,
            }),
            now,
        )?;
        let frame = read_frame(&mut self.socket)?;
        self.session.validate_frame(&frame)?;
        match frame.payload {
            FabricPayload::ArtifactCapabilityResponse(capability) => Ok(capability),
            FabricPayload::LeaseFence { reason } => Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                reason,
            )),
            _ => Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "artifact capability response was not server-authoritative",
            )),
        }
    }

    /// Process one server-routed operation completely. This method is
    /// intentionally single-operation and deterministic; the long-running
    /// NodeDaemon loop decides scheduling and repeatedly calls it.
    pub fn apply_next<A: NodeApplication>(
        &mut self,
        local_store: &NodeLocalFabricStore,
        application: &mut A,
    ) -> Result<RouteReceipt, FabricError> {
        let frame = read_frame(&mut self.socket)?;
        self.session.validate_frame(&frame)?;
        let delivery = match frame.payload {
            FabricPayload::RoutedOperation(delivery) => *delivery,
            FabricPayload::Drain { reason } => {
                return Err(FabricError::none(FabricErrorCode::TargetNotPlaced, reason))
            }
            FabricPayload::LeaseFence { reason } => {
                return Err(FabricError::none(
                    FabricErrorCode::NodeStaleGeneration,
                    reason,
                ))
            }
            FabricPayload::PendingBatchComplete { .. } => {
                return Err(FabricError::none(
                    FabricErrorCode::TargetOffline,
                    "pending delivery batch is complete",
                ))
            }
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::ProtocolIncompatible,
                    "expected a server-routed operation",
                ))
            }
        };
        let (inbox, _) =
            local_store.persist_inbox(&self.session, &delivery.operation, &delivery.attempt)?;
        send_payload(
            &mut self.socket,
            &self.session,
            &delivery.operation.correlation_id,
            FabricPayload::TargetPersisted(TargetPersistedClaim {
                operation_id: delivery.operation.id.clone(),
                request_digest: inbox.request_digest.clone(),
                route_seq: delivery.attempt.route_seq,
            }),
            now_unix_ms()?,
        )?;
        let persisted_receipt = self.read_receipt()?;
        if persisted_receipt.kind != ReceiptKind::TargetPersisted {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "Control Plane did not acknowledge target persistence",
            ));
        }
        local_store.claim_inbox(&self.session, &delivery.operation.id)?;
        let (result_schema, result, effect) = match application.apply(&delivery.operation) {
            Ok(result) => result,
            Err(error) => {
                let effect = match error.effect {
                    EffectCertainty::Applied => EffectCertainty::Applied,
                    EffectCertainty::Unknown => EffectCertainty::Unknown,
                    EffectCertainty::None | EffectCertainty::NotApplied => {
                        EffectCertainty::NotApplied
                    }
                };
                (
                    "agentfirm.remote_fabric.application_error.v1".into(),
                    serde_json::to_value(&error).map_err(|encode_error| {
                        FabricError::unknown(
                            delivery.operation.id.clone(),
                            format!("application error could not be journaled: {encode_error}"),
                        )
                    })?,
                    effect,
                )
            }
        };
        local_store.record_application_result(
            &self.session,
            &delivery.operation.id,
            &result_schema,
            result.clone(),
            effect,
            now_unix_ms()?,
        )?;
        send_payload(
            &mut self.socket,
            &self.session,
            &delivery.operation.correlation_id,
            FabricPayload::OperationResult(TargetApplicationResult {
                operation_id: delivery.operation.id,
                result_schema,
                result,
                effect,
            }),
            now_unix_ms()?,
        )?;
        self.read_receipt()
    }

    fn read_receipt(&mut self) -> Result<RouteReceipt, FabricError> {
        let frame = read_frame(&mut self.socket)?;
        self.session.validate_frame(&frame)?;
        match frame.payload {
            FabricPayload::Receipt(receipt) => Ok(*receipt),
            FabricPayload::LeaseFence { reason } => Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                reason,
            )),
            _ => Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "expected a canonical route receipt",
            )),
        }
    }

    pub fn close(mut self) -> Result<(), FabricError> {
        self.socket
            .close(None)
            .map_err(|error| FabricError::none(FabricErrorCode::TargetOffline, error.to_string()))
    }

    pub fn set_read_timeout(
        &mut self,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), FabricError> {
        set_node_gateway_read_timeout(&mut self.socket, timeout)
    }
}

/// Closed application used by health probes and deterministic fabric
/// acceptance. It cannot mutate TeamWork or any Wave 4C business object.
#[derive(Default)]
pub struct ProbeApplication;

impl NodeApplication for ProbeApplication {
    fn apply(
        &mut self,
        operation: &RoutedOperation,
    ) -> Result<(String, serde_json::Value, EffectCertainty), FabricError> {
        match operation.closed_body()? {
            ClosedOperationBody::Probe(body) | ClosedOperationBody::ReconcileProbe(body) => Ok((
                "agentfirm.remote_fabric.probe_result.v1".into(),
                serde_json::json!({"echo": body.probe}),
                EffectCertainty::Applied,
            )),
            _ => Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "probe application accepts only closed non-business probe operations",
            )),
        }
    }
}
