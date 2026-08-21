use super::*;

// a single host surface (codex-app / kimi-cli / claude-cli). `ProviderRuntimeProjection`s are
// the per-member session rows inside it; `TeamMessageProjection`s the routed mail;
// `MemberAction`s the fine-grained action journal; `DelegationRun`s the
// provider-native / harness-worker / dynamic-workflow child runs; and
// `TeamRunEvent` the folded per-run event log. All journal to their own
// append-only JSONL with latest-wins projection, like every other harness
// object. All Option/Vec fields carry `#[serde(default)]` so v0 rows stay
// forward-compatible as fields are added.
// ---------------------------------------------------------------------------

/// Lifecycle of an [`AgentTeamRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunStatus {
    Planning,
    Running,
    Waiting,
    Reviewing,
    Completed,
    Failed,
    Cancelled,
}

/// One execution attempt of one durable AgentTeam. Team identity, Node
/// placement, and project binding are required fences; Mission identity is
/// reached through the Team rather than copied onto the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTeamRun {
    pub id: String,
    pub agent_team_id: String,
    /// Node that owns execution for this TeamRun. It must match the parent
    /// AgentTeam's immutable `node_id` at the Store boundary.
    pub execution_node_id: String,
    /// Project registration selected on `execution_node_id`.
    pub project_binding_id: String,
    #[serde(default)]
    pub previous_run_id: Option<String>,
    pub host_surface: String,
    #[serde(default)]
    pub host_thread_id: Option<String>,
    /// Typed Lead identity for new writes. Historical rows infer the reserved
    /// Host actor from `host_surface` and `host_thread_id`.
    #[serde(default)]
    pub host_actor: Option<TeamActorRef>,
    /// Whether Harness owns a persistent Host connection or observes an
    /// external provider task through safe-boundary hooks.
    #[serde(default)]
    pub host_control_mode: HostControlMode,
    pub objective: String,
    /// Concrete root selected for this attempt's execution. This is distinct
    /// from both the registered project root and the centralized store root.
    /// Older rows may omit it; callers then fall back to the project root.
    #[serde(default)]
    pub execution_root: Option<String>,
    pub status: TeamRunStatus,
    #[serde(default)]
    pub member_run_ids: Vec<String>,
    #[serde(default)]
    pub budget_limit_usd: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostControlMode {
    Managed,
    #[default]
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamSupervisorLeaseStatus {
    Active,
    Released,
}

/// Lifecycle of a machine-scoped execution Node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionNodeStatus {
    Active,
    Draining,
    Retired,
}

/// Durable machine identity. `id` is a stable UUID generated once when the
/// Node is enrolled; a daemon restart never changes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionNode {
    pub id: String,
    pub display_name: String,
    pub status: ExecutionNodeStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeProjectRegistrationStatus {
    Active,
    Disabled,
}

/// One project binding made available on one Node inside one Execution Space.
/// Latest-row identity is the `(node_id, execution_space_id,
/// project_binding_id)` composite key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeProjectRegistration {
    pub node_id: String,
    pub execution_space_id: String,
    pub project_binding_id: String,
    pub status: NodeProjectRegistrationStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeDaemonLeaseStatus {
    Active,
    Draining,
    Released,
    Expired,
}

/// Exclusive machine-scoped authority for the one NodeDaemon allowed to
/// manage all AgentTeams placed on a Node. Latest row wins by `node_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDaemonLease {
    pub node_id: String,
    pub daemon_id: String,
    pub generation: u64,
    pub instance_id: String,
    pub status: NodeDaemonLeaseStatus,
    pub acquired_unix_ms: u64,
    pub renewed_unix_ms: u64,
    pub expires_unix_ms: u64,
    #[serde(default)]
    pub released_unix_ms: Option<u64>,
}

/// Durable ownership record for the one process/service allowed to control a
/// TeamRun's provider-native sessions. Latest row wins by `team_run_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamSupervisorLease {
    pub team_run_id: String,
    /// Parent NodeDaemon fence. A Team supervisor cannot outlive or move
    /// independently from the daemon generation that created it.
    pub node_id: String,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub execution_space_id: String,
    pub project_binding_id: String,
    pub supervisor_id: String,
    pub generation: u64,
    pub owner_process_id: u32,
    pub owner_locator: String,
    pub status: TeamSupervisorLeaseStatus,
    pub acquired_unix_ms: u64,
    pub heartbeat_unix_ms: u64,
    pub expires_unix_ms: u64,
    #[serde(default)]
    pub released_unix_ms: Option<u64>,
}

/// Kind of Host actor holding the exclusive lease for a TeamRun's exact
/// provider-native Host binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostBindingLeaseOwnerKind {
    Interactive,
    Dispatcher,
}

/// Persisted lifecycle of a [`HostBindingLease`]. Expiry is deliberately not
/// a third status: an `Active` row is effective only while `expires_unix_ms`
/// is strictly greater than the observation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostBindingLeaseStatus {
    Active,
    Released,
}

