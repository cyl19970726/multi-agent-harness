use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Blocked,
    Review,
    Done,
    /// Deprecated legacy terminal state (ADR 0019): retained for old rows; new
    /// writers emit `Done`. Read models fold `Complete` into `Done`.
    Complete,
    Archived,
}

/// Shared git/worktree context for a Goal or Task (ADR 0019). All fields
/// optional; additive — old rows that omit it deserialize as `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GitMetadata {
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub pr_ref: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub owned_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub owner_agent_id: String,
    pub status: GoalStatus,
    pub success_criteria: Vec<String>,
    pub priority: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub vision_id: Option<String>,
    #[serde(default)]
    pub goal_design_id: Option<String>,
    #[serde(default)]
    pub closed_by_decision_id: Option<String>,
    #[serde(default)]
    pub git_metadata: Option<GitMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMemberStatus {
    Creating,
    Idle,
    Assigned,
    Running,
    WaitingForInput,
    WaitingForApproval,
    Reviewing,
    Blocked,
    Closing,
    Closed,
    Error,
    Paused,
    Stale,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentProviderConfig {
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub collaboration_mode: Option<String>,
    #[serde(default)]
    pub approval_policy: Option<String>,
    #[serde(default)]
    pub approvals_reviewer: Option<String>,
    #[serde(default)]
    pub sandbox_policy: Option<String>,
    #[serde(default)]
    pub permission_profile: Option<String>,
    #[serde(default)]
    pub runtime_workspace_roots: Vec<String>,
    #[serde(default)]
    pub environment_id: Option<String>,
    /// Optional MCP servers attached to this member (Pillar 2).
    /// When present, `build_launch_spec` carries this to the neutral launch spec.
    #[serde(default)]
    pub mcp: Option<LaunchMcp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMember {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub provider: String,
    pub model: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub provider_config: AgentProviderConfig,
    pub capabilities: Vec<String>,
    pub team_ids: Vec<String>,
    pub prompt_ref: Option<String>,
    pub skill_refs: Vec<String>,
    pub workspace_policy: Option<String>,
    #[serde(default)]
    pub worktree_ref: Option<String>,
    #[serde(default)]
    pub permission_profile: Option<String>,
    #[serde(default)]
    pub runtime_workspace_roots: Vec<String>,
    pub status: AgentMemberStatus,
    pub current_task_id: Option<String>,
    pub current_proposal_id: Option<String>,
    pub provider_runtime_id: Option<String>,
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_agent_path: Option<String>,
    #[serde(default)]
    pub provider_agent_nickname: Option<String>,
    #[serde(default)]
    pub provider_agent_role: Option<String>,
    pub control_endpoint: Option<String>,
    pub created_at: String,
    pub last_seen_at: Option<String>,
}

/// Neutral permission posture for a single delivery turn.
///
/// This is the launch-spec `permission` enum from the launch-spec table in
/// [docs/agent-integration-model.md](../../../docs/agent-integration-model.md).
/// It deliberately does **not** reuse Codex wire vocabulary
/// (`readOnly` / `workspaceWrite` / `dangerFullAccess`): each provider adapter
/// (Pillar 3) translates this neutral enum onto its own controls — Codex
/// sandbox/approval flags, Claude `--permission-mode`, a future platform's
/// controls — per ADR 0011. The snake_case wire values (`read_only`,
/// `workspace_write`, `full_access`) are the neutral spelling, distinct from any
/// platform's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchPermission {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

impl LaunchPermission {
    pub fn as_str(&self) -> &'static str {
        match self {
            LaunchPermission::ReadOnly => "read_only",
            LaunchPermission::WorkspaceWrite => "workspace_write",
            LaunchPermission::FullAccess => "full_access",
        }
    }
}

impl Default for LaunchPermission {
    /// The safe default posture: a turn that has not declared a writable
    /// permission is read-only, never silently writable.
    fn default() -> Self {
        LaunchPermission::WorkspaceWrite
    }
}

/// One neutral MCP server entry for the launch spec.
///
/// This is the minimal neutral shape from the PROPOSED `mcp` block in
/// [docs/agent-integration-model.md](../../../docs/agent-integration-model.md)
/// (Pillar 2). It carries no platform wire vocabulary: each adapter maps it onto
/// `--config mcp_servers.*` (Codex) or `--mcp-config` (Claude). Provider-neutral.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchMcpServer {
    /// Stable id for the server.
    pub id: String,
    /// Transport hint (`stdio` / `http` / `sse`); free string, neutral.
    #[serde(default)]
    pub transport: Option<String>,
    /// argv for a local stdio server.
    #[serde(default)]
    pub command: Vec<String>,
    /// endpoint for a remote http/sse server.
    #[serde(default)]
    pub url: Option<String>,
    /// Tool allowlist for this server; empty = all tools on the server.
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

/// Minimal neutral MCP block for the launch spec (PROPOSED shape, Pillar 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LaunchMcp {
    #[serde(default)]
    pub servers: Vec<LaunchMcpServer>,
}

/// The provider-neutral launch spec: one normalized per-turn request.
///
/// This is the launch-spec table in
/// [docs/agent-integration-model.md](../../../docs/agent-integration-model.md).
/// The harness builds it from the member (Pillars 1–2) and the claimed
/// [`Message`] via [`build_launch_spec`]; each provider adapter (Pillar 3) then
/// maps it onto its own CLI/SDK call. It is the seam that keeps the operator
/// composer and Dashboard uniform across Codex, Claude, and future platforms.
///
/// Per ADR 0011 this neutral object carries **no** Codex wire vocabulary:
/// `permission` is the neutral [`LaunchPermission`] enum and `writable_roots`
/// replaces Codex's `workspaceWrite.writableRoots`. The Codex-leaking
/// `AgentProviderConfig` fields (`sandbox_policy`, `approval_policy`,
/// `service_tier`, `collaboration_mode`, …) are abstracted here, not reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchSpec {
    /// Composed system/developer instructions (Pillar 1 prompt stack), read as a
    /// durable artifact reference — not inline chat text. `None` when the member
    /// has no role prompt.
    #[serde(default)]
    pub prompt_ref: Option<String>,
    /// The turn input: the claimed [`Message`] envelope + content.
    pub message_content: String,
    /// Model selection (Pillar 1). `None` = provider default.
    #[serde(default)]
    pub model: Option<String>,
    /// Neutral permission posture for this turn.
    pub permission: LaunchPermission,
    /// Paths the turn may write (basis for `workspaceWrite` / `--add-dir`).
    #[serde(default)]
    pub writable_roots: Vec<String>,
    /// Abstract allowed-tool set; empty = adapter default.
    #[serde(default)]
    pub tools: Vec<String>,
    /// cwd / worktree root the turn runs in.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Neutral MCP block (PROPOSED, Pillar 2). `None` = no MCP attachment.
    #[serde(default)]
    pub mcp: Option<LaunchMcp>,
    /// Skills to inject (Pillar 1 skill contract); skill `<id>` refs.
    #[serde(default)]
    pub skill_refs: Vec<String>,
    /// Resume an existing provider session (Codex `--session`, Claude
    /// `--resume`); `None` = a fresh session.
    #[serde(default)]
    pub resume: Option<String>,
    /// The event-stream output contract the adapter should request so its native
    /// output normalizes into [`AgentEvent`] (Codex `--json`, Claude
    /// `--output-format stream-json`). Free string, neutral.
    #[serde(default)]
    pub output: Option<String>,
}

/// Provider-neutral delivery handle: how the harness reaches a member's runtime
/// for a delivery.
///
/// This generalizes `control_endpoint` (a raw `unix://socket`) into a
/// process/session descriptor, per ADR 0018 ("Generalize the `control_endpoint`
/// … neither provider needs a long-lived socket in the target design"). It is
/// **additive and pass-through only** in this work package: it does not remove
/// `control_endpoint` and does not change delivery behavior. The existing
/// `socket_path_from_endpoint` resolution stays where it is; this handle simply
/// preserves the raw endpoint string verbatim so callers that still inspect it
/// keep working while the exec-stream path is built in later work packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryHandle {
    /// The raw control endpoint as stored on the member / runtime (e.g.
    /// `unix://…/codex.sock`, or a future exec/session descriptor). Preserved
    /// verbatim; no scheme is assumed.
    pub endpoint: String,
}

impl DeliveryHandle {
    /// Construct a handle that passes the endpoint through unchanged.
    pub fn from_endpoint(endpoint: impl Into<String>) -> Self {
        DeliveryHandle {
            endpoint: endpoint.into(),
        }
    }

