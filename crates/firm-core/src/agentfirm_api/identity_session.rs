//! Canonical Member Execution Trust Kernel wire contracts.
//!
//! These closed types intentionally live behind one module while the Wave 4A
//! clean cutover removes the former runtime-heavy identity and embedded
//! delivery/gate records from the root module.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    AgentMember,
    External,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorRef {
    pub kind: ActorKind,
    pub id: String,
}

/// Stable organizational identity. Provider-native state never lives here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentIdentity {
    pub id: String,
    pub display_name: String,
    pub organization_status: AgentMemberOrganizationStatus,
    pub permission_ceiling: PermissionCeiling,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionCeiling {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMemberOrganizationStatus {
    Active,
    Paused,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMember {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub skill_refs: Vec<String>,
    #[serde(default)]
    pub provider_profile_ref: Option<String>,
    #[serde(default)]
    pub model_preference: Option<String>,
    pub workspace_policy: String,
    pub permission_ceiling: PermissionCeiling,
    pub organization_status: AgentMemberOrganizationStatus,
    pub version: u64,
    pub created_by: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSessionAvailability {
    Available,
    Stale,
    Missing,
    Incompatible,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionStatus {
    Cold,
    Idle,
    Active,
    Waiting,
    Interrupted,
    RecoveryRequired,
    Closed,
}

/// Whether this process generation currently owns a live provider runtime.
///
/// This is deliberately independent from [`AgentSessionStatus`]: a durable
/// session may remain open while its process-local runtime is detached.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeResidency {
    Detached,
    Attaching,
    Attached,
    Releasing,
    #[default]
    Unknown,
}

/// Observable activity of the current execution cycle, if one exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeActivity {
    Idle,
    Running,
    WaitingInput,
    Interrupting,
    #[default]
    Unknown,
}

/// The one party allowed to schedule the next top-level execution cycle.
/// NodeDaemon remains the Runtime Supervisor in every variant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberExecutionDriver {
    #[default]
    HostDriven,
    ProviderDriven,
    /// The user drives an already-open external interactive runtime which
    /// Harness neither spawned nor owns.
    UserDriven,
}

/// Exact current owner of the next-cycle authority. The broad
/// [`MemberExecutionDriver`] answers *which class* of driver is active; this
/// reference identifies the concrete generation which commands must fence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeDriverRef {
    NodeDaemon {
        node_daemon_id: String,
        node_daemon_generation: u64,
    },
    TeamSupervisor {
        team_run_id: String,
        team_supervisor_id: String,
        team_supervisor_generation: u64,
    },
    ProviderContinuation {
        provider: String,
        continuation_id: String,
        #[serde(default)]
        continuation_revision: Option<u64>,
        runtime_generation: u64,
    },
    #[default]
    Unknown,
}

/// A driver transfer fences both parties until its postconditions are proven.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverHandoffState {
    #[default]
    None,
    PreparingHostToProvider,
    PreparingProviderToHost,
    RecoveryRequired,
    Unknown,
}

/// Durable provider-native continuation phase. It does not say whether this
/// runtime generation is currently authorized to schedule another cycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeContinuationPhase {
    Inactive,
    Active,
    Paused,
    Blocked,
    Satisfied,
    #[default]
    Unknown,
}

/// Process-local continuation authorization. `Armed` is valid only for the
/// exact runtime and execution-driver generations carried by the variant.
/// Old records deserialize to `Disarmed`, so resume never silently inherits
/// provider-driven execution authority.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeContinuationActivation {
    Armed {
        runtime_generation: u64,
        driver_generation: u64,
    },
    #[default]
    Disarmed,
    Unknown,
}

/// Provider-native continuation budget projection. Providers may expose only a
/// subset; missing fields remain unknown rather than being synthesized.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeContinuationBudget {
    #[serde(default)]
    pub remaining_cycles: Option<u64>,
    #[serde(default)]
    pub remaining_tokens: Option<u64>,
    #[serde(default)]
    pub deadline: Option<String>,
    #[serde(default)]
    pub provider_budget_ref: Option<String>,
}

/// Durable provider-native continuation definition projection. This remains a
/// reference/snapshot of provider truth, not a CompanyOS Goal or Work object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeContinuationDefinition {
    #[serde(default)]
    pub continuation_ref: Option<String>,
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub phase: NativeContinuationPhase,
    #[serde(default)]
    pub budget: Option<NativeContinuationBudget>,
}

/// Bounded control projection of a provider-native continuation. Durable
/// definition and process-local activation are separate so a resume/fork can
/// preserve phase while defaulting activation to disarmed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeContinuationProjection {
    #[serde(default)]
    pub definition: NativeContinuationDefinition,
    #[serde(default)]
    pub activation: NativeContinuationActivation,
    #[serde(default)]
    pub observed_at: Option<String>,
}