/// Exclusive, provider-neutral ownership of one TeamRun's exact Host task.
///
/// Rows are append-only and latest-wins by `team_run_id`. Every successful
/// takeover advances `generation`; renew/release operations must present the
/// complete generation + lease id + owner fence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBindingLease {
    pub team_run_id: String,
    pub host_surface: String,
    pub host_thread_id: String,
    pub owner_kind: HostBindingLeaseOwnerKind,
    pub owner_id: String,
    pub generation: u64,
    pub lease_id: String,
    pub acquired_unix_ms: u64,
    pub heartbeat_unix_ms: u64,
    pub expires_unix_ms: u64,
    pub status: HostBindingLeaseStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_unix_ms: Option<u64>,
}

impl HostBindingLease {
    pub fn is_effective_at(&self, now_unix_ms: u64) -> bool {
        self.status == HostBindingLeaseStatus::Active && self.expires_unix_ms > now_unix_ms
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMemberCloseStatus {
    Pending,
    Applied,
}

/// Durable Host request to end one ProviderRuntimeProjection runtime. The owning Supervisor
/// applies the latest pending row before starting or resuming provider work.
/// Latest row wins by `member_run_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMemberCloseRequest {
    pub id: String,
    pub team_run_id: String,
    pub member_run_id: String,
    pub requested_by: String,
    pub reason: String,
    pub status: TeamMemberCloseStatus,
    pub requested_at: String,
    #[serde(default)]
    pub applied_at: Option<String>,
}

/// Non-secret workspace facts observed when a member runtime starts.
///
/// These values make the execution location reconstructable without copying
/// instruction/skill contents or any provider-native transcript or tool
/// stream into Harness storage. `git_branch` is absent for detached HEADs and
/// all Git fields are absent outside a Git worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberWorkspaceSnapshot {
    pub cwd: String,
    /// Stable Project Binding used to validate this cwd, when one was selected.
    #[serde(default)]
    pub project_binding_id: Option<String>,
    /// Why this exact cwd won: `member_worktree`, `team_execution_root`,
    /// `project_binding_root`, or `explicit_unbound`.
    #[serde(default)]
    pub resolution_source: Option<String>,
    #[serde(default)]
    pub git_head: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    /// Directories containing discovered instruction files used for context.
    #[serde(default)]
    pub instruction_roots: Vec<String>,
    /// Directories containing discovered skills used for context.
    #[serde(default)]
    pub skill_roots: Vec<String>,
}

/// Lifecycle of a [`ProviderRuntimeProjection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRunStatus {
    Starting,
    Idle,
    Queued,
    Running,
    Waiting,
    /// The durable ProviderRuntimeProjection and native-session binding still exist, but the
    /// Supervisor currently has no healthy provider transport. This is
    /// recoverable and intentionally distinct from `Failed` or `Stopped`.
    Disconnected,
    Reviewing,
    Blocked,
    Completed,
    Failed,
    Stopped,
}

/// The provider execution boundary fenced before any provider-native side
/// effect. This enum is deliberately closed: free-form boundary prose must
/// never become recovery authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCompatibilityBlockBoundary {
    StartPersistentExecution,
    ResumePersistentExecution,
}

/// The compatibility resolver branch that caused a durable provider block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCompatibilityBlockSource {
    AdapterCompatibility,
    ProbeFailure,
}

/// Typed, replay-validatable authority for a compatibility-owned block.
///
/// `MemberAction` remains an audit projection. Neither action type nor action
/// summary can create or clear this authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCompatibilityBlockCause {
    pub schema_version: u32,
    pub id: String,
    pub member_run_id: String,
    pub provider: String,
    pub execution_mode: String,
    pub provider_version: String,
    pub adapter_contract_version: String,
    pub boundary: ProviderCompatibilityBlockBoundary,
    pub compatibility_status: ProviderCompatibilityStatus,
    pub source: ProviderCompatibilityBlockSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_error: Option<String>,
    pub caused_at: String,
}

impl ProviderCompatibilityBlockCause {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn exact_key(&self) -> (&str, &str, &str, &str) {
        (
            &self.provider,
            &self.execution_mode,
            &self.provider_version,
            &self.adapter_contract_version,
        )
    }
}