    /// The raw endpoint string, verbatim.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// Map a member's existing (Codex-flavored) `sandbox_policy` onto the neutral
/// [`LaunchPermission`] enum.
///
/// Accepts both the dashed and camelCase spellings that the CLI provider layer
/// already tolerates (`read-only`/`readOnly`, `workspace-write`/`workspaceWrite`,
/// `danger-full-access`/`dangerFullAccess`). An absent or unrecognized policy
/// falls back to the safe [`LaunchPermission::default`] posture, so a member that
/// never declared one is not silently elevated.
fn permission_from_sandbox_policy(policy: Option<&str>) -> LaunchPermission {
    match policy {
        Some("read-only") | Some("readOnly") => LaunchPermission::ReadOnly,
        Some("workspace-write") | Some("workspaceWrite") => LaunchPermission::WorkspaceWrite,
        Some("danger-full-access") | Some("dangerFullAccess") => LaunchPermission::FullAccess,
        _ => LaunchPermission::default(),
    }
}

/// Compose the neutral turn-input envelope for a claimed [`Message`].
///
/// This mirrors the harness message-envelope shape the CLI provider layer
/// already hands to a turn (message id / kind / task / routing + content) but
/// keeps it provider-neutral text: the adapter decides how to deliver it (Codex
/// `input` item, Claude `-p`, …).
fn compose_message_content(message: &Message) -> String {
    format!(
        "Harness message envelope:\nmessage_id: {}\nkind: {}\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: {}\ncontent:\n{}",
        message.id,
        message_kind_wire(&message.kind),
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.channel.as_deref().unwrap_or("-"),
        message.content
    )
}

fn message_kind_wire(kind: &MessageKind) -> &'static str {
    match kind {
        MessageKind::Message => "message",
        MessageKind::Task => "task",
        MessageKind::Report => "report",
    }
}

/// Build the provider-neutral [`LaunchSpec`] for one turn from a member and the
/// claimed [`Message`].
///
/// This is the additive composition seam (ADR 0018 WP-1). It reads the existing
/// `AgentMember` / `AgentProviderConfig` fields — including the Codex-flavored
/// `sandbox_policy` — and produces a neutral spec: the permission posture and
/// `writable_roots` are abstracted out of the Codex `workspaceWrite` vocabulary,
/// and no Codex wire names appear on the result (ADR 0011). It does not perform
/// any delivery side effect and does not require a live provider binary.
pub fn build_launch_spec(member: &AgentMember, message: &Message) -> LaunchSpec {
    let permission =
        permission_from_sandbox_policy(member.provider_config.sandbox_policy.as_deref());

    // Writable roots are member-level then provider_config-level roots, in that
    // order, de-duplicated. They are only meaningful when the turn may write, so
    // a read-only posture carries no writable roots.
    let writable_roots = if matches!(permission, LaunchPermission::ReadOnly) {
        Vec::new()
    } else {
        let mut roots: Vec<String> = Vec::new();
        for root in member
            .runtime_workspace_roots
            .iter()
            .chain(member.provider_config.runtime_workspace_roots.iter())
        {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }
        roots
    };

    LaunchSpec {
        prompt_ref: member.prompt_ref.clone(),
        message_content: compose_message_content(message),
        model: member.model.clone(),
        permission,
        writable_roots,
        // The abstract allowed-tool set is not yet sourced from a neutral member
        // field; left empty until the tool contract lands (Pillar 1/3). Adapters
        // apply their own default meanwhile.
        tools: Vec::new(),
        workspace: member.worktree_ref.clone(),
        // MCP from provider_config (Pillar 2); now available.
        mcp: member.provider_config.mcp.clone(),
        skill_refs: member.skill_refs.clone(),
        // Resume an existing provider session when the member already carries a
        // provider thread/session id from a prior delivery. This is what lets
        // memory carry across deliveries: the next turn is dispatched as a
        // resume of the same session (Codex `exec resume <id>`, Claude
        // `--resume <id>`) instead of a fresh session. `None` (no prior id) = a
        // fresh session.
        resume: member.provider_thread_id.clone(),
        output: None,
    }
}

/// Dispatch discriminant for the provider seam.
///
/// This is **not** a schema field: `AgentMember.provider` (and the other
/// `provider` fields across the model) remain free `String`s, serialized
/// verbatim and validated only as non-empty. `ProviderKind` exists purely so
/// the CLI provider layer can `match` on a member's provider when routing to
/// runtime spawn / delivery / probe / ingest, while keeping the core
/// provider-neutral per ADR 0011.
///
/// Any provider string the harness does not recognise round-trips through
/// [`ProviderKind::Unknown`] so fidelity is never lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Codex,
    Claude,
    Unknown(String),
}

impl ProviderKind {
    pub fn as_str(&self) -> &str {
        match self {
            ProviderKind::Codex => "codex",
            ProviderKind::Claude => "claude",
            ProviderKind::Unknown(value) => value,
        }
    }
}

impl From<&str> for ProviderKind {
    fn from(value: &str) -> Self {
        match value {
            "codex" => ProviderKind::Codex,
            "claude" => ProviderKind::Claude,
            other => ProviderKind::Unknown(other.to_string()),
        }
    }
}

