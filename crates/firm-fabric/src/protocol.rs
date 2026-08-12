use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    canonical_digest, FABRIC_CANONICALIZATION_VERSION, FABRIC_PROTOCOL_VERSION,
    FABRIC_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectCertainty {
    None,
    NotApplied,
    Unknown,
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationSourceAuthority {
    Node,
    ControlPlane,
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
    pub allowed_capabilities: BTreeSet<String>,
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
        current_control_plane_generation: u64,
        now_unix_ms: u64,
    ) -> NodeConnectionStatus {
        match lease {
            Some(lease)
                if lease.expires_at_unix_ms > now_unix_ms
                    && lease.control_plane_generation == current_control_plane_generation =>
            {
                NodeConnectionStatus::Online
            }
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
    pub revision: u64,
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
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
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
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
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
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
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

pub const PROBE_OPERATION_KIND: &str = "fabric.probe.v1";
pub const RECONCILE_PROBE_OPERATION_KIND: &str = "fabric.reconcile_probe.v1";
pub const RUNTIME_COMMAND_REFERENCE_KIND: &str = "runtime_command.reference.v1";
pub const MESSAGE_REFERENCE_KIND: &str = "message.reference.v1";
pub const DELIVERY_INTENT_REFERENCE_KIND: &str = "delivery_intent.reference.v1";
pub const ARTIFACT_REFERENCE_KIND: &str = "artifact.reference.v1";

pub const PROBE_BODY_SCHEMA: &str = "agentfirm.remote_fabric.probe.v1";
pub const RECONCILE_PROBE_BODY_SCHEMA: &str = "agentfirm.remote_fabric.reconcile_probe.v1";
pub const RUNTIME_COMMAND_REFERENCE_SCHEMA: &str =
    "agentfirm.remote_fabric.runtime_command_reference.v1";
pub const MESSAGE_REFERENCE_SCHEMA: &str = "agentfirm.remote_fabric.message_reference.v1";
pub const DELIVERY_INTENT_REFERENCE_SCHEMA: &str =
    "agentfirm.remote_fabric.delivery_intent_reference.v1";
pub const ARTIFACT_REFERENCE_SCHEMA: &str = "agentfirm.remote_fabric.artifact_reference.v1";

/// Closed transport probe. Probe operations never acquire application
/// authority and cannot carry arbitrary fields that a future reader might
/// mistake for authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FabricProbeBody {
    pub probe: String,
}

/// Immutable reference to the Wave 4C RuntimeCommand envelope. The target
/// application must independently resolve and verify `command_fingerprint`
/// before asking the exact NodeDaemon generation to execute it. Fabric never
/// interprets or mutates the command payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommandReference {
    pub runtime_command_id: String,
    pub command_fingerprint: String,
    pub target_execution_space_id: String,
    pub target_node_daemon_id: String,
    pub target_node_daemon_generation: u64,
    /// Exact Wave4C command authority transported to the target. Identity and
    /// fingerprint alone are not executable across independent Stores.
    pub canonical_command_envelope: Value,
}

/// Wave 4C Message authority remains on the source NodeDaemon. Cross-node
/// routing carries the immutable envelope itself or an authenticated,
/// content-addressed reference; identity + digest alone is never routable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageReference {
    pub message_id: String,
    pub body_digest: String,
    pub canonical_message_envelope: Option<Value>,
    pub message_object_ref: Option<MessageObjectReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageObjectReference {
    pub artifact_id: String,
    pub object_digest: String,
    pub read_capability: ArtifactCapability,
}

/// Per-recipient routing intent. The target NodeDaemon remains the only
/// authority allowed to create/claim/settle the canonical MessageDelivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryIntentReference {
    pub delivery_id: String,
    pub message_id: String,
    pub message_body_digest: String,
    pub recipient_identity_id: String,
    pub target_execution_space_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReference {
    pub artifact_id: String,
    pub artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosedOperationBody {
    Probe(FabricProbeBody),
    ReconcileProbe(FabricProbeBody),
    RuntimeCommand(RuntimeCommandReference),
    Message(MessageReference),
    DeliveryIntent(DeliveryIntentReference),
    Artifact(ArtifactReference),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedOperation {
    pub id: String,
    pub company_id: String,
    pub kind: String,
    pub source_authority: OperationSourceAuthority,
    pub source_node_id: Option<String>,
    pub target_node_id: String,
    pub source_gateway_generation: Option<u64>,
    pub source_node_daemon_id: Option<String>,
    pub source_node_daemon_generation: Option<u64>,
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
    pub canonicalization_version: String,
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

    /// Parse the operation through the frozen kind/schema registry. This is
    /// intentionally stronger than validating the outer JSON schema: it keeps
    /// arbitrary browser/Node JSON from becoming a future application command.
    pub fn closed_body(&self) -> Result<ClosedOperationBody, FabricError> {
        fn decode<T: serde::de::DeserializeOwned>(
            value: &Value,
            kind: &str,
        ) -> Result<T, FabricError> {
            serde_json::from_value(value.clone()).map_err(|error| {
                FabricError::none(
                    FabricErrorCode::InvalidPayload,
                    format!("{kind} body does not match its closed schema: {error}"),
                )
            })
        }

        let body = match (self.kind.as_str(), self.body_schema.as_str()) {
            (PROBE_OPERATION_KIND, PROBE_BODY_SCHEMA) => {
                ClosedOperationBody::Probe(decode(&self.body, &self.kind)?)
            }
            (RECONCILE_PROBE_OPERATION_KIND, RECONCILE_PROBE_BODY_SCHEMA) => {
                ClosedOperationBody::ReconcileProbe(decode(&self.body, &self.kind)?)
            }
            (RUNTIME_COMMAND_REFERENCE_KIND, RUNTIME_COMMAND_REFERENCE_SCHEMA) => {
                ClosedOperationBody::RuntimeCommand(decode(&self.body, &self.kind)?)
            }
            (MESSAGE_REFERENCE_KIND, MESSAGE_REFERENCE_SCHEMA) => {
                ClosedOperationBody::Message(decode(&self.body, &self.kind)?)
            }
            (DELIVERY_INTENT_REFERENCE_KIND, DELIVERY_INTENT_REFERENCE_SCHEMA) => {
                ClosedOperationBody::DeliveryIntent(decode(&self.body, &self.kind)?)
            }
            (ARTIFACT_REFERENCE_KIND, ARTIFACT_REFERENCE_SCHEMA) => {
                ClosedOperationBody::Artifact(decode(&self.body, &self.kind)?)
            }
            _ => {
                return Err(FabricError::none(
                    FabricErrorCode::SchemaIncompatible,
                    "operation kind/body_schema pair is not in the frozen registry",
                ))
            }
        };
        validate_closed_body(self, &body)?;
        Ok(body)
    }
}

fn validate_closed_body(
    operation: &RoutedOperation,
    body: &ClosedOperationBody,
) -> Result<(), FabricError> {
    let non_empty = |value: &str| !value.trim().is_empty();
    let digest =
        |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    let fingerprint = |value: &str| {
        value.strip_prefix("sha256:").is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    };
    let valid = match body {
        ClosedOperationBody::Probe(body) | ClosedOperationBody::ReconcileProbe(body) => {
            non_empty(&body.probe)
        }
        ClosedOperationBody::RuntimeCommand(body) => {
            non_empty(&body.runtime_command_id)
                && fingerprint(&body.command_fingerprint)
                && non_empty(&body.target_execution_space_id)
                && operation.target_execution_space_id.as_deref()
                    == Some(body.target_execution_space_id.as_str())
                && non_empty(&body.target_node_daemon_id)
                && body.target_node_daemon_generation > 0
                && body.canonical_command_envelope.is_object()
        }
        ClosedOperationBody::Message(body) => {
            let embedded = body
                .canonical_message_envelope
                .as_ref()
                .is_some_and(|envelope| {
                    let body_value = envelope.get("body").and_then(Value::as_str);
                    let computed_body_digest = body_value
                        .map(|message_body| format!("sha256:{}", crate::sha256_hex(message_body)));
                    envelope.get("id").and_then(Value::as_str) == Some(body.message_id.as_str())
                        && envelope.get("body_digest").and_then(Value::as_str)
                            == Some(body.body_digest.as_str())
                        && computed_body_digest.as_deref() == Some(body.body_digest.as_str())
                });
            let referenced = body.message_object_ref.as_ref().is_some_and(|reference| {
                non_empty(&reference.artifact_id)
                    && digest(&reference.object_digest)
                    && reference.read_capability.artifact_id == reference.artifact_id
                    && reference.read_capability.artifact_digest == reference.object_digest
                    && reference.read_capability.company_id == operation.company_id
                    && reference.read_capability.node_id == operation.target_node_id
                    && reference.read_capability.purpose == ArtifactCapabilityPurpose::Download
                    && reference.read_capability.expires_at_unix_ms > operation.created_at_unix_ms
            });
            non_empty(&body.message_id) && fingerprint(&body.body_digest) && (embedded ^ referenced)
        }
        ClosedOperationBody::DeliveryIntent(body) => {
            non_empty(&body.delivery_id)
                && non_empty(&body.message_id)
                && fingerprint(&body.message_body_digest)
                && non_empty(&body.recipient_identity_id)
                && operation.target_execution_space_id.as_deref()
                    == Some(body.target_execution_space_id.as_str())
        }
        ClosedOperationBody::Artifact(body) => {
            non_empty(&body.artifact_id) && digest(&body.artifact_digest)
        }
    };
    if !valid {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            "closed operation body is empty, malformed, or disagrees with the routed scope",
        ));
    }
    Ok(())
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
    pub ordering_seq: u64,
    pub state: RouteAttemptState,
    pub error_code: Option<FabricErrorCode>,
    pub effect: EffectCertainty,
    pub started_at_unix_ms: u64,
    pub ended_at_unix_ms: Option<u64>,
    pub schema_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FabricRateWindow {
    pub company_id: String,
    pub source_authority_key: String,
    pub actor_id: String,
    pub window_started_at_unix_ms: u64,
    pub accepted_count: u32,
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
    /// Application effect asserted by the generation-fenced target result.
    /// Transport-only receipts leave this unset.
    pub application_effect: Option<EffectCertainty>,
    pub result_schema: Option<String>,
    pub result: Option<Value>,
    pub result_digest: Option<String>,
    pub error: Option<FabricError>,
    pub created_at_unix_ms: u64,
    pub schema_version: String,
}

/// Control Plane delivery to an authenticated target gateway. The attempt is
/// transport authority owned by FabricStore; a Node may not invent route_seq
/// or the generation it is acknowledging.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutedOperationDelivery {
    pub operation: RoutedOperation,
    pub attempt: RouteAttempt,
}

/// Target claim emitted only after the exact operation bytes and attempt have
/// been durably persisted in the Node-local inbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPersistedClaim {
    pub operation_id: String,
    pub request_digest: String,
    pub route_seq: u64,
}