impl Validate for ProviderCompatibilityBlockCause {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(ValidationError::Invalid {
                field: "ProviderCompatibilityBlockCause.schema_version",
                reason: "unsupported schema version",
            });
        }
        require_non_empty(&self.id, "ProviderCompatibilityBlockCause.id")?;
        require_non_empty(
            &self.member_run_id,
            "ProviderCompatibilityBlockCause.member_run_id",
        )?;
        require_non_empty(&self.provider, "ProviderCompatibilityBlockCause.provider")?;
        require_non_empty(
            &self.execution_mode,
            "ProviderCompatibilityBlockCause.execution_mode",
        )?;
        require_non_empty(
            &self.provider_version,
            "ProviderCompatibilityBlockCause.provider_version",
        )?;
        require_non_empty(
            &self.adapter_contract_version,
            "ProviderCompatibilityBlockCause.adapter_contract_version",
        )?;
        require_non_empty(&self.caused_at, "ProviderCompatibilityBlockCause.caused_at")?;
        match (self.compatibility_status, self.source, &self.probe_error) {
            (
                ProviderCompatibilityStatus::Unavailable,
                ProviderCompatibilityBlockSource::ProbeFailure,
                Some(error),
            ) => require_non_empty(error, "ProviderCompatibilityBlockCause.probe_error")?,
            (
                ProviderCompatibilityStatus::ReviewRequired
                | ProviderCompatibilityStatus::Incompatible
                | ProviderCompatibilityStatus::Unknown,
                ProviderCompatibilityBlockSource::AdapterCompatibility,
                None,
            ) => {}
            _ => {
                return Err(ValidationError::Invalid {
                    field: "ProviderCompatibilityBlockCause.compatibility_status",
                    reason: "status, source, and probe_error are inconsistent",
                });
            }
        }
        Ok(())
    }
}

/// Durable coordination lifecycle of one ProviderRuntimeProjection, separate from its
/// provider runtime/work status. Close is reversible; Retire is permanent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberCoordinationStatus {
    #[default]
    Active,
    Closed,
    Retired,
}

const fn default_member_runtime_generation() -> u64 {
    1
}

/// A provider-owned conversation/runtime that contains the execution truth for
/// one member. Harness persists this locator and capability snapshot, but does
/// not copy the provider's transcript, tool stream, command output, or turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSessionRef {
    pub provider: String,
    pub execution_mode: String,
    pub native_session_id: String,
    pub native_locator_kind: String,
    #[serde(default)]
    pub provider_version: Option<String>,
    pub adapter_contract_version: String,
    #[serde(default)]
    pub availability: NativeSessionAvailability,
    pub supports_resume: bool,
    #[serde(default)]
    pub last_verified_at: Option<String>,
    #[serde(default)]
    pub parent_native_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeSessionAvailability {
    Available,
    Stale,
    Missing,
    Incompatible,
    #[default]
    Unknown,
}

/// Provider-neutral control lifecycle for one requested execution setting.
///
/// `requested` is Harness intent. `effective` is populated only from a
/// provider-native receipt or a reviewed protocol guarantee; adapters must
/// never copy the request into this field merely for display. Unsupported and
/// unreviewed settings remain explicit so the Dashboard cannot imply that a
/// model, reasoning effort, or latency tier took effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderControlStatus {
    #[default]
    NotRequested,
    Requested,
    Effective,
    Unsupported,
    ReviewRequired,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderControlValue {
    #[serde(default)]
    pub requested: Option<String>,
    #[serde(default)]
    pub effective: Option<String>,
    #[serde(default)]
    pub status: ProviderControlStatus,
    #[serde(default)]
    pub note: Option<String>,
}

impl ProviderControlValue {
    pub fn requested(value: Option<String>) -> Self {
        Self {
            status: if value.is_some() {
                ProviderControlStatus::Requested
            } else {
                ProviderControlStatus::NotRequested
            },
            requested: value,
            effective: None,
            note: None,
        }
    }

    pub fn mark_effective(&mut self, value: Option<String>, note: impl Into<String>) {
        self.effective = value;
        self.status = ProviderControlStatus::Effective;
        self.note = Some(note.into());
    }

    pub fn mark_unsupported(&mut self, note: impl Into<String>) {
        self.effective = None;
        self.status = ProviderControlStatus::Unsupported;
        self.note = Some(note.into());
    }

