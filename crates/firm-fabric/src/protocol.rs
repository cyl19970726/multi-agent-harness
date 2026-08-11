use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::{canonical_digest, FABRIC_PROTOCOL_VERSION, FABRIC_SCHEMA_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectCertainty {
    None,
    Unknown,
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FabricErrorCode {
    EnrollmentInvalid,
    EnrollmentExpired,
    EnrollmentConsumed,
    EnrollmentRevoked,
    WrongCompany,
    NodeRevoked,
    NodeStaleGeneration,
    ControlPlaneStaleGeneration,
    LeaseConflict,
    ProtocolIncompatible,
    SchemaIncompatible,
    FeatureIncompatible,
    UnauthorizedActor,
    SourceMismatch,
    TargetOffline,
    TargetNotPlaced,
    ExpectedRevisionConflict,
    IdempotencyConflict,
    InvalidPayload,
    ArtifactInvalid,
    ArtifactTampered,
    CapabilityInvalid,
    CapabilityExpired,
    CapabilityConsumed,
    OperationExpired,
    OperationUnknown,
    QueueCapacity,
    RateLimited,
    StoreUnavailable,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct FabricError {
    pub code: FabricErrorCode,
    pub message: String,
    pub retryable: bool,
    pub effect: EffectCertainty,
    pub operation_id: Option<String>,
    pub expected_revision: Option<u64>,
    pub actual_revision: Option<u64>,
    pub retry_after_ms: Option<u64>,
    pub details: BTreeMap<String, String>,
}

impl FabricError {
    pub fn none(code: FabricErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            effect: EffectCertainty::None,
            operation_id: None,
            expected_revision: None,
            actual_revision: None,
            retry_after_ms: None,
            details: BTreeMap::new(),
        }
    }

    pub fn unknown(operation_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: FabricErrorCode::RecoveryRequired,
            message: message.into(),
            retryable: false,
            effect: EffectCertainty::Unknown,
            operation_id: Some(operation_id.into()),
            expected_revision: None,
            actual_revision: None,
            retry_after_ms: None,
            details: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    AgentMember,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedActor {
    pub company_id: String,
    pub actor_id: String,
    pub actor_kind: ActorKind,
    pub role_bindings: BTreeSet<String>,
    pub session_id: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

impl AuthenticatedActor {
    pub fn require_company_and_role(
        &self,
        company_id: &str,
        role: &str,
        now_unix_ms: u64,
    ) -> Result<(), FabricError> {
        if self.company_id != company_id {
            return Err(FabricError::none(
                FabricErrorCode::WrongCompany,
                "authenticated actor belongs to another Company",
            ));
        }
        if self.expires_at_unix_ms <= now_unix_ms {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                "authenticated actor session expired",
            ));
        }
        if !self.role_bindings.contains(role) {
            return Err(FabricError::none(
                FabricErrorCode::UnauthorizedActor,
                format!("authenticated actor lacks required role {role}"),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeAdministrativeStatus {
    Active,
    Draining,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeConnectionStatus {
    Online,
    Offline,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyNode {
    pub id: String,
    pub company_id: String,
    pub display_name: String,
    pub public_key_fingerprint: String,
    pub certificate_serial: String,
    pub administrative_status: NodeAdministrativeStatus,
    pub node_revision: u64,
    pub enrolled_at_unix_ms: u64,
    pub last_seen_at_unix_ms: Option<u64>,
    pub revoked_at_unix_ms: Option<u64>,
    pub revoke_reason: Option<String>,
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub schema_bundle_digest: String,
    pub schema_version: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl CompanyNode {
    pub fn connection_status(
        &self,
        lease: Option<&NodeGatewayLease>,
        now_unix_ms: u64,
    ) -> NodeConnectionStatus {
        match lease {
            Some(lease) if lease.expires_at_unix_ms > now_unix_ms => NodeConnectionStatus::Online,
            Some(_) => NodeConnectionStatus::Stale,
            None => NodeConnectionStatus::Offline,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentStatus {
    Pending,
    Consumed,
    Expired,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeEnrollment {
    pub id: String,
    pub company_id: String,
    pub token_digest: String,
    pub requested_name: String,
    pub allowed_capabilities: BTreeSet<String>,
    pub created_by: String,
    pub expires_at_unix_ms: u64,
    pub consumed_at_unix_ms: Option<u64>,
    pub consumed_by_node_id: Option<String>,
    pub status: EnrollmentStatus,
    pub schema_version: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyControlPlaneLease {
    pub company_id: String,
    pub lease_id: String,
    pub instance_id: String,
    pub control_plane_generation: u64,
    pub revision: u64,
    pub acquired_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub last_heartbeat_at_unix_ms: u64,
    pub schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeGatewayLease {
    pub company_id: String,
    pub node_id: String,
    pub lease_id: String,
    pub gateway_generation: u64,
    pub instance_id: String,
    pub revision: u64,
    pub control_plane_generation: u64,
    pub acquired_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub last_heartbeat_at_unix_ms: u64,
    pub protocol_version: u32,
    pub build_sha: String,
    pub schema_bundle_digest: String,
    pub schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeCertificate {
    pub serial: String,
    pub company_id: String,
    pub node_id: String,
    pub public_key_fingerprint: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub revoked_at_unix_ms: Option<u64>,
    pub proof_of_possession_digest: String,
    pub schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentProof {
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
    pub challenge: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeHello {
    pub company_id: String,
    pub node_id: String,
    pub instance_id: String,
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub schema_bundle_digest: String,
    pub features: BTreeSet<String>,
    pub build_sha: String,
    pub last_persisted_route_seq: u64,
    pub unresolved_operation_ids: BTreeSet<String>,
    pub certificate_serial: String,
    pub public_key_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeHelloProof {
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
    pub challenge: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeWelcome {
    pub company_id: String,
    pub node_id: String,
    pub accepted_protocol_version: u32,
    pub lease_id: String,
    pub gateway_generation: u64,
    pub control_plane_generation: u64,
    pub next_route_seq: u64,
    pub required_reconcile_ids: BTreeSet<String>,
    pub schema_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPriority {
    Normal,
    Control,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedOperation {
    pub id: String,
    pub company_id: String,
    pub kind: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub source_gateway_generation: u64,
    pub control_plane_generation: u64,
    pub source_execution_space_id: Option<String>,
    pub target_execution_space_id: Option<String>,
    pub actor: AuthenticatedActor,
    pub actor_runtime_generation: Option<u64>,
    pub authorization_context: BTreeMap<String, String>,
    pub idempotency_key: String,
    pub ordering_key: String,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub expected_target_revision: Option<u64>,
    pub body_schema: String,
    pub body: Value,
    pub body_digest: String,
    pub priority: OperationPriority,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub protocol_version: u32,
    pub schema_version: String,
}

impl RoutedOperation {
    pub fn validate_digest(&self) -> Result<(), FabricError> {
        let actual = canonical_digest(&self.body)?;
        if actual != self.body_digest {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "RoutedOperation body digest mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteAttemptState {
    Queued,
    Sent,
    TargetPersisted,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteAttempt {
    pub id: String,
    pub company_id: String,
    pub operation_id: String,
    pub attempt_no: u32,
    pub target_node_id: String,
    pub target_gateway_generation: u64,
    pub control_plane_generation: u64,
    pub route_seq: u64,
    pub state: RouteAttemptState,
    pub error_code: Option<FabricErrorCode>,
    pub effect: EffectCertainty,
    pub started_at_unix_ms: u64,
    pub ended_at_unix_ms: Option<u64>,
    pub schema_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptKind {
    ControlPlaneAccepted,
    TargetPersisted,
    OperationApplied,
    OperationRejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteReceipt {
    pub id: String,
    pub company_id: String,
    pub operation_id: String,
    pub target_node_id: String,
    pub target_gateway_generation: u64,
    pub control_plane_generation: u64,
    pub route_seq: u64,
    pub kind: ReceiptKind,
    pub result_schema: Option<String>,
    pub result: Option<Value>,
    pub result_digest: Option<String>,
    pub error: Option<FabricError>,
    pub created_at_unix_ms: u64,
    pub schema_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOutboxState {
    Pending,
    Submitted,
    Accepted,
    Terminal,
    ReconcileRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRemoteOutbox {
    pub company_id: String,
    pub node_id: String,
    pub operation_id: String,
    pub request_digest: String,
    pub local_state: LocalOutboxState,
    pub gateway_generation: u64,
    pub attempt_count: u32,
    pub last_attempt_at_unix_ms: Option<u64>,
    pub terminal_receipt_ref: Option<String>,
    pub schema_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalInboxState {
    Persisted,
    Claimed,
    Applied,
    Rejected,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRemoteInbox {
    pub company_id: String,
    pub node_id: String,
    pub operation_id: String,
    pub route_seq: u64,
    pub request_digest: String,
    pub state: LocalInboxState,
    pub gateway_generation: u64,
    pub attempt_count: u32,
    pub claim_generation: Option<u64>,
    pub result_digest: Option<String>,
    pub schema_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactClassification {
    CompanyInternal,
    Sensitive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteArtifactManifest {
    pub id: String,
    pub company_id: String,
    pub source_node_id: String,
    pub source_team_id: Option<String>,
    pub source_work_id: Option<String>,
    pub operation_id: Option<String>,
    pub media_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub classification: ArtifactClassification,
    pub initiator: String,
    pub authorized_readers: BTreeSet<String>,
    pub created_by: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub completed_at_unix_ms: Option<u64>,
    pub deleted_at_unix_ms: Option<u64>,
    pub revision: u64,
    pub schema_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactCapabilityPurpose {
    Upload,
    Download,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCapability {
    pub token: String,
    pub company_id: String,
    pub node_id: String,
    pub artifact_id: String,
    pub artifact_digest: String,
    pub purpose: ArtifactCapabilityPurpose,
    pub issued_to: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub one_use: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame_kind", content = "payload", rename_all = "snake_case")]
pub enum FabricPayload {
    Hello(NodeHello),
    Welcome(NodeWelcome),
    RoutedOperation(Box<RoutedOperation>),
    Receipt(Box<RouteReceipt>),
    Heartbeat { observed_at_unix_ms: u64 },
    HeartbeatAck { observed_at_unix_ms: u64 },
    ReconcileRequest { operation_ids: BTreeSet<String> },
    ReconcileResult { receipts: Vec<RouteReceipt> },
    LeaseFence { reason: String },
    Drain { reason: String },
    ProtocolShutdown { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FabricFrame {
    pub frame_id: String,
    pub protocol_version: u32,
    pub schema_version: String,
    pub company_id: String,
    pub node_id: String,
    pub gateway_generation: u64,
    pub control_plane_generation: u64,
    pub sent_at_unix_ms: u64,
    pub correlation_id: String,
    pub payload: FabricPayload,
    pub payload_digest: String,
}

impl FabricFrame {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        frame_id: impl Into<String>,
        company_id: impl Into<String>,
        node_id: impl Into<String>,
        gateway_generation: u64,
        control_plane_generation: u64,
        sent_at_unix_ms: u64,
        correlation_id: impl Into<String>,
        payload: FabricPayload,
    ) -> Result<Self, FabricError> {
        let payload_digest = canonical_digest(&payload)?;
        Ok(Self {
            frame_id: frame_id.into(),
            protocol_version: FABRIC_PROTOCOL_VERSION,
            schema_version: FABRIC_SCHEMA_VERSION.into(),
            company_id: company_id.into(),
            node_id: node_id.into(),
            gateway_generation,
            control_plane_generation,
            sent_at_unix_ms,
            correlation_id: correlation_id.into(),
            payload,
            payload_digest,
        })
    }

    pub fn validate(&self) -> Result<(), FabricError> {
        if self.protocol_version != FABRIC_PROTOCOL_VERSION {
            return Err(FabricError::none(
                FabricErrorCode::ProtocolIncompatible,
                "fabric protocol major mismatch",
            ));
        }
        if self.schema_version != FABRIC_SCHEMA_VERSION {
            return Err(FabricError::none(
                FabricErrorCode::SchemaIncompatible,
                "fabric schema version mismatch",
            ));
        }
        if canonical_digest(&self.payload)? != self.payload_digest {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "FabricFrame payload digest mismatch",
            ));
        }
        Ok(())
    }
}