impl From<String> for ProviderKind {
    fn from(value: String) -> Self {
        ProviderKind::from(value.as_str())
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTeamStatus {
    Active,
    Closed,
    Archived,
}

fn default_agent_team_status() -> AgentTeamStatus {
    AgentTeamStatus::Active
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTeam {
    pub id: String,
    pub name: String,
    pub description: String,
    pub owner_agent_id: String,
    #[serde(default = "default_agent_team_status")]
    pub status: AgentTeamStatus,
    pub member_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Planned,
    Assigned,
    Running,
    Blocked,
    Review,
    Done,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub goal_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub objective: String,
    pub owner_agent_id: String,
    pub assignee_agent_id: Option<String>,
    pub reviewer_agent_id: Option<String>,
    pub status: TaskStatus,
    pub depends_on_task_ids: Vec<String>,
    pub workspace_ref: Option<String>,
    pub branch_ref: Option<String>,
    pub pr_ref: Option<String>,
    pub owned_paths: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub scope_refs: Vec<String>,
    #[serde(default)]
    pub requires_human_approval: bool,
    #[serde(default)]
    pub verdict_decision_id: Option<String>,
    /// Full task write-up (markdown). `objective` stays the one-line summary.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub git_metadata: Option<GitMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    Task,
    Report,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDeliveryStatus {
    Queued,
    Delivered,
    Acknowledged,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSessionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Stale,
}

/// Identity class of a [`Message`] sender. Distinguishes harness-managed agents
/// from external operators (humans / external agents acting on their own behalf)
/// and system-emitted messages, so an operator-authored message is never
/// rendered as if it came from the Lead agent. Provider-neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SenderKind {
    Agent,
    Operator,
    System,
}

impl Default for SenderKind {
    fn default() -> Self {
        SenderKind::Agent
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageTerminalSource {
    TurnCompleted,
    ThreadIdle,
    ThreadRead,
    HookStop,
    DryRun,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDelivery {
    #[serde(default)]
    pub provider_session_id: Option<String>,
    #[serde(default)]
    pub provider_request_id: Option<String>,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub terminal_source: Option<MessageTerminalSource>,
    #[serde(default)]
    pub delivered_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSession {
    pub id: String,
    pub provider: String,
    pub agent_member_id: String,
    pub task_id: Option<String>,
    pub workspace_ref: Option<String>,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub terminal_source: Option<MessageTerminalSource>,
    pub status: ProviderSessionStatus,
    pub command: String,
    pub args: Vec<String>,
    pub prompt_ref: Option<String>,
    pub prompt_summary: Option<String>,
    pub provider_session_ref: Option<String>,
    pub stdout_ref: Option<String>,
    pub jsonl_ref: Option<String>,
    pub transcript_ref: Option<String>,
    pub last_message_ref: Option<String>,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AgentRuntimeHealth {
    #[serde(default)]
    pub process_alive: bool,
    #[serde(default)]
    pub socket_exists: bool,
    #[serde(default)]
    pub protocol_probe: Option<String>,
    #[serde(default)]
    pub delivery_probe: Option<String>,
    #[serde(default)]
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRuntime {
    pub id: String,
    pub agent_member_id: String,
    pub provider: String,
    pub status: AgentRuntimeStatus,
    pub pid: Option<u32>,
    pub control_endpoint: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub health: AgentRuntimeHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub id: String,
    pub agent_member_id: String,
    pub provider_runtime_id: Option<String>,
    pub task_id: Option<String>,
    pub provider: String,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub provider_child_thread_id: Option<String>,
    pub event_type: String,
    pub summary: String,
    pub payload_ref: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderChildThreadStatus {
    Open,
    Running,
    Completed,
    Interrupted,
    Errored,
    Closed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderChildThread {
    pub id: String,
    pub provider: String,
    pub agent_member_id: String,
    pub provider_runtime_id: Option<String>,
    pub task_id: Option<String>,
    pub parent_provider_thread_id: Option<String>,
    pub provider_thread_id: String,
    pub provider_agent_path: Option<String>,
    pub provider_agent_nickname: Option<String>,
    pub provider_agent_role: Option<String>,
    pub status: ProviderChildThreadStatus,
    pub last_message_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    Submitted,
    Accepted,
    Rejected,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub task_id: String,
    pub agent_member_id: String,
    pub title: String,
    pub summary: String,
    pub status: ProposalStatus,
    pub changed_paths: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub task_id: Option<String>,
    pub from_agent_id: String,
    pub to_agent_id: Option<String>,
    pub channel: Option<String>,
    pub kind: MessageKind,
    pub delivery_status: MessageDeliveryStatus,
    pub content: String,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub delivery: Option<MessageDelivery>,
    /// Identity class of the sender. Defaults to [`SenderKind::Agent`] so existing
    /// records (which omit the field) deserialize unchanged. When
    /// [`SenderKind::Operator`], `from_agent_id` uses the reserved `"operator"` id
    /// convention rather than a roster member id.
    #[serde(default)]
    pub sender_kind: SenderKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub task_id: Option<String>,
    pub source_type: String,
    pub source_ref: String,
    pub summary: String,
    pub created_at: String,
    #[serde(default)]
    pub evidence_kind: Option<String>,
    #[serde(default)]
    pub goal_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub task_id: String,
    pub decision: String,
    pub rationale: String,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub decision_kind: Option<String>,
    #[serde(default)]
    pub goal_id: Option<String>,
    #[serde(default)]
    pub is_waiver: bool,
    #[serde(default)]
    pub follow_up_task_id: Option<String>,
}

/// Verdict carried by a [`Review`]. Open enum: the canonical, harness-owned set
/// is modelled as named variants for type safety; any other value supplied by an
/// adapter or skill round-trips through [`ReviewVerdict::Other`].
///
/// `#[serde(other)]` only supports unit variants and would discard the original
/// string, so this uses `from`/`into` String conversions to preserve fidelity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum ReviewVerdict {
    Pass,
    Fail,
    Blocked,
    NeedsChanges,
    Other(String),
}

impl ReviewVerdict {
    pub fn as_str(&self) -> &str {
        match self {
            ReviewVerdict::Pass => "pass",
            ReviewVerdict::Fail => "fail",
            ReviewVerdict::Blocked => "blocked",
            ReviewVerdict::NeedsChanges => "needs_changes",
            ReviewVerdict::Other(value) => value,
        }
    }
}

impl From<String> for ReviewVerdict {
    fn from(value: String) -> Self {
        match value.as_str() {
            "pass" => ReviewVerdict::Pass,
            "fail" => ReviewVerdict::Fail,
            "blocked" => ReviewVerdict::Blocked,
            "needs_changes" => ReviewVerdict::NeedsChanges,
            _ => ReviewVerdict::Other(value),
        }
    }
}

impl From<ReviewVerdict> for String {
    fn from(value: ReviewVerdict) -> Self {
        value.as_str().to_string()
    }
}

/// First-class evaluator/critic output. Today an unstructured report Message; the
/// Review object captures verdict + findings + residual risk as structured data.
///
/// Concept-model invariant: a Review is *evidence for* a Decision, not the global
/// decision itself — a Lead/gate still issues the Decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub id: String,
    pub task_id: Option<String>,
    pub goal_id: Option<String>,
    pub reviewer_agent_id: String,
    pub review_kind: String,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub blockers: Vec<String>,
    pub residual_risk: Option<String>,
    pub missing_validation: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
}

/// Severity of a [`Gap`]. Truly-closed, harness-owned set (matches the GAP
/// ledger P0/P1/P2 convention), so it is a hard enum on both wire and schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapSeverity {
    P0,
    P1,
    P2,
}

/// Lifecycle status of a [`Gap`]. Unifies the GAP checkbox state and the bug
/// ledger state machine into one closed, harness-owned set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapStatus {
    Open,
    InProgress,
    Fixed,
    Blocked,
    Deferred,
    Wontfix,
}

/// A first-class Gap ledger entry, absorbing the bug ledger: a Bug is simply a
/// Gap with `category = "bug"` (plus the optional `repro_ref`/`closing_test_ref`).
///
/// `category` is an open enum (free string): the canonical generic dimensions are
/// ux/data/observability/parity/tooling/workflow/docs/bug/other, but an adapter may
/// keep a domain-flavored category here without a schema bump. `severity` and
/// `status` are closed harness-owned enums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub id: String,
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    pub category: String,
    pub severity: GapSeverity,
    pub status: GapStatus,
    pub summary: String,
    pub evidence_ids: Vec<String>,
    pub next_step: Option<String>,
    pub owner_agent_id: Option<String>,
    pub repro_ref: Option<String>,
    pub closing_test_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Executable thesis for a Goal: the generic subset of let-me-try's strategy
/// creation checklist. Graduates from `Evidence(source_type=goal_design)`; both
/// representations coexist (dual-read by `goal_id`, no backfill).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalDesign {
    pub id: String,
    pub goal_id: String,
    pub scenario_summary: String,
    pub non_goals: Vec<String>,
    pub risk_and_permission_boundaries: String,
    pub required_infra: Vec<String>,
    /// Team id or inline team description; nullable when not yet assigned.
    pub agent_team: Option<String>,
    /// Task ids forming the design's task graph.
    pub task_graph: Vec<String>,
    pub evidence_plan: Vec<String>,
    pub acceptance_gates: Vec<String>,
    pub created_at: String,
}

/// Outcome of a [`GoalEvaluation`]. Open enum: the canonical generic set is
/// success/partial/failed/blocked, but an adapter may supply another value that
/// round-trips through [`EvaluationOutcome::Other`] without a schema bump.
///
/// `#[serde(other)]` only supports unit variants and would discard the original
/// string, so this uses `from`/`into` String conversions to preserve fidelity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum EvaluationOutcome {
    Success,
    Partial,
    Failed,
    Blocked,
    Other(String),
}

impl EvaluationOutcome {
    pub fn as_str(&self) -> &str {
        match self {
            EvaluationOutcome::Success => "success",
            EvaluationOutcome::Partial => "partial",
            EvaluationOutcome::Failed => "failed",
            EvaluationOutcome::Blocked => "blocked",
            EvaluationOutcome::Other(value) => value,
        }
    }
}

impl From<String> for EvaluationOutcome {
    fn from(value: String) -> Self {
        match value.as_str() {
            "success" => EvaluationOutcome::Success,
            "partial" => EvaluationOutcome::Partial,
            "failed" => EvaluationOutcome::Failed,
            "blocked" => EvaluationOutcome::Blocked,
            _ => EvaluationOutcome::Other(value),
        }
    }
}

impl From<EvaluationOutcome> for String {
    fn from(value: EvaluationOutcome) -> Self {
        value.as_str().to_string()
    }
}

/// Retrospective for a Goal: what worked / failed, reusable patterns and
/// anti-patterns, and the follow-up / proposed-goal pointers that feed the next
/// round. Graduates from `Evidence(source_type=goal_evaluation)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalEvaluation {
    pub id: String,
    pub goal_id: String,
    pub evaluator_agent_id: String,
    pub outcome: EvaluationOutcome,
    pub what_worked: String,
    pub what_failed: String,
    pub missing_infra: Vec<String>,
    pub missing_evidence: Vec<String>,
    pub team_design_feedback: String,
    pub task_graph_feedback: String,
    pub dashboard_feedback: String,
    pub reusable_patterns: Vec<String>,
    pub anti_patterns: Vec<String>,
    pub follow_up_task_ids: Vec<String>,
    pub proposed_goal_ids: Vec<String>,
    pub created_at: String,
}

/// Reusable teaching artifact distilled from a completed Goal. The human-facing
/// files under `examples/goal-cases/<case-id>/` remain the artifact; this is the
/// optional structured manifest over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalCase {
    pub case_id: String,
    pub source_goal_id: String,
    pub scenario_type: String,
    pub project_adapter: Option<String>,
    pub goal_design_ref: Option<String>,
    pub evaluation_ref: Option<String>,
    pub reusable_patterns: Vec<String>,
    pub anti_patterns: Vec<String>,
    pub follow_up_refs: Vec<String>,
    pub tags: Vec<String>,
    pub created_at: String,
}

/// A durable product vision a Goal can be scheduled against. The autonomous
/// next-goal proposal compares a [`GoalEvaluation`] against the linked Vision;
/// there is no NextRoundPlan object (a proposed goal stays Goal+Task+Message+Decision).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vision {
    pub id: String,
    pub summary: String,
    /// PRD / design-basis doc paths backing the vision.
    pub source_refs: Vec<String>,
    pub created_at: String,
}

pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("{field} is required")]
    Required { field: &'static str },
}

fn require_non_empty(value: &str, field: &'static str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        Err(ValidationError::Required { field })
    } else {
        Ok(())
    }
}