    pub fn mark_review_required(&mut self, note: impl Into<String>) {
        self.effective = None;
        self.status = ProviderControlStatus::ReviewRequired;
        self.note = Some(note.into());
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderExecutionControls {
    #[serde(default)]
    pub model: ProviderControlValue,
    #[serde(default)]
    pub reasoning_effort: ProviderControlValue,
    #[serde(default)]
    pub service_tier: ProviderControlValue,
}

impl ProviderExecutionControls {
    pub fn requested(
        model: Option<String>,
        reasoning_effort: Option<String>,
        service_tier: Option<String>,
    ) -> Self {
        Self {
            model: ProviderControlValue::requested(model),
            reasoning_effort: ProviderControlValue::requested(reasoning_effort),
            service_tier: ProviderControlValue::requested(service_tier),
        }
    }
}

/// Runtime availability of one provider account for one execution mode.
///
/// This is deliberately NOT [`ProviderCompatibilityStatus`]. Compatibility
/// answers "has this adapter been reviewed against the installed provider
/// version"; capacity answers "can this account actually execute a turn right
/// now". Wave 2 proved the two are independent: a `current` Claude adapter
/// still returned 403 because the Harness process lacked the required proxy,
/// and a `current` Kimi adapter still returned a quota 403.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapacityState {
    /// A reviewed provider signal says this account can execute now.
    Available,
    /// A reviewed provider signal says usage is high but not blocking.
    Limited,
    /// A reviewed provider signal says the account is out of capacity.
    Exhausted,
    /// A reviewed provider signal says the credential is missing or rejected.
    Unauthorized,
    /// Nothing reviewed was observed. This never means "available" and never
    /// borrows the adapter's compatibility verdict.
    #[default]
    Unknown,
}

impl ProviderCapacityState {
    /// `true` only for states a reviewed provider signal proved are blocking.
    /// `Unknown` is explicitly not blocking: honesty must not become a gate.
    pub fn is_known_unavailable(self) -> bool {
        matches!(self, Self::Exhausted | Self::Unauthorized)
    }
}

/// Where a [`ProviderCapacitySnapshot`] came from. The reader must be able to
/// tell a quota API answer apart from "a credential file exists on disk".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapacityEvidence {
    /// A reviewed provider RPC/endpoint that reports account limits.
    ProviderQuotaApi,
    /// Credential/auth metadata — read locally OR from a provider account
    /// endpoint. It proves a credential's presence or absence, never that a
    /// request would succeed.
    AuthMetadata,
    /// A real, minimal provider request issued through the execution path.
    ExecutionCanary,
    /// A terminal provider error already observed by this Harness.
    ProviderError,
    /// The reviewed protocol for this execution mode exposes no capacity API.
    NotExposed,
    /// A probe was attempted and failed before producing a provider answer.
    ProbeFailed,
    #[default]
    None,
}

/// How much the snapshot's `state` can be trusted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapacityConfidence {
    /// Read directly from a provider answer.
    Observed,
    /// Derived from an adjacent fact (an error, a credential, an env gap).
    Inferred,
    #[default]
    Unknown,
}

/// One provider-reported usage window. `used_percent` is only ever populated
/// from a provider number; adapters must never synthesise one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapacityWindow {
    pub label: String,
    #[serde(default)]
    pub limit_id: Option<String>,
    #[serde(default)]
    pub used_percent: Option<i64>,
    #[serde(default)]
    pub window_duration_mins: Option<i64>,
    #[serde(default)]
    pub resets_at: Option<String>,
}

/// The account/source boundary a snapshot describes. Two members on one
/// provider can hold different accounts, so capacity is never global.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderAccountRef {
    /// Neutral credential source spelling: `chatgpt`, `api_key`,
    /// `amazon_bedrock`, `oauth_credentials_file`, `unknown`, …
    pub source: String,
    /// Non-secret account identifier when the provider returns one.
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
}

impl ProviderAccountRef {
    pub fn unknown() -> Self {
        Self {
            source: "unknown".to_string(),
            identifier: None,
            plan: None,
        }
    }
}

/// One non-secret fact about the runtime environment the provider would be
/// launched into. This is what turns "403" into "the Harness process has no
/// HTTPS_PROXY" instead of a guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRuntimeContextFact {
    pub key: String,
    pub present: bool,
    /// Non-secret description only (for example `set`, `absent`, a host name).
    /// Adapters must never copy a token or credential here.
    #[serde(default)]
    pub note: Option<String>,
}

/// Execution-mode-specific runtime availability of one provider account.
///
/// Every field is provider-neutral. `state` never inherits from
/// [`ProviderCompatibilityStatus`], and an absent snapshot is never treated as
/// available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapacitySnapshot {
    pub provider: String,
    pub execution_mode: String,
    pub account: ProviderAccountRef,
    pub state: ProviderCapacityState,
    /// RFC-ish harness timestamp string of the observation.
    pub observed_at: String,
    /// Unix milliseconds of the observation. Staleness is computed from this
    /// so a snapshot read back from the store cannot silently look fresh.
    pub observed_unix_ms: u64,
    /// When the provider says the blocking window reopens.
    #[serde(default)]
    pub reset_at: Option<String>,
    pub evidence_source: ProviderCapacityEvidence,
    pub confidence: ProviderCapacityConfidence,
    #[serde(default)]
    pub windows: Vec<ProviderCapacityWindow>,
    /// Actionable explanation when the observed failure is a runtime/context
    /// gap rather than an account limit.
    #[serde(default)]
    pub diagnosis: Option<String>,
    #[serde(default)]
    pub runtime_context: Vec<ProviderRuntimeContextFact>,
    #[serde(default)]
    pub detail: Option<String>,
}