/// Generation-fenced target application result. RouteAttempt never carries
/// this authority and the Control Plane constructs the canonical receipt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetApplicationResult {
    pub operation_id: String,
    pub result_schema: String,
    pub result: Value,
    pub effect: EffectCertainty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOutboxState {
    Pending,
    QueuedForControlPlane,
    Submitted,
    Accepted,
    Terminal,
    ReconcileRequired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRemoteOutbox {
    pub company_id: String,
    pub node_id: String,
    pub operation_id: String,
    pub request_digest: String,
    pub local_state: LocalOutboxState,
    pub gateway_generation: u64,
    pub control_plane_generation: u64,
    pub attempt_count: u32,
    pub last_attempt_at_unix_ms: Option<u64>,
    pub terminal_receipt_ref: Option<String>,
    /// Durable pre-acceptance transport envelope. FabricStore becomes the sole
    /// route truth after Control Plane acceptance; this copy exists only so a
    /// live source gateway can safely retry its own queued submission.
    #[serde(default)]
    pub operation: Option<RoutedOperation>,
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
    pub control_plane_generation: u64,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCapabilityRequest {
    pub artifact_id: String,
    pub purpose: ArtifactCapabilityPurpose,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame_kind", content = "payload", rename_all = "snake_case")]
pub enum FabricPayload {
    Hello(NodeHello),
    Welcome(NodeWelcome),
    OperationSubmit(Box<RoutedOperation>),
    RoutedOperation(Box<RoutedOperationDelivery>),
    TargetPersisted(TargetPersistedClaim),
    OperationResult(TargetApplicationResult),
    Receipt(Box<RouteReceipt>),
    Heartbeat { observed_at_unix_ms: u64 },
    HeartbeatAck { observed_at_unix_ms: u64 },
    PendingBatchComplete { observed_at_unix_ms: u64 },
    ReconcileRequest { operation_ids: BTreeSet<String> },
    ReconcileResult { receipts: Vec<RouteReceipt> },
    ArtifactCapabilityRequest(ArtifactCapabilityRequest),
    ArtifactCapabilityResponse(ArtifactCapability),
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
    pub canonicalization_version: String,
    pub company_id: String,
    pub node_id: String,
    pub gateway_generation: u64,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
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
        node_daemon_id: impl Into<String>,
        node_daemon_generation: u64,
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
            canonicalization_version: FABRIC_CANONICALIZATION_VERSION.into(),
            company_id: company_id.into(),
            node_id: node_id.into(),
            gateway_generation,
            node_daemon_id: node_daemon_id.into(),
            node_daemon_generation,
            control_plane_generation,
            sent_at_unix_ms,
            correlation_id: correlation_id.into(),
            payload,
            payload_digest,
        })
    }

    pub fn validate(&self) -> Result<(), FabricError> {
        let pre_lease_hello = matches!(self.payload, FabricPayload::Hello(_));
        if self.frame_id.trim().is_empty()
            || self.company_id.trim().is_empty()
            || self.node_id.trim().is_empty()
            || self.correlation_id.trim().is_empty()
            || self.node_daemon_id.trim().is_empty()
            || self.node_daemon_generation == 0
            || (!pre_lease_hello
                && (self.gateway_generation == 0 || self.control_plane_generation == 0))
            || (pre_lease_hello
                && (self.gateway_generation != 0 || self.control_plane_generation != 0))
        {
            return Err(FabricError::none(
                FabricErrorCode::InvalidPayload,
                "FabricFrame requires non-empty identity and non-zero authority generations",
            ));
        }
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
        if self.canonicalization_version != FABRIC_CANONICALIZATION_VERSION {
            return Err(FabricError::none(
                FabricErrorCode::SchemaIncompatible,
                "Fabric canonicalization version mismatch",
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