impl Validate for AgentMember {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentMember.id")?;
        require_non_empty(&self.name, "AgentMember.name")?;
        require_non_empty(&self.description, "AgentMember.description")?;
        require_non_empty(&self.role, "AgentMember.role")?;
        require_non_empty(&self.provider, "AgentMember.provider")?;
        require_non_empty(&self.created_at, "AgentMember.created_at")
    }
}

impl Validate for AgentTeam {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentTeam.id")?;
        require_non_empty(&self.name, "AgentTeam.name")?;
        require_non_empty(&self.description, "AgentTeam.description")?;
        require_non_empty(&self.owner_agent_id, "AgentTeam.owner_agent_id")?;
        require_non_empty(&self.created_at, "AgentTeam.created_at")?;
        require_non_empty(&self.updated_at, "AgentTeam.updated_at")
    }
}

impl Validate for Goal {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Goal.id")?;
        require_non_empty(&self.title, "Goal.title")?;
        require_non_empty(&self.objective, "Goal.objective")?;
        require_non_empty(&self.owner_agent_id, "Goal.owner_agent_id")?;
        require_non_empty(&self.priority, "Goal.priority")?;
        require_non_empty(&self.created_at, "Goal.created_at")?;
        require_non_empty(&self.updated_at, "Goal.updated_at")
    }
}

impl Validate for Task {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Task.id")?;
        require_non_empty(&self.title, "Task.title")?;
        require_non_empty(&self.objective, "Task.objective")?;
        require_non_empty(&self.owner_agent_id, "Task.owner_agent_id")?;
        require_non_empty(&self.created_at, "Task.created_at")?;
        require_non_empty(&self.updated_at, "Task.updated_at")
    }
}

impl Validate for Message {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Message.id")?;
        require_non_empty(&self.from_agent_id, "Message.from_agent_id")?;
        require_non_empty(&self.content, "Message.content")?;
        require_non_empty(&self.created_at, "Message.created_at")
    }
}

impl Validate for ProviderSession {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "ProviderSession.id")?;
        require_non_empty(&self.provider, "ProviderSession.provider")?;
        require_non_empty(&self.agent_member_id, "ProviderSession.agent_member_id")?;
        require_non_empty(&self.command, "ProviderSession.command")?;
        require_non_empty(&self.started_at, "ProviderSession.started_at")
    }
}

impl Validate for AgentRuntime {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentRuntime.id")?;
        require_non_empty(&self.agent_member_id, "AgentRuntime.agent_member_id")?;
        require_non_empty(&self.provider, "AgentRuntime.provider")?;
        require_non_empty(&self.command, "AgentRuntime.command")?;
        require_non_empty(&self.started_at, "AgentRuntime.started_at")
    }
}

impl Validate for AgentEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentEvent.id")?;
        require_non_empty(&self.agent_member_id, "AgentEvent.agent_member_id")?;
        require_non_empty(&self.provider, "AgentEvent.provider")?;
        require_non_empty(&self.event_type, "AgentEvent.event_type")?;
        require_non_empty(&self.summary, "AgentEvent.summary")?;
        require_non_empty(&self.created_at, "AgentEvent.created_at")
    }
}

impl Validate for Proposal {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Proposal.id")?;
        require_non_empty(&self.task_id, "Proposal.task_id")?;
        require_non_empty(&self.agent_member_id, "Proposal.agent_member_id")?;
        require_non_empty(&self.title, "Proposal.title")?;
        require_non_empty(&self.summary, "Proposal.summary")?;
        require_non_empty(&self.created_at, "Proposal.created_at")?;
        require_non_empty(&self.updated_at, "Proposal.updated_at")
    }
}

impl Validate for Evidence {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Evidence.id")?;
        require_non_empty(&self.source_type, "Evidence.source_type")?;
        require_non_empty(&self.source_ref, "Evidence.source_ref")?;
        require_non_empty(&self.summary, "Evidence.summary")?;
        require_non_empty(&self.created_at, "Evidence.created_at")
    }
}

impl Validate for Decision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Decision.id")?;
        require_non_empty(&self.task_id, "Decision.task_id")?;
        require_non_empty(&self.decision, "Decision.decision")?;
        require_non_empty(&self.rationale, "Decision.rationale")?;
        require_non_empty(&self.created_at, "Decision.created_at")
    }
}

impl Validate for Review {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Review.id")?;
        require_non_empty(&self.reviewer_agent_id, "Review.reviewer_agent_id")?;
        require_non_empty(&self.review_kind, "Review.review_kind")?;
        require_non_empty(self.verdict.as_str(), "Review.verdict")?;
        require_non_empty(&self.summary, "Review.summary")?;
        require_non_empty(&self.created_at, "Review.created_at")
    }
}

impl Validate for Gap {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Gap.id")?;
        require_non_empty(&self.category, "Gap.category")?;
        require_non_empty(&self.summary, "Gap.summary")?;
        require_non_empty(&self.created_at, "Gap.created_at")?;
        require_non_empty(&self.updated_at, "Gap.updated_at")
    }
}

impl Validate for GoalDesign {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "GoalDesign.id")?;
        require_non_empty(&self.goal_id, "GoalDesign.goal_id")?;
        require_non_empty(&self.scenario_summary, "GoalDesign.scenario_summary")?;
        require_non_empty(
            &self.risk_and_permission_boundaries,
            "GoalDesign.risk_and_permission_boundaries",
        )?;
        require_non_empty(&self.created_at, "GoalDesign.created_at")
    }
}

impl Validate for GoalEvaluation {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "GoalEvaluation.id")?;
        require_non_empty(&self.goal_id, "GoalEvaluation.goal_id")?;
        require_non_empty(&self.evaluator_agent_id, "GoalEvaluation.evaluator_agent_id")?;
        require_non_empty(self.outcome.as_str(), "GoalEvaluation.outcome")?;
        require_non_empty(&self.what_worked, "GoalEvaluation.what_worked")?;
        require_non_empty(&self.what_failed, "GoalEvaluation.what_failed")?;
        require_non_empty(&self.created_at, "GoalEvaluation.created_at")
    }
}

impl Validate for GoalCase {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.case_id, "GoalCase.case_id")?;
        require_non_empty(&self.source_goal_id, "GoalCase.source_goal_id")?;
        require_non_empty(&self.scenario_type, "GoalCase.scenario_type")?;
        require_non_empty(&self.created_at, "GoalCase.created_at")
    }
}

impl Validate for Vision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Vision.id")?;
        require_non_empty(&self.summary, "Vision.summary")?;
        require_non_empty(&self.created_at, "Vision.created_at")
    }
}

// ---------------------------------------------------------------------------
// Dynamic workflow runtime objects (WP1)
//
// Additive, ADR-0017-style: a `WorkflowRun` is a standalone object with its own
// id and lifecycle. It is NOT bound to a `Goal`/`Task` yet (design decision 2 in
// docs/research/dynamic-workflow-runtime-design.md). Each `WorkflowStep` is the
// workflow-layer wrapper around one `agent()` call; it references the
// `ProviderSession` that the delivery produced rather than re-recording the
// execution. Both journal to their own append-only JSONL with latest-wins
// projection, exactly like every other harness object.
// ---------------------------------------------------------------------------

/// Lifecycle of a [`WorkflowRun`]. WP1 only exercises Running -> Completed and
/// Running -> Failed; Pending/Paused are reserved for the scheduler/resume work
/// packages (WP2/WP4) so existing rows remain forward-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

/// Status of a single [`WorkflowStep`] (one `agent()` call). WP1 uses
/// Running -> Completed / Failed. Queued/Cached are reserved for the
/// scheduler/resume work packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cached,
}

/// One run of a built-in (registered) workflow. The `workflow_name` selects the
/// registered Rust fn (option C in the design). `step_ids` orders the steps in
/// the sequence they were started, so the journal alone reconstructs the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: String,
    pub workflow_name: String,
    pub status: WorkflowRunStatus,
    #[serde(default)]
    pub step_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    /// Optional human-facing summary set when the run reaches a terminal state.
    #[serde(default)]
    pub summary: Option<String>,
}

/// One agent step inside a [`WorkflowRun`]. `phase` is the declarative grouping
/// marker (e.g. "audit", "synthesize"); `label` names the step within the phase.
/// `provider_session_id` links to the [`ProviderSession`] the delivery produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub run_id: String,
    pub phase: String,
    pub label: String,
    #[serde(default)]
    pub provider_session_id: Option<String>,
    pub status: WorkflowStepStatus,
    #[serde(default)]
    pub output_summary: Option<String>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
}

impl Validate for WorkflowRun {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowRun.id")?;
        require_non_empty(&self.workflow_name, "WorkflowRun.workflow_name")?;
        require_non_empty(&self.created_at, "WorkflowRun.created_at")
    }
}