impl ProviderCapacitySnapshot {
    /// An honest "nothing was observed" snapshot. Used whenever a probe cannot
    /// reach a reviewed provider answer.
    pub fn unknown(
        provider: impl Into<String>,
        execution_mode: impl Into<String>,
        observed_at: impl Into<String>,
        observed_unix_ms: u64,
        evidence_source: ProviderCapacityEvidence,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            execution_mode: execution_mode.into(),
            account: ProviderAccountRef::unknown(),
            state: ProviderCapacityState::Unknown,
            observed_at: observed_at.into(),
            observed_unix_ms,
            reset_at: None,
            evidence_source,
            confidence: ProviderCapacityConfidence::Unknown,
            windows: Vec::new(),
            diagnosis: None,
            runtime_context: Vec::new(),
            detail: Some(detail.into()),
        }
    }

    pub fn freshness(&self, now_unix_ms: u64, ttl_ms: u64) -> ProviderCapacityFreshness {
        if self.observed_unix_ms == 0 || now_unix_ms < self.observed_unix_ms {
            // A missing or future-dated observation is not evidence of
            // freshness. Treat it as unknown rather than trusting it.
            return ProviderCapacityFreshness::Unknown;
        }
        if now_unix_ms.saturating_sub(self.observed_unix_ms) <= ttl_ms {
            ProviderCapacityFreshness::Fresh
        } else {
            ProviderCapacityFreshness::Stale
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapacityFreshness {
    Fresh,
    Stale,
    Unknown,
}

/// Default staleness bound for a start-time capacity decision: five minutes.
pub const PROVIDER_CAPACITY_DEFAULT_TTL_MS: u64 = 5 * 60 * 1000;

/// Parse a `unix-ms:<millis>` harness timestamp.
///
/// Timestamps must be compared as numbers. String ordering happens to agree
/// only while every stamp has the same digit count, which is a bug waiting for
/// a boundary rather than a comparison.
pub fn parse_harness_unix_ms(raw: &str) -> Option<u64> {
    raw.strip_prefix("unix-ms:")?.trim().parse::<u64>().ok()
}

/// Whether a ProviderRuntimeProjection may claim and consume its Assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum ProviderCapacityStartDecision {
    Proceed {
        reason: String,
    },
    /// The Assignment must stay queued and unclaimed.
    Block {
        state: ProviderCapacityState,
        reason: String,
    },
}

impl ProviderCapacityStartDecision {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Block { .. })
    }

    pub fn reason(&self) -> &str {
        match self {
            Self::Proceed { reason } | Self::Block { reason, .. } => reason,
        }
    }
}

/// Decide whether a member may start, from a capacity snapshot alone.
///
/// The rule is deliberately narrow so honesty never becomes a gate:
/// block ONLY on a snapshot that is both FRESH and KNOWN unavailable. No
/// snapshot, an unknown state, or a stale observation all proceed — and none
/// of them is recorded as "available".
pub fn provider_capacity_start_decision(
    snapshot: Option<&ProviderCapacitySnapshot>,
    now_unix_ms: u64,
    ttl_ms: u64,
) -> ProviderCapacityStartDecision {
    let Some(snapshot) = snapshot else {
        return ProviderCapacityStartDecision::Proceed {
            reason: "no capacity snapshot was observed; start is not gated by an unknown"
                .to_string(),
        };
    };
    if !snapshot.state.is_known_unavailable() {
        return ProviderCapacityStartDecision::Proceed {
            reason: format!(
                "capacity state {:?} is not a known-unavailable provider answer",
                snapshot.state
            )
            .to_lowercase(),
        };
    }
    match snapshot.freshness(now_unix_ms, ttl_ms) {
        ProviderCapacityFreshness::Fresh => ProviderCapacityStartDecision::Block {
            state: snapshot.state,
            reason: format!(
                "provider {} ({}) reported {} for account source {}{}",
                snapshot.provider,
                snapshot.execution_mode,
                match snapshot.state {
                    ProviderCapacityState::Exhausted => "exhausted capacity",
                    ProviderCapacityState::Unauthorized => "an unauthorized credential",
                    _ => "a blocking state",
                },
                snapshot.account.source,
                snapshot
                    .reset_at
                    .as_ref()
                    .map(|reset| format!("; resets at {reset}"))
                    .unwrap_or_default()
            ),
        },
        ProviderCapacityFreshness::Stale | ProviderCapacityFreshness::Unknown => {
            ProviderCapacityStartDecision::Proceed {
                reason: "the known-unavailable snapshot is no longer fresh; re-observe instead of \
                         gating on stale evidence"
                    .to_string(),
            }
        }
    }
}

