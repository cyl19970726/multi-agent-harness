use crate::protocol::{FabricError, FabricErrorCode, FabricFrame};

pub const MAX_FABRIC_FRAME_BYTES: usize = 256 * 1024;
pub const FABRIC_WEBSOCKET_SUBPROTOCOL: &str = "agentfirm.node.v1";
pub const FABRIC_GATEWAY_PATH: &str = "/v1/node-gateway/connect";
pub const GATEWAY_HEARTBEAT_INTERVAL_MS: u64 = 10_000;
pub const GATEWAY_LEASE_DURATION_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMtlsPeer {
    pub company_id: String,
    pub node_id: String,
    pub certificate_serial: String,
    pub public_key_fingerprint: String,
    pub tls_version: String,
    pub websocket_subprotocol: String,
}

impl VerifiedMtlsPeer {
    pub fn validate_node_hello(
        &self,
        hello: &crate::protocol::NodeHello,
    ) -> Result<(), FabricError> {
        if self.tls_version != "TLS1.3"
            || self.websocket_subprotocol != FABRIC_WEBSOCKET_SUBPROTOCOL
        {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "Remote Fabric requires verified mutual TLS 1.3 and agentfirm.node.v1",
            ));
        }
        if self.company_id != hello.company_id
            || self.node_id != hello.node_id
            || self.certificate_serial != hello.certificate_serial
            || self.public_key_fingerprint != hello.public_key_fingerprint
        {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "NodeHello identity does not match the verified mTLS peer certificate",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFabricConfig {
    pub company_id: String,
    pub node_id: String,
    pub control_plane_url: String,
    pub reconnect_floor_ms: u64,
    pub reconnect_ceiling_ms: u64,
}

impl NodeFabricConfig {
    pub fn validate(&self) -> Result<(), FabricError> {
        if self.company_id.trim().is_empty() || self.node_id.trim().is_empty() {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "Remote Fabric config requires exact Company and Node identity",
            ));
        }
        let endpoint = self
            .control_plane_url
            .strip_prefix("wss://")
            .ok_or_else(|| {
                FabricError::none(
                    FabricErrorCode::ProtocolIncompatible,
                    "Node Fabric permits outbound wss:// Control Plane connections only",
                )
            })?;
        if endpoint.is_empty()
            || endpoint.starts_with('/')
            || endpoint.contains('@')
            || endpoint.contains('#')
            || endpoint.contains('?')
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "Control Plane endpoint must be an authority/path without credentials, query, or fragment",
            ));
        }
        if !endpoint.ends_with(FABRIC_GATEWAY_PATH) {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "Control Plane endpoint must use the frozen /v1/node-gateway/connect path",
            ));
        }
        if self.reconnect_floor_ms == 0
            || self.reconnect_ceiling_ms < self.reconnect_floor_ms
            || self.reconnect_ceiling_ms > GATEWAY_LEASE_DURATION_MS
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "reconnect backoff must be bounded within the gateway lease duration",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricSessionFence {
    pub company_id: String,
    pub node_id: String,
    pub gateway_generation: u64,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub control_plane_generation: u64,
}

impl FabricSessionFence {
    pub fn validate_frame(&self, frame: &FabricFrame) -> Result<(), FabricError> {
        frame.validate()?;
        if frame.company_id != self.company_id
            || frame.node_id != self.node_id
            || frame.gateway_generation != self.gateway_generation
            || frame.node_daemon_id != self.node_daemon_id
            || frame.node_daemon_generation != self.node_daemon_generation
            || frame.control_plane_generation != self.control_plane_generation
        {
            return Err(FabricError::none(
                FabricErrorCode::NodeStaleGeneration,
                "FabricFrame does not match the authenticated session fence",
            ));
        }
        Ok(())
    }
}

pub fn encode_frame(frame: &FabricFrame) -> Result<Vec<u8>, FabricError> {
    frame.validate()?;
    let encoded = serde_json::to_vec(frame).map_err(|error| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("FabricFrame encoding failed: {error}"),
        )
    })?;
    if encoded.len() > MAX_FABRIC_FRAME_BYTES {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "FabricFrame exceeds the 256 KiB wire limit; use an artifact capability",
        ));
    }
    Ok(encoded)
}

pub fn decode_frame(bytes: &[u8]) -> Result<FabricFrame, FabricError> {
    if bytes.is_empty() || bytes.len() > MAX_FABRIC_FRAME_BYTES {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "FabricFrame is empty or exceeds the 256 KiB wire limit",
        ));
    }
    let frame: FabricFrame = serde_json::from_slice(bytes).map_err(|error| {
        FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("FabricFrame is not valid closed-contract JSON: {error}"),
        )
    })?;
    frame.validate()?;
    Ok(frame)
}