impl Validate for WorkflowStep {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowStep.id")?;
        require_non_empty(&self.run_id, "WorkflowStep.run_id")?;
        require_non_empty(&self.label, "WorkflowStep.label")?;
        require_non_empty(&self.started_at, "WorkflowStep.started_at")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_round_trips_via_str() {
        for (input, expected) in [
            ("codex", ProviderKind::Codex),
            ("claude", ProviderKind::Claude),
        ] {
            let kind = ProviderKind::from(input);
            assert_eq!(kind, expected);
            // Display must reproduce the original provider string verbatim.
            assert_eq!(kind.to_string(), input);
            assert_eq!(kind.as_str(), input);
        }
    }

    #[test]
    fn provider_kind_unknown_preserves_value() {
        let kind = ProviderKind::from("gemini");
        assert_eq!(kind, ProviderKind::Unknown("gemini".to_string()));
        // Unknown providers round-trip without losing fidelity.
        assert_eq!(kind.to_string(), "gemini");
        assert_eq!(ProviderKind::from("gemini".to_string()), kind);
    }

    #[test]
    fn task_round_trips_json() {
        let task = Task {
            id: "task-1".to_string(),
            goal_id: Some("goal-1".to_string()),
            parent_task_id: None,
            title: "Inspect issue".to_string(),
            objective: "Find the root cause".to_string(),
            owner_agent_id: "leader-1".to_string(),
            assignee_agent_id: Some("agent-1".to_string()),
            reviewer_agent_id: Some("reviewer-1".to_string()),
            status: TaskStatus::Assigned,
            depends_on_task_ids: vec!["task-0".to_string()],
            workspace_ref: Some("../worktrees/task-1".to_string()),
            branch_ref: Some("agent/task-1".to_string()),
            pr_ref: Some("https://github.com/cyl19970726/multi-agent-harness/pull/1".to_string()),
            owned_paths: vec!["crates/harness-core".to_string()],
            acceptance_criteria: vec!["Evidence is attached".to_string()],
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T00:00:00Z".to_string(),
            phase: Some("design".to_string()),
            scope_refs: vec!["scope-1".to_string()],
            requires_human_approval: true,
            verdict_decision_id: Some("decision-1".to_string()),
            description: Some("Trace the failing assertion to its root cause.".to_string()),
            git_metadata: Some(GitMetadata {
                branch: Some("agent/task-1".to_string()),
                base_branch: Some("master".to_string()),
                owned_paths: vec!["crates/harness-core".to_string()],
                ..Default::default()
            }),
        };

        let json = serde_json::to_string(&task).expect("serialize task");
        let parsed: Task = serde_json::from_str(&json).expect("deserialize task");

        assert_eq!(parsed, task);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn goal_round_trips_json() {
        let goal = Goal {
            id: "goal-1".to_string(),
            title: "Self-host MVP".to_string(),
            objective: "Use the harness to manage its own development".to_string(),
            owner_agent_id: "leader-1".to_string(),
            status: GoalStatus::Active,
            success_criteria: vec!["Self-hosting task graph is visible".to_string()],
            priority: "p0".to_string(),
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T00:00:00Z".to_string(),
            vision_id: Some("vision-1".to_string()),
            goal_design_id: Some("goal-design-1".to_string()),
            closed_by_decision_id: Some("decision-1".to_string()),
            git_metadata: Some(GitMetadata {
                repo: Some("multi-agent-harness".to_string()),
                branch: Some("feature/self-host".to_string()),
                base_branch: Some("master".to_string()),
                ..Default::default()
            }),
        };

        let json = serde_json::to_string(&goal).expect("serialize goal");
        let parsed: Goal = serde_json::from_str(&json).expect("deserialize goal");

        assert_eq!(parsed, goal);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn review_round_trips_json() {
        let review = Review {
            id: "review-1".to_string(),
            task_id: Some("task-1".to_string()),
            goal_id: Some("goal-1".to_string()),
            reviewer_agent_id: "evaluator-1".to_string(),
            review_kind: "acceptance".to_string(),
            verdict: ReviewVerdict::Pass,
            summary: "Acceptance gates met; evidence backs the verdict.".to_string(),
            blockers: vec![],
            residual_risk: Some("Snapshot regeneration not yet automated.".to_string()),
            missing_validation: vec!["load test deferred".to_string()],
            evidence_ids: vec!["evidence-1".to_string()],
            created_at: "2026-05-26T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&review).expect("serialize review");
        let parsed: Review = serde_json::from_str(&json).expect("deserialize review");

        assert_eq!(parsed, review);
        assert!(parsed.validate().is_ok());
        // Canonical verdict serializes to its snake_case wire value.
        assert!(json.contains("\"verdict\":\"pass\""));
    }

    #[test]
    fn review_verdict_open_enum_round_trips_unknown_value() {
        // An adapter-supplied verdict that is not in the canonical set must
        // round-trip through ReviewVerdict::Other without losing the string.
        let review = Review {
            id: "review-2".to_string(),
            task_id: None,
            goal_id: Some("goal-1".to_string()),
            reviewer_agent_id: "critic-1".to_string(),
            review_kind: "safety".to_string(),
            verdict: ReviewVerdict::Other("conditional_pass".to_string()),
            summary: "Goal-level review with adapter verdict.".to_string(),
            blockers: vec!["needs second safety sign-off".to_string()],
            residual_risk: None,
            missing_validation: vec![],
            evidence_ids: vec![],
            created_at: "2026-05-26T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&review).expect("serialize review");
        assert!(json.contains("\"verdict\":\"conditional_pass\""));

        let parsed: Review = serde_json::from_str(&json).expect("deserialize review");
        assert_eq!(parsed.verdict, ReviewVerdict::Other("conditional_pass".to_string()));
        assert_eq!(parsed, review);
        assert!(parsed.validate().is_ok());

        // A canonical value deserialized from the wire collapses to its named variant.
        let canonical: Review =
            serde_json::from_str(&json.replace("conditional_pass", "needs_changes"))
                .expect("deserialize canonical verdict");
        assert_eq!(canonical.verdict, ReviewVerdict::NeedsChanges);
    }

    #[test]
    fn gap_round_trips_json() {
        let gap = Gap {
            id: "gap-1".to_string(),
            goal_id: Some("goal-1".to_string()),
            task_id: None,
            category: "observability".to_string(),
            severity: GapSeverity::P1,
            status: GapStatus::Open,
            summary: "Dashboard does not surface open reviews per task.".to_string(),
            evidence_ids: vec!["evidence-1".to_string()],
            next_step: Some("Wire reviewsByTask into the task surface.".to_string()),
            owner_agent_id: Some("worker-1".to_string()),
            repro_ref: None,
            closing_test_ref: None,
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&gap).expect("serialize gap");
        let parsed: Gap = serde_json::from_str(&json).expect("deserialize gap");

        assert_eq!(parsed, gap);
        assert!(parsed.validate().is_ok());
        // Closed severity/status enums serialize to their snake_case wire values.
        assert!(json.contains("\"severity\":\"p1\""));
        assert!(json.contains("\"status\":\"open\""));
    }

    #[test]
    fn gap_bug_round_trips_with_bug_fields() {
        // A Bug is a Gap with category="bug" carrying the optional repro/closing-test
        // refs; no separate Bug object exists.
        let bug = Gap {
            id: "gap-bug-1".to_string(),
            goal_id: None,
            task_id: Some("task-1".to_string()),
            category: "bug".to_string(),
            severity: GapSeverity::P0,
            status: GapStatus::InProgress,
            summary: "Snapshot serialization drops the new gaps key.".to_string(),
            evidence_ids: vec![],
            next_step: None,
            owner_agent_id: Some("worker-2".to_string()),
            repro_ref: Some("artifacts/repro-1.log".to_string()),
            closing_test_ref: Some("crates/harness-cli/src/main.rs::snapshot_test".to_string()),
            created_at: "2026-05-26T00:00:00Z".to_string(),
            updated_at: "2026-05-26T01:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&bug).expect("serialize bug gap");
        let parsed: Gap = serde_json::from_str(&json).expect("deserialize bug gap");

        assert_eq!(parsed, bug);
        assert!(parsed.validate().is_ok());
        assert!(json.contains("\"status\":\"in_progress\""));
        assert_eq!(parsed.severity, GapSeverity::P0);
    }

    #[test]
    fn goal_design_round_trips_json() {
        let design = GoalDesign {
            id: "goal-design-1".to_string(),
            goal_id: "goal-1".to_string(),
            scenario_summary: "Render the learning layer on the dashboard.".to_string(),
            non_goals: vec!["No backfill of legacy Evidence rows.".to_string()],
            risk_and_permission_boundaries: "Read-only snapshot; no auto-merge.".to_string(),
            required_infra: vec!["harness-store goal_designs.jsonl".to_string()],
            agent_team: Some("team-1".to_string()),
            task_graph: vec!["task-1".to_string(), "task-2".to_string()],
            evidence_plan: vec!["screenshot of GoalDocument".to_string()],
            acceptance_gates: vec!["cargo test green".to_string(), "pnpm check green".to_string()],
            created_at: "2026-05-30T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&design).expect("serialize goal design");
        let parsed: GoalDesign = serde_json::from_str(&json).expect("deserialize goal design");

        assert_eq!(parsed, design);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn goal_evaluation_round_trips_json() {
        let evaluation = GoalEvaluation {
            id: "goal-eval-1".to_string(),
            goal_id: "goal-1".to_string(),
            evaluator_agent_id: "evaluator-1".to_string(),
            outcome: EvaluationOutcome::Success,
            what_worked: "Dual-read union surfaced both objects and legacy evidence.".to_string(),
            what_failed: "Demo snapshot lagged the new keys until late.".to_string(),
            missing_infra: vec![],
            missing_evidence: vec!["load test".to_string()],
            team_design_feedback: "Solo WP was sufficient.".to_string(),
            task_graph_feedback: "Linear graph held.".to_string(),
            dashboard_feedback: "GoalDocument now shows real sections.".to_string(),
            reusable_patterns: vec!["additive-optional fields".to_string()],
            anti_patterns: vec!["required new fields on existing objects".to_string()],
            follow_up_task_ids: vec!["task-3".to_string()],
            proposed_goal_ids: vec!["goal-2".to_string()],
            created_at: "2026-05-30T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&evaluation).expect("serialize goal evaluation");
        let parsed: GoalEvaluation =
            serde_json::from_str(&json).expect("deserialize goal evaluation");

        assert_eq!(parsed, evaluation);
        assert!(parsed.validate().is_ok());
        assert!(json.contains("\"outcome\":\"success\""));
    }

    #[test]
    fn evaluation_outcome_open_enum_round_trips_unknown_value() {
        // An adapter-supplied outcome that is not in the canonical set must
        // round-trip through EvaluationOutcome::Other without losing the string.
        let outcome = EvaluationOutcome::Other("partially_blocked".to_string());
        let json = serde_json::to_string(&outcome).expect("serialize outcome");
        assert_eq!(json, "\"partially_blocked\"");

        let parsed: EvaluationOutcome = serde_json::from_str(&json).expect("deserialize outcome");
        assert_eq!(parsed, EvaluationOutcome::Other("partially_blocked".to_string()));

        // A canonical value deserialized from the wire collapses to its named variant.
        let canonical: EvaluationOutcome =
            serde_json::from_str("\"partial\"").expect("deserialize canonical outcome");
        assert_eq!(canonical, EvaluationOutcome::Partial);
    }

    #[test]
    fn goal_case_round_trips_json() {
        let case = GoalCase {
            case_id: "goal-case-1".to_string(),
            source_goal_id: "goal-1".to_string(),
            scenario_type: "dashboard-rendering".to_string(),
            project_adapter: None,
            goal_design_ref: Some("goal-design-1".to_string()),
            evaluation_ref: Some("goal-eval-1".to_string()),
            reusable_patterns: vec!["additive-optional fields".to_string()],
            anti_patterns: vec![],
            follow_up_refs: vec!["task-3".to_string()],
            tags: vec!["learning-layer".to_string()],
            created_at: "2026-05-30T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&case).expect("serialize goal case");
        let parsed: GoalCase = serde_json::from_str(&json).expect("deserialize goal case");

        assert_eq!(parsed, case);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn vision_round_trips_json() {
        let vision = Vision {
            id: "vision-1".to_string(),
            summary: "Generic harness object-model with a closed learning loop.".to_string(),
            source_refs: vec!["docs/goal-learning-loop.md".to_string()],
            created_at: "2026-05-30T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&vision).expect("serialize vision");
        let parsed: Vision = serde_json::from_str(&json).expect("deserialize vision");

        assert_eq!(parsed, vision);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn provider_session_round_trips_json() {
        let session = ProviderSession {
            id: "session-1".to_string(),
            provider: "codex".to_string(),
            agent_member_id: "agent-1".to_string(),
            task_id: Some("task-1".to_string()),
            workspace_ref: Some("../worktrees/task-1".to_string()),
            provider_thread_id: Some("thread-1".to_string()),
            provider_turn_id: Some("turn-1".to_string()),
            terminal_source: Some(MessageTerminalSource::TurnCompleted),
            status: ProviderSessionStatus::Succeeded,
            command: "codex".to_string(),
            args: vec!["exec".to_string()],
            prompt_ref: Some(".harness/prompts/task-1.md".to_string()),
            prompt_summary: Some("Implement task-1".to_string()),
            provider_session_ref: None,
            stdout_ref: Some(".harness/provider-sessions/session-1/stdout.log".to_string()),
            jsonl_ref: Some(".harness/provider-sessions/session-1/events.jsonl".to_string()),
            transcript_ref: None,
            last_message_ref: Some(
                ".harness/provider-sessions/session-1/last-message.md".to_string(),
            ),
            exit_code: Some(0),
            started_at: "2026-05-26T00:00:00Z".to_string(),
            ended_at: Some("2026-05-26T00:05:00Z".to_string()),
            evidence_ids: vec!["evidence-1".to_string()],
        };

        let json = serde_json::to_string(&session).expect("serialize provider session");
        let parsed: ProviderSession =
            serde_json::from_str(&json).expect("deserialize provider session");

        assert_eq!(parsed, session);
        assert!(parsed.validate().is_ok());
    }

    #[test]
    fn validation_rejects_missing_required_id() {
        let member = AgentMember {
            id: "".to_string(),
            name: "Leader".to_string(),
            description: "Lead agent".to_string(),
            role: "leader".to_string(),
            provider: "codex".to_string(),
            model: None,
            profile: None,
            provider_config: AgentProviderConfig::default(),
            capabilities: vec![],
            team_ids: vec![],
            prompt_ref: None,
            skill_refs: vec![],
            workspace_policy: None,
            worktree_ref: None,
            permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            status: AgentMemberStatus::Idle,
            current_task_id: None,
            current_proposal_id: None,
            provider_runtime_id: None,
            provider_thread_id: None,
            provider_agent_path: None,
            provider_agent_nickname: None,
            provider_agent_role: None,
            control_endpoint: None,
            created_at: "2026-05-26T00:00:00Z".to_string(),
            last_seen_at: None,
        };

        assert_eq!(
            member.validate(),
            Err(ValidationError::Required {
                field: "AgentMember.id"
            })
        );
    }

    #[test]
    fn message_sender_kind_defaults_to_agent_and_persists_operator() {
        // A record persisted before sender_kind existed omits the field entirely.
        // It must deserialize as SenderKind::Agent (additive-optional backfill).
        let legacy_json = r#"{
            "id": "msg-legacy",
            "task_id": null,
            "from_agent_id": "leader-1",
            "to_agent_id": "agent-1",
            "channel": null,
            "kind": "message",
            "delivery_status": "queued",
            "content": "hello",
            "evidence_ids": [],
            "created_at": "2026-05-26T00:00:00Z",
            "delivery": null
        }"#;
        let legacy: Message =
            serde_json::from_str(legacy_json).expect("deserialize legacy message");
        assert_eq!(legacy.sender_kind, SenderKind::Agent);
        assert!(legacy.validate().is_ok());

        // An operator-authored message uses the reserved "operator" from id and
        // round-trips its sender_kind without loss.
        let operator = Message {
            id: "msg-op".to_string(),
            task_id: None,
            from_agent_id: "operator".to_string(),
            to_agent_id: Some("agent-1".to_string()),
            channel: None,
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "do the thing".to_string(),
            evidence_ids: vec![],
            created_at: "2026-05-26T00:00:00Z".to_string(),
            delivery: None,
            sender_kind: SenderKind::Operator,
        };
        let json = serde_json::to_string(&operator).expect("serialize operator message");
        assert!(
            json.contains("\"sender_kind\":\"operator\""),
            "operator message must serialize sender_kind as snake_case: {json}"
        );
        let parsed: Message = serde_json::from_str(&json).expect("deserialize operator message");
        assert_eq!(parsed, operator);
        assert_eq!(parsed.sender_kind, SenderKind::Operator);
        assert!(parsed.validate().is_ok());
    }

    fn sample_member() -> AgentMember {
        AgentMember {
            id: "agent-1".to_string(),
            name: "Worker".to_string(),
            description: "A worker member".to_string(),
            role: "worker".to_string(),
            provider: "codex".to_string(),
            model: Some("o3".to_string()),
            profile: None,
            provider_config: AgentProviderConfig::default(),
            capabilities: vec!["code".to_string()],
            team_ids: vec![],
            prompt_ref: Some(".harness/prompts/worker.md".to_string()),
            skill_refs: vec!["harness-workflow".to_string()],
            workspace_policy: None,
            worktree_ref: Some("../worktrees/task-1".to_string()),
            permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            status: AgentMemberStatus::Idle,
            current_task_id: None,
            current_proposal_id: None,
            provider_runtime_id: None,
            provider_thread_id: None,
            provider_agent_path: None,
            provider_agent_nickname: None,
            provider_agent_role: None,
            control_endpoint: None,
            created_at: "2026-05-26T00:00:00Z".to_string(),
            last_seen_at: None,
        }
    }

    fn sample_message() -> Message {
        Message {
            id: "msg-1".to_string(),
            task_id: Some("task-1".to_string()),
            from_agent_id: "leader-1".to_string(),
            to_agent_id: Some("agent-1".to_string()),
            channel: Some("team".to_string()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Implement the launch spec.".to_string(),
            evidence_ids: vec![],
            created_at: "2026-05-26T00:00:00Z".to_string(),
            delivery: None,
            sender_kind: SenderKind::Agent,
        }
    }

    #[test]
    fn launch_spec_composes_from_member_and_message() {
        let mut member = sample_member();
        member.provider_config.sandbox_policy = Some("workspace-write".to_string());
        member.runtime_workspace_roots = vec!["crates/harness-core".to_string()];
        member.provider_config.runtime_workspace_roots = vec!["crates/harness-cli".to_string()];
        let message = sample_message();

        let spec = build_launch_spec(&member, &message);

        // Pillar 1 base configuration flows through unchanged.
        assert_eq!(spec.prompt_ref.as_deref(), Some(".harness/prompts/worker.md"));
        assert_eq!(spec.model.as_deref(), Some("o3"));
        assert_eq!(spec.skill_refs, vec!["harness-workflow".to_string()]);
        // Pillar 2 workspace flows through as the cwd / worktree root.
        assert_eq!(spec.workspace.as_deref(), Some("../worktrees/task-1"));
        // The turn input carries the message envelope + content.
        assert!(spec.message_content.contains("message_id: msg-1"));
        assert!(spec.message_content.contains("kind: task"));
        assert!(spec.message_content.contains("task_id: task-1"));
        assert!(spec.message_content.contains("Implement the launch spec."));
        // Fields with no neutral source yet are empty/none, not invented.
        assert!(spec.tools.is_empty());
        assert!(spec.mcp.is_none());
        // A fresh member (no prior provider thread/session) carries no resume token.
        assert!(spec.resume.is_none());
        assert!(spec.output.is_none());
    }

    #[test]
    fn launch_spec_carries_resume_from_member_provider_thread_id() {
        // A member that already has a provider thread/session id (from a prior
        // delivery) must produce a spec that resumes that session, so memory
        // carries across deliveries instead of starting fresh each turn.
        let mut member = sample_member();
        member.provider_thread_id = Some("thread-abc-123".to_string());
        let message = sample_message();

        let spec = build_launch_spec(&member, &message);

        assert_eq!(spec.resume.as_deref(), Some("thread-abc-123"));
    }

    #[test]
    fn launch_spec_maps_codex_sandbox_vocabulary_onto_neutral_permission() {
        // Each Codex sandbox spelling (dashed and camelCase) maps onto the neutral
        // permission enum; no Codex wire vocabulary survives onto the spec.
        let cases = [
            ("read-only", LaunchPermission::ReadOnly),
            ("readOnly", LaunchPermission::ReadOnly),
            ("workspace-write", LaunchPermission::WorkspaceWrite),
            ("workspaceWrite", LaunchPermission::WorkspaceWrite),
            ("danger-full-access", LaunchPermission::FullAccess),
            ("dangerFullAccess", LaunchPermission::FullAccess),
        ];
        for (policy, expected) in cases {
            let mut member = sample_member();
            member.provider_config.sandbox_policy = Some(policy.to_string());
            let spec = build_launch_spec(&member, &sample_message());
            assert_eq!(spec.permission, expected, "policy {policy} should map to {expected:?}");
        }
    }

    #[test]
    fn launch_spec_writable_roots_dedupe_and_drop_on_read_only() {
        // workspace_write carries de-duplicated member + provider_config roots.
        let mut member = sample_member();
        member.provider_config.sandbox_policy = Some("workspaceWrite".to_string());
        member.runtime_workspace_roots = vec!["shared".to_string(), "a".to_string()];
        member.provider_config.runtime_workspace_roots =
            vec!["shared".to_string(), "b".to_string()];
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(
            spec.writable_roots,
            vec!["shared".to_string(), "a".to_string(), "b".to_string()],
            "writable roots must be member-then-config order, de-duplicated"
        );

        // read_only never carries writable roots even if the member declares them.
        member.provider_config.sandbox_policy = Some("read-only".to_string());
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(spec.permission, LaunchPermission::ReadOnly);
        assert!(
            spec.writable_roots.is_empty(),
            "a read-only turn must not carry writable roots"
        );
    }

    #[test]
    fn launch_spec_absent_sandbox_policy_falls_back_to_safe_default() {
        // A member that never declared a sandbox policy must not be silently
        // elevated; it falls back to the default posture.
        let member = sample_member();
        assert!(member.provider_config.sandbox_policy.is_none());
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(spec.permission, LaunchPermission::default());
    }

    #[test]
    fn launch_spec_round_trips_json() {
        let mut member = sample_member();
        member.provider_config.sandbox_policy = Some("workspaceWrite".to_string());
        member.runtime_workspace_roots = vec!["crates".to_string()];
        let spec = build_launch_spec(&member, &sample_message());

        let json = serde_json::to_string(&spec).expect("serialize launch spec");
        let parsed: LaunchSpec = serde_json::from_str(&json).expect("deserialize launch spec");
        assert_eq!(parsed, spec);
        // The neutral permission serializes to its snake_case wire spelling, not
        // the Codex `workspaceWrite` vocabulary it was mapped from.
        assert!(json.contains("\"permission\":\"workspace_write\""));
        assert!(!json.contains("workspaceWrite"));
    }

    #[test]
    fn launch_permission_wire_values_are_neutral() {
        assert_eq!(LaunchPermission::ReadOnly.as_str(), "read_only");
        assert_eq!(LaunchPermission::WorkspaceWrite.as_str(), "workspace_write");
        assert_eq!(LaunchPermission::FullAccess.as_str(), "full_access");
        // Round-trip each variant through serde to confirm the wire spelling.
        for variant in [
            LaunchPermission::ReadOnly,
            LaunchPermission::WorkspaceWrite,
            LaunchPermission::FullAccess,
        ] {
            let json = serde_json::to_string(&variant).expect("serialize permission");
            assert_eq!(json, format!("\"{}\"", variant.as_str()));
            let parsed: LaunchPermission =
                serde_json::from_str(&json).expect("deserialize permission");
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn delivery_handle_passes_endpoint_through_verbatim() {
        // The neutral delivery handle preserves any endpoint scheme verbatim; it
        // does not interpret or strip `unix://` (that stays in the CLI layer).
        for endpoint in [
            "unix:///tmp/agent/codex.sock",
            "exec://session/abc",
            "/tmp/plain/path",
        ] {
            let handle = DeliveryHandle::from_endpoint(endpoint);
            assert_eq!(handle.endpoint(), endpoint);
            let json = serde_json::to_string(&handle).expect("serialize handle");
            let parsed: DeliveryHandle =
                serde_json::from_str(&json).expect("deserialize handle");
            assert_eq!(parsed, handle);
            assert_eq!(parsed.endpoint(), endpoint);
        }
    }

    #[test]
    fn launch_mcp_block_round_trips_when_present() {
        // The MCP block is omitted by build_launch_spec today, but the neutral
        // shape must round-trip so later WPs can populate it.
        let mcp = LaunchMcp {
            servers: vec![LaunchMcpServer {
                id: "fs".to_string(),
                transport: Some("stdio".to_string()),
                command: vec!["mcp-fs".to_string(), "--root".to_string()],
                url: None,
                allowed_tools: vec!["read".to_string()],
            }],
        };
        let json = serde_json::to_string(&mcp).expect("serialize mcp");
        let parsed: LaunchMcp = serde_json::from_str(&json).expect("deserialize mcp");
        assert_eq!(parsed, mcp);
    }

    #[test]
    fn build_launch_spec_carries_mcp_from_provider_config() {
        let mut member = sample_member();
        member.provider_config.mcp = Some(LaunchMcp {
            servers: vec![LaunchMcpServer {
                id: "fs".to_string(),
                transport: Some("stdio".to_string()),
                command: vec!["mcp-fs".to_string()],
                url: None,
                allowed_tools: vec![],
            }],
        });
        let spec = build_launch_spec(&member, &sample_message());
        assert!(spec.mcp.is_some(), "launch spec should carry mcp from provider_config");
        let mcp = spec.mcp.as_ref().unwrap();
        assert_eq!(mcp.servers.len(), 1);
        assert_eq!(mcp.servers[0].id, "fs");
    }

    #[test]
    fn build_launch_spec_mcp_none_when_absent() {
        let member = sample_member();
        assert!(member.provider_config.mcp.is_none());
        let spec = build_launch_spec(&member, &sample_message());
        assert!(
            spec.mcp.is_none(),
            "launch spec mcp should be none when member has no mcp"
        );
    }

    #[test]
    fn build_launch_spec_mcp_round_trips_json() {
        let mut member = sample_member();
        member.provider_config.mcp = Some(LaunchMcp {
            servers: vec![LaunchMcpServer {
                id: "api".to_string(),
                transport: Some("http".to_string()),
                command: vec![],
                url: Some("http://localhost:3000".to_string()),
                allowed_tools: vec!["query".to_string()],
            }],
        });
        let spec = build_launch_spec(&member, &sample_message());
        let json = serde_json::to_string(&spec).expect("serialize spec");
        let parsed: LaunchSpec = serde_json::from_str(&json).expect("deserialize spec");
        assert_eq!(parsed.mcp, spec.mcp);
    }

    #[test]
    fn provider_capabilities_codex_matches_doc_table() {
        let cap = ProviderCapabilities::codex_exec();
        assert!(cap.streaming, "Codex exec has --json streaming");
        assert!(cap.resume, "Codex exec has --session resume");
        assert!(!cap.mid_turn_approval, "Codex exec has policy pre-approve, no mid-turn");
        assert!(cap.subagents, "Codex supports subagents");
        assert!(cap.mcp, "Codex exec has --config mcp_servers");
        assert!(!cap.hooks, "Codex exec has limited hooks");
    }

    #[test]
    fn provider_capabilities_claude_matches_doc_table() {
        let cap = ProviderCapabilities::claude_exec();
        assert!(cap.streaming, "Claude -p has --output-format stream-json");
        assert!(cap.resume, "Claude has --resume");
        assert!(!cap.mid_turn_approval, "Claude -p has no mid-turn approval");
        assert!(cap.subagents, "Claude supports subagents");
        assert!(cap.mcp, "Claude has --mcp-config");
        assert!(!cap.hooks, "Claude has no documented hooks");
    }

    #[test]
    fn provider_capabilities_round_trips_json() {
        let cap = ProviderCapabilities::codex_exec();
        let json = serde_json::to_string(&cap).expect("serialize capabilities");
        let parsed: ProviderCapabilities =
            serde_json::from_str(&json).expect("deserialize capabilities");
        assert_eq!(parsed, cap);
    }

    #[test]
    fn provider_capabilities_display_shows_enabled_features() {
        let cap = ProviderCapabilities::codex_exec();
        let display = cap.to_string();
        assert!(display.contains("streaming"));
        assert!(display.contains("resume"));
        assert!(display.contains("mcp"));
        assert!(display.contains("subagents"));
        assert!(!display.contains("mid_turn_approval"), "disabled features should not show");
    }

    #[test]
    fn supports_streaming_exec_check() {
        let mut cap = ProviderCapabilities::codex_exec();
        assert!(cap.supports_streaming_exec(), "streaming + no mid-turn should be ok");
        cap.mid_turn_approval = true;
        assert!(!cap.supports_streaming_exec(), "mid-turn approval blocks streaming exec");
    }
}

/// Skill reference resolution: maps skill_refs to SKILL.md content.
///
/// A skill is durable at `.agents/skills/<id>/SKILL.md`. This module provides
/// the contract for resolving and validating skill references (Pillar 1 skill
/// contract from docs/agent-integration-model.md).
pub mod skill_resolver {
    use std::path::PathBuf;

    /// Result of resolving a skill reference.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ResolvedSkill {
        /// The skill id (matches `.agents/skills/<id>/`)
        pub id: String,
        /// The absolute or relative path to SKILL.md
        pub path: PathBuf,
        /// The full content of SKILL.md (header + body)
        pub content: String,
    }

    /// Error type for skill resolution.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum SkillResolutionError {
        /// The skill reference does not resolve to an existing SKILL.md.
        SkillNotFound { skill_id: String, path: PathBuf },
        /// An IO error occurred while reading the skill file.
        IoError {
            skill_id: String,
            reason: String,
        },
    }

    impl std::fmt::Display for SkillResolutionError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                SkillResolutionError::SkillNotFound { skill_id, path } => {
                    write!(f, "skill '{}' not found at {}", skill_id, path.display())
                }
                SkillResolutionError::IoError { skill_id, reason } => {
                    write!(f, "failed to read skill '{}': {}", skill_id, reason)
                }
            }
        }
    }

    impl std::error::Error for SkillResolutionError {}

    /// Resolve a single skill reference using the given skills root directory.
    ///
    /// The contract: a skill_ref `<id>` resolves to `.agents/skills/<id>/SKILL.md`.
    /// If the file exists and is readable, returns the content and path.
    /// If not found or unreadable, returns SkillResolutionError.
    ///
    /// This function is synchronous and does not require a live provider binary.
    pub fn resolve_skill(
        skill_id: &str,
        skills_root: &std::path::Path,
    ) -> Result<ResolvedSkill, SkillResolutionError> {
        let skill_path = skills_root.join(skill_id).join("SKILL.md");
        let content = std::fs::read_to_string(&skill_path).map_err(|e| {
            SkillResolutionError::IoError {
                skill_id: skill_id.to_string(),
                reason: e.to_string(),
            }
        })?;
        Ok(ResolvedSkill {
            id: skill_id.to_string(),
            path: skill_path,
            content,
        })
    }

    /// Resolve all skill references at once using the given skills root directory.
    ///
    /// Returns a Vec of resolved skills in the order they appear in the input.
    /// If any skill fails to resolve, returns an error (fail-fast); the caller
    /// must decide whether to report it or continue.
    pub fn resolve_skills(
        skill_ids: &[String],
        skills_root: &std::path::Path,
    ) -> Result<Vec<ResolvedSkill>, SkillResolutionError> {
        let mut resolved = Vec::new();
        for id in skill_ids {
            resolved.push(resolve_skill(id, skills_root)?);
        }
        Ok(resolved)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn skill_resolution_error_displays_clearly() {
            let err = SkillResolutionError::SkillNotFound {
                skill_id: "my-skill".to_string(),
                path: PathBuf::from(".agents/skills/my-skill/SKILL.md"),
            };
            let msg = err.to_string();
            assert!(msg.contains("my-skill"));
            assert!(msg.contains(".agents/skills"));
        }

        #[test]
        fn skill_not_found_error() {
            let result = resolve_skill("nonexistent", PathBuf::from(".").as_path());
            assert!(result.is_err());
            match result {
                Err(SkillResolutionError::IoError { skill_id, .. }) => {
                    assert_eq!(skill_id, "nonexistent");
                }
                _ => panic!("expected IoError"),
            }
        }
    }
}

/// Provider capabilities declaration: what a platform can technically support.
///
/// This is distinct from member-level `AgentMember.capabilities` (intent: what
/// the member is *meant* to do). This declares what the *platform* can do
/// (streaming, resume, mid-turn approval, subagents, MCP, hooks).
///
/// See Pillar 3 and the capability declaration table in
/// docs/agent-integration-model.md for the current capability set per provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Platform supports incremental event stream during a turn.
    pub streaming: bool,
    /// Platform supports session resume (`--session`, `--resume`, etc).
    pub resume: bool,
    /// Platform supports mid-turn tool approval/denial (approve/reject before execution).
    pub mid_turn_approval: bool,
    /// Platform supports native child threads / subagents.
    pub subagents: bool,
    /// Platform supports MCP server attachment.
    pub mcp: bool,
    /// Platform supports lifecycle hooks.
    pub hooks: bool,
}

impl ProviderCapabilities {
    /// Codex exec capabilities per the capability declaration table in
    /// docs/agent-integration-model.md.
    pub fn codex_exec() -> Self {
        ProviderCapabilities {
            streaming: true,   // --json NDJSON
            resume: true,      // --session
            mid_turn_approval: false,  // policy pre-approve only
            subagents: true,   // observed in Codex
            mcp: true,         // --config mcp_servers.*
            hooks: false,      // limited in exec mode
        }
    }

    /// Claude exec capabilities per the capability declaration table.
    pub fn claude_exec() -> Self {
        ProviderCapabilities {
            streaming: true,   // --output-format stream-json
            resume: true,      // --resume
            mid_turn_approval: false,  // not documented for -p; Tier-3 only
            subagents: true,   // observed in Claude
            mcp: true,         // --mcp-config JSON
            hooks: false,      // not documented
        }
    }

    /// Check if all critical capabilities for basic streaming exec are present.
    pub fn supports_streaming_exec(&self) -> bool {
        self.streaming && !self.mid_turn_approval
    }
}

impl std::fmt::Display for ProviderCapabilities {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let features = [
            ("streaming", self.streaming),
            ("resume", self.resume),
            ("mid_turn_approval", self.mid_turn_approval),
            ("subagents", self.subagents),
            ("mcp", self.mcp),
            ("hooks", self.hooks),
        ];
        let enabled: Vec<&str> = features
            .iter()
            .filter_map(|(name, enabled)| if *enabled { Some(*name) } else { None })
            .collect();
        write!(f, "{{{}}}", enabled.join(", "))
    }
}