/// One member's session inside an [`AgentTeamRun`]. `provider` is the neutral
/// provider spelling (codex|claude|kimi). `native_session` points to the
/// provider-owned execution record; Harness owns only the surrounding
/// coordination state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRuntimeProjection {
    pub id: String,
    pub team_run_id: String,
    #[serde(default)]
    pub slot_id: Option<String>,
    /// Required stable link to the one canonical durable AgentMember.
    pub agent_member_id: String,
    pub name: String,
    pub role: String,
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    /// Immutable requested controls plus provider-confirmed effective values.
    /// `model` above remains as a wire-compatible shortcut for older readers.
    #[serde(default)]
    pub provider_controls: ProviderExecutionControls,
    /// Immutable-at-start snapshot of the concrete provider execution path.
    /// This distinguishes provider-native capability from what this adapter
    /// and execution mode have actually wired for the run.
    #[serde(default)]
    pub provider_profile: Option<ProviderIntegrationProfile>,
    /// Last observed runtime availability of this member's provider account.
    /// Absent means nothing was observed; it never means available, and it is
    /// independent of `provider_profile.compatibility_status`.
    #[serde(default)]
    pub provider_capacity: Option<ProviderCapacitySnapshot>,
    /// Present only while the Store's provider-compatibility transition owns
    /// this ProviderRuntimeProjection's Blocked state. Generic ProviderRuntimeProjection CAS cannot set or
    /// clear it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_compatibility_block_cause: Option<ProviderCompatibilityBlockCause>,
    /// Durable mailbox/participation state, independent of the process state
    /// represented by `status`.
    #[serde(default)]
    pub coordination_status: MemberCoordinationStatus,
    /// Monotonic activation generation. Explicit Reopen increments this so a
    /// live Supervisor can start a new process for the same ProviderRuntimeProjection id.
    #[serde(default = "default_member_runtime_generation")]
    pub runtime_generation: u64,
    pub status: MemberRunStatus,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
    #[serde(default)]
    pub provider_cwd_hint: Option<String>,
    /// Facts actually observed from the spawned member's working directory and
    /// non-secret instruction/skill roots discovered from that environment.
    #[serde(default)]
    pub provider_environment_observation: Option<MemberWorkspaceSnapshot>,
    #[serde(default)]
    pub owned_paths: Vec<String>,
    /// Consecutive provider turns where the member produced no tool calls
    /// AND no Work transitions. Persisted so the degradation streak survives
    /// supervisor restart. Reset to 0 on any productive turn.
    #[serde(default)]
    pub zero_output_streak: u32,
    /// The last Work version the member consumed (saw at turn-start). When
    /// this equals the current Work version, the version-Continue arm in
    /// decide_wake is suppressed to avoid re-waking on stale content.
    #[serde(default)]
    pub last_consumed_work_version: Option<u64>,
    pub started_at: String,
    #[serde(default)]
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

impl ProviderRuntimeProjection {
    pub fn coordination_is_active(&self) -> bool {
        self.coordination_status == MemberCoordinationStatus::Active
    }

    pub fn coordination_is_closed(&self) -> bool {
        self.coordination_status == MemberCoordinationStatus::Closed
    }

    pub fn coordination_is_retired(&self) -> bool {
        self.coordination_status == MemberCoordinationStatus::Retired
    }

    /// Whether this is a declared non-driven external interactive member (see
    /// [`EXECUTION_MODE_EXTERNAL_INTERACTIVE`]). The Supervisor must not spawn
    /// a provider adapter for it; its deliveries stay queued until the
    /// external session polls and acks.
    pub fn is_external_interactive(&self) -> bool {
        self.provider_profile.as_ref().is_some_and(|profile| {
            profile.execution_mode == EXECUTION_MODE_EXTERNAL_INTERACTIVE
                && profile.execution_driver == MemberExecutionDriver::UserDriven
        })
    }
}

impl Validate for AgentTeamRun {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentTeamRun.id")?;
        require_non_empty(&self.agent_team_id, "AgentTeamRun.agent_team_id")?;
        require_uuid(&self.execution_node_id, "AgentTeamRun.execution_node_id")?;
        require_non_empty(&self.project_binding_id, "AgentTeamRun.project_binding_id")?;
        require_non_empty(&self.host_surface, "AgentTeamRun.host_surface")?;
        require_non_empty(&self.objective, "AgentTeamRun.objective")?;
        require_non_empty(&self.created_at, "AgentTeamRun.created_at")?;
        require_non_empty(&self.updated_at, "AgentTeamRun.updated_at")?;
        if let Some(execution_root) = &self.execution_root {
            require_non_empty(execution_root, "AgentTeamRun.execution_root")?;
        }
        if let Some(actor) = &self.host_actor {
            require_non_empty(&actor.id, "AgentTeamRun.host_actor.id")?;
            validate_actor_metadata(actor, "AgentTeamRun.host_actor")?;
        }
        validate_non_empty_unique_strings(
            &self.member_run_ids,
            "AgentTeamRun.member_run_ids",
            true,
        )?;
        Ok(())
    }
}

impl Validate for ExecutionNode {
    fn validate(&self) -> Result<(), ValidationError> {
        require_uuid(&self.id, "ExecutionNode.id")?;
        require_non_empty(&self.display_name, "ExecutionNode.display_name")?;
        require_non_empty(&self.created_at, "ExecutionNode.created_at")?;
        require_non_empty(&self.updated_at, "ExecutionNode.updated_at")
    }
}