/// Non-ledger runtime-control state attached to an AgentSession.
///
/// Its default intentionally preserves readable legacy records while failing
/// closed: live state is unknown, Host remains the only possible driver, no
/// driver generation is admitted, and continuation activation is disarmed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSessionControlState {
    #[serde(default)]
    pub runtime_residency: RuntimeResidency,
    #[serde(default)]
    pub activity: RuntimeActivity,
    #[serde(default)]
    pub execution_driver: MemberExecutionDriver,
    #[serde(default)]
    pub driver_generation: u64,
    #[serde(default)]
    pub driver_ref: RuntimeDriverRef,
    #[serde(default)]
    pub handoff_state: DriverHandoffState,
    #[serde(default)]
    pub continuation: NativeContinuationProjection,
    #[serde(default)]
    pub composition_fingerprint: Option<String>,
    #[serde(default)]
    pub capability_fingerprint: Option<String>,
    #[serde(default)]
    pub last_reconciled_at: Option<String>,
}

/// One machine-local provider session owned by an exact NodeDaemon generation.
/// Team membership is deliberately absent: a session can outlive or move
/// between collaboration overlays without changing its identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSession {
    pub id: String,
    #[serde(alias = "agent_identity_id")]
    pub agent_member_id: String,
    pub node_id: String,
    pub execution_space_id: String,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub provider_kind: String,
    pub provider_profile_ref: String,
    pub permission_envelope_ref: String,
    pub effective_permission_ceiling: PermissionCeiling,
    pub lifecycle: AgentSessionStatus,
    pub runtime_generation: u64,
    /// Orthogonal, bounded control facts. This does not mirror provider-native
    /// turns, tools, commands, transcript, or file history.
    #[serde(default)]
    pub control_state: AgentSessionControlState,
    #[serde(default)]
    pub native_session_ref: Option<NativeSessionRef>,
    #[serde(default)]
    pub current_turn_id: Option<String>,
    pub queued_input_count: u64,
    pub version: u64,
    pub opened_at: String,
    pub last_active_at: String,
    #[serde(default)]
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMembershipStatus {
    Invited,
    Active,
    Leaving,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMembershipRole {
    Host,
    Member,
    Observer,
}

/// Collaboration membership. It binds a stable identity to one Team, not a
/// provider process or native session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamMembership {
    pub id: String,
    pub team_id: String,
    #[serde(alias = "agent_identity_id")]
    pub agent_member_id: String,
    pub node_id: String,
    pub role: TeamMembershipRole,
    pub state: TeamMembershipStatus,
    pub membership_generation: u64,
    #[serde(default)]
    pub default_subscription_refs: Vec<String>,
    pub created_by: ActorRef,
    pub revision: u64,
    pub joined_at: String,
    #[serde(default)]
    pub left_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyAgentTeamStatus {
    Active,
    Closed,
    Archived,
}

/// Explicit legacy input used only by the one-way, same-ID Team migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyAgentTeamProjection {
    pub id: String,
    pub name: String,
    pub description: String,
    pub mission_id: String,
    pub host_agent_id: String,
    pub node_id: String,
    pub status: LegacyAgentTeamStatus,
    #[serde(default)]
    pub member_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Closed, reviewable migration bundle. Every legacy AgentIdentity id must map
/// to the same AgentMember id; ambiguity or inferred identity is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamMigrationBundle {
    pub source: LegacyAgentTeamProjection,
    pub target: crate::AgentTeam,
    pub memberships: Vec<TeamMembership>,
    pub identity_id_map: BTreeMap<String, String>,
    pub migration_id: String,
    pub source_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamPurgeRequest {
    pub tombstone_id: String,
    pub team_id: String,
    pub expected_team_revision: u64,
    pub approval_ref: String,
    pub export_manifest_ref: String,
    pub restore_window_closed_at: String,
    pub requested_by: ActorRef,
    pub requested_at: String,
}

/// Purge authorization evidence. DEV-35 records this tombstone but deliberately
/// does not bulk-delete Team-related Work, Messages, Memberships or sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamPurgeTombstone {
    pub id: String,
    pub team_id: String,
    pub team_revision: u64,
    pub approval_ref: String,
    pub export_manifest_ref: String,
    pub restore_window_closed_at: String,
    pub recorded_by: ActorRef,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionRef {
    pub provider: String,
    pub execution_mode: String,
    pub native_session_id: String,
    pub native_locator_kind: String,
    #[serde(default)]
    pub provider_version: Option<String>,
    pub adapter_contract_version: String,
    pub availability: NativeSessionAvailability,
    pub supports_resume: bool,
    #[serde(default)]
    pub last_verified_at: Option<String>,
    #[serde(default)]
    pub parent_native_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberCoordinationStatus {
    Active,
    Closed,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRuntimeStatus {
    Starting,
    Idle,
    Queued,
    Running,
    Waiting,
    Disconnected,
    Reviewing,
    Blocked,
    Completed,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberRun {
    pub id: String,
    pub agent_member_id: String,
    pub team_run_id: String,
    pub role_snapshot: String,
    #[serde(default)]
    pub provider_profile_snapshot: Option<String>,
    #[serde(default)]
    pub requested_controls: serde_json::Value,
    #[serde(default)]
    pub effective_controls: serde_json::Value,
    pub coordination_status: MemberCoordinationStatus,
    pub runtime_status: MemberRuntimeStatus,
    pub runtime_generation: u64,
    #[serde(default)]
    pub workspace_binding_id: Option<String>,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
    pub version: u64,
    pub started_at: String,
    #[serde(default)]
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}