impl Validate for NodeProjectRegistration {
    fn validate(&self) -> Result<(), ValidationError> {
        require_uuid(&self.node_id, "NodeProjectRegistration.node_id")?;
        require_non_empty(
            &self.execution_space_id,
            "NodeProjectRegistration.execution_space_id",
        )?;
        require_non_empty(
            &self.project_binding_id,
            "NodeProjectRegistration.project_binding_id",
        )?;
        require_non_empty(&self.created_at, "NodeProjectRegistration.created_at")?;
        require_non_empty(&self.updated_at, "NodeProjectRegistration.updated_at")
    }
}

impl Validate for NodeDaemonLease {
    fn validate(&self) -> Result<(), ValidationError> {
        require_uuid(&self.node_id, "NodeDaemonLease.node_id")?;
        require_non_empty(&self.daemon_id, "NodeDaemonLease.daemon_id")?;
        require_non_empty(&self.instance_id, "NodeDaemonLease.instance_id")?;
        if self.generation == 0 {
            return Err(ValidationError::Invalid {
                field: "NodeDaemonLease.generation",
                reason: "must be greater than zero",
            });
        }
        if self.renewed_unix_ms < self.acquired_unix_ms
            || self.expires_unix_ms < self.renewed_unix_ms
        {
            return Err(ValidationError::Invalid {
                field: "NodeDaemonLease.timestamps",
                reason: "must be monotonic",
            });
        }
        match (self.status, self.released_unix_ms) {
            (NodeDaemonLeaseStatus::Released, Some(released))
                if released >= self.acquired_unix_ms =>
            {
                Ok(())
            }
            (NodeDaemonLeaseStatus::Released, _) => Err(ValidationError::Invalid {
                field: "NodeDaemonLease.released_unix_ms",
                reason: "released leases require a release time after acquisition",
            }),
            (_, Some(_)) => Err(ValidationError::Invalid {
                field: "NodeDaemonLease.released_unix_ms",
                reason: "only released leases may carry a release time",
            }),
            _ => Ok(()),
        }
    }
}

impl Validate for TeamSupervisorLease {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.team_run_id, "TeamSupervisorLease.team_run_id")?;
        require_uuid(&self.node_id, "TeamSupervisorLease.node_id")?;
        require_non_empty(&self.node_daemon_id, "TeamSupervisorLease.node_daemon_id")?;
        if self.node_daemon_generation == 0 {
            return Err(ValidationError::Invalid {
                field: "TeamSupervisorLease.node_daemon_generation",
                reason: "must be greater than zero",
            });
        }
        require_non_empty(
            &self.execution_space_id,
            "TeamSupervisorLease.execution_space_id",
        )?;
        require_non_empty(
            &self.project_binding_id,
            "TeamSupervisorLease.project_binding_id",
        )?;
        require_non_empty(&self.supervisor_id, "TeamSupervisorLease.supervisor_id")?;
        require_non_empty(&self.owner_locator, "TeamSupervisorLease.owner_locator")?;
        if self.generation == 0 {
            return Err(ValidationError::Invalid {
                field: "TeamSupervisorLease.generation",
                reason: "must be greater than zero",
            });
        }
        if self.owner_process_id == 0 {
            return Err(ValidationError::Invalid {
                field: "TeamSupervisorLease.owner_process_id",
                reason: "must be greater than zero",
            });
        }
        if self.heartbeat_unix_ms < self.acquired_unix_ms
            || self.expires_unix_ms < self.heartbeat_unix_ms
        {
            return Err(ValidationError::Invalid {
                field: "TeamSupervisorLease.timestamps",
                reason: "must be monotonic",
            });
        }
        match (self.status, self.released_unix_ms) {
            (TeamSupervisorLeaseStatus::Released, Some(released))
                if released >= self.acquired_unix_ms =>
            {
                Ok(())
            }
            (TeamSupervisorLeaseStatus::Released, _) => Err(ValidationError::Invalid {
                field: "TeamSupervisorLease.released_unix_ms",
                reason: "released leases require a release time after acquisition",
            }),
            (_, Some(_)) => Err(ValidationError::Invalid {
                field: "TeamSupervisorLease.released_unix_ms",
                reason: "only released leases may carry a release time",
            }),
            _ => Ok(()),
        }
    }
}

impl Validate for HostBindingLease {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.team_run_id, "HostBindingLease.team_run_id")?;
        require_non_empty(&self.host_surface, "HostBindingLease.host_surface")?;
        require_non_empty(&self.host_thread_id, "HostBindingLease.host_thread_id")?;
        require_non_empty(&self.owner_id, "HostBindingLease.owner_id")?;
        require_non_empty(&self.lease_id, "HostBindingLease.lease_id")?;
        if self.generation == 0 {
            return Err(ValidationError::Invalid {
                field: "HostBindingLease.generation",
                reason: "must be greater than zero",
            });
        }
        if self.heartbeat_unix_ms < self.acquired_unix_ms
            || self.expires_unix_ms < self.heartbeat_unix_ms
        {
            return Err(ValidationError::Invalid {
                field: "HostBindingLease.timestamps",
                reason: "must be monotonic",
            });
        }
        match (self.status, self.released_unix_ms) {
            (HostBindingLeaseStatus::Active, None) => {}
            (HostBindingLeaseStatus::Released, Some(released))
                if released >= self.acquired_unix_ms
                    && self.expires_unix_ms == released
                    && self.heartbeat_unix_ms == released => {}
            _ => {
                return Err(ValidationError::Invalid {
                    field: "HostBindingLease.status",
                    reason: "release fields do not match status",
                });
            }
        }
        Ok(())
    }
}

impl Validate for TeamMemberCloseRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "TeamMemberCloseRequest.id")?;
        require_non_empty(&self.team_run_id, "TeamMemberCloseRequest.team_run_id")?;
        require_non_empty(&self.member_run_id, "TeamMemberCloseRequest.member_run_id")?;
        require_non_empty(&self.requested_by, "TeamMemberCloseRequest.requested_by")?;
        require_non_empty(&self.reason, "TeamMemberCloseRequest.reason")?;
        require_non_empty(&self.requested_at, "TeamMemberCloseRequest.requested_at")
    }
}

impl Validate for MemberWorkspaceSnapshot {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.cwd, "MemberWorkspaceSnapshot.cwd")?;
        if let Some(binding) = &self.project_binding_id {
            require_non_empty(binding, "MemberWorkspaceSnapshot.project_binding_id")?;
        }
        if let Some(source) = &self.resolution_source {
            require_non_empty(source, "MemberWorkspaceSnapshot.resolution_source")?;
        }
        if let Some(git_head) = &self.git_head {
            require_non_empty(git_head, "MemberWorkspaceSnapshot.git_head")?;
        }
        if let Some(git_branch) = &self.git_branch {
            require_non_empty(git_branch, "MemberWorkspaceSnapshot.git_branch")?;
        }
        for root in &self.instruction_roots {
            require_non_empty(root, "MemberWorkspaceSnapshot.instruction_roots")?;
        }
        for root in &self.skill_roots {
            require_non_empty(root, "MemberWorkspaceSnapshot.skill_roots")?;
        }
        Ok(())
    }
}

impl Validate for ProviderRuntimeProjection {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "ProviderRuntimeProjection.id")?;
        require_non_empty(&self.team_run_id, "ProviderRuntimeProjection.team_run_id")?;
        require_non_empty(
            &self.agent_member_id,
            "ProviderRuntimeProjection.agent_member_id",
        )?;
        require_non_empty(&self.name, "ProviderRuntimeProjection.name")?;
        require_non_empty(&self.role, "ProviderRuntimeProjection.role")?;
        require_non_empty(&self.provider, "ProviderRuntimeProjection.provider")?;
        require_non_empty(&self.started_at, "ProviderRuntimeProjection.started_at")?;
        if self.runtime_generation == 0 {
            return Err(ValidationError::Invalid {
                field: "ProviderRuntimeProjection.runtime_generation",
                reason: "must be at least 1",
            });
        }
        if let Some(provider_cwd_hint) = &self.provider_cwd_hint {
            require_non_empty(
                provider_cwd_hint,
                "ProviderRuntimeProjection.provider_cwd_hint",
            )?;
        }
        if let Some(snapshot) = &self.provider_environment_observation {
            snapshot.validate()?;
        }
        if let Some(cause) = &self.provider_compatibility_block_cause {
            cause.validate()?;
            if self.status != MemberRunStatus::Blocked {
                return Err(ValidationError::Invalid {
                    field: "ProviderRuntimeProjection.provider_compatibility_block_cause",
                    reason: "typed compatibility cause requires Blocked status",
                });
            }
            if cause.member_run_id != self.id || cause.provider != self.provider {
                return Err(ValidationError::Invalid {
                    field: "ProviderRuntimeProjection.provider_compatibility_block_cause",
                    reason: "typed compatibility cause does not match ProviderRuntimeProjection identity",
                });
            }
            let profile = self
                .provider_profile
                .as_ref()
                .ok_or(ValidationError::Invalid {
                    field: "ProviderRuntimeProjection.provider_compatibility_block_cause",
                    reason: "typed compatibility cause requires the observed provider profile",
                })?;
            if cause.compatibility_status != profile.compatibility_status
                || cause.exact_key()
                    != (
                        profile.provider.as_str(),
                        profile.execution_mode.as_str(),
                        profile.provider_version.as_deref().unwrap_or("unavailable"),
                        profile
                            .adapter_contract_version
                            .as_deref()
                            .unwrap_or("unknown"),
                    )
            {
                return Err(ValidationError::Invalid {
                    field: "ProviderRuntimeProjection.provider_compatibility_block_cause",
                    reason: "typed compatibility cause does not match the observed provider tuple",
                });
            }
        }
        Ok(())
    }
}
