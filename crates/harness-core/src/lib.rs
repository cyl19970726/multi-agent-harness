use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod company_os;
pub use company_os::*;

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
    pub effort: Option<String>,
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
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
    /// Reasoning effort (Pillar 1). `None` = provider default.
    #[serde(default)]
    pub effort: Option<String>,
    /// Optional structured-output schema to enforce natively when the provider
    /// supports it. `None` = no schema flag.
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,
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
        MessageKind::Assignment => "assignment",
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
        effort: member.provider_config.effort.clone(),
        output_schema: member.provider_config.output_schema.clone(),
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

/// Rough public list price ($ per 1M tokens) `(input, output)` per provider — an
/// ESTIMATE used only to bound workflow spend when the provider reports no dollar
/// cost. The single source of truth for provider pricing across the harness.
/// Unknown providers fall back to the codex/gpt-5-class rate to preserve behavior.
pub fn provider_price_per_mtok(provider: &str) -> (f64, f64) {
    match provider {
        "claude" => (3.0, 15.0),
        // PLACEHOLDER pricing for Kimi (goal-provider-neutral S4). Moonshot's
        // published `kimi-for-coding`/`kimi-k2` list price is well BELOW the
        // codex/gpt-5-class default, so estimating Kimi at the gpt-5 rate would
        // wildly over-bound spend. These numbers are a conservative documented
        // guess — the real $/Mtok MUST be confirmed against Moonshot's pricing
        // page (or the live `kimi` CLI usage frame) before any spend decision is
        // trusted; see the goal's S3 spike. Until then this only bounds the
        // workflow token-estimate, never bills.
        "kimi" => (0.60, 2.50),
        _ => (1.25, 10.0), // codex / gpt-5-class default
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
pub enum MessageKind {
    Message,
    Assignment,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SenderKind {
    #[default]
    Agent,
    Operator,
    System,
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

// ---------------------------------------------------------------------------
// Multi-project identity (goal-multi-project, Stage 0 — pure layer, no I/O).
//
// A project's STORE (the JSONL ledgers + provider-sessions) is centralized under
// `~/.harness/projects/<id>/`, but its PROJECT ROOT (the git repo / dir where a
// worker runs and reads CLAUDE.md / AGENTS.md / memory) stays where it is. These
// two roots are deliberately distinct — see `ProjectContext`.
// ---------------------------------------------------------------------------

/// Reserved project id for the GLOBAL project, rooted at `$HOME` itself. Its
/// relative path is empty, so it cannot share the slug space — hence a reserved id.
pub const GLOBAL_PROJECT_ID: &str = "_global";

/// Whether a project is a specific repo/dir or the reserved global (`$HOME`) one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Repo,
    Global,
}

/// A resolved project. `project_root` is where workers run (and CLAUDE.md /
/// AGENTS.md / memory resolve); `store_root` is the centralized
/// `~/.harness/projects/<id>/` where the JSONL ledgers live. Keeping them separate
/// is the core of multi-project: the store centralizes while worktrees + agent cwd
/// stay tied to the project's own directory/git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContext {
    pub id: String,
    pub project_root: std::path::PathBuf,
    pub store_root: std::path::PathBuf,
    pub kind: ProjectKind,
    pub is_git_repo: bool,
}

/// FNV-1a 64-bit — a small, stable, dependency-free hash used to content-address
/// projects OUTSIDE `$HOME` (where there is no clean relative slug). Stable across
/// runs/platforms (unlike `std::hash::DefaultHasher`), which is what a durable
/// project id needs; it is not used for any security purpose.
fn fnv1a_hex16(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// A stable 16-hex content hash of a string (FNV-1a, dependency-free). Used to
/// name compiled phase workflows so an identical DAG → identical filename.
pub fn content_hash_hex16(s: &str) -> String {
    fnv1a_hex16(s.as_bytes())
}

/// Make one path segment filesystem-safe for use inside a project-id slug:
/// keep `[A-Za-z0-9._-]`, replace every other char (incl. path separators) with `-`.
fn sanitize_id_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Derive a STABLE project id from a project's canonical absolute path, relative to
/// the canonical `$HOME`:
/// - `path == home` → [`GLOBAL_PROJECT_ID`] (`_global`).
/// - under `home` → the relative path with separators flattened to `-`
///   (e.g. `~/ai-luodi/jyx3d` → `ai-luodi-jyx3d`).
/// - outside `home` → `proj-<fnv1a-hex16>` of the canonical path string.
///
/// Callers should pass realpath-canonicalized paths so symlinks / `..` don't mint
/// two ids for one project. NOTE (known edge): the `/`→`-` flattening can collide
/// `a/b-c` with `a-b/c`; acceptable for v1, revisit if it bites.
pub fn project_id_for_path(path: &std::path::Path, home: &std::path::Path) -> String {
    if path == home {
        return GLOBAL_PROJECT_ID.to_string();
    }
    match path.strip_prefix(home) {
        Ok(rel) => {
            let slug = sanitize_id_segment(
                &rel.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "-"),
            );
            // A leading/trailing or doubled '-' from odd paths is harmless; an empty
            // slug (shouldn't happen, since path != home) falls back to the hash.
            if slug.is_empty() {
                format!("proj-{}", fnv1a_hex16(path.to_string_lossy().as_bytes()))
            } else {
                slug
            }
        }
        Err(_) => format!("proj-{}", fnv1a_hex16(path.to_string_lossy().as_bytes())),
    }
}

/// The centralized store root for a project id, under a harness home (`~/.harness`):
/// `<harness_home>/projects/<id>`.
pub fn project_store_root(harness_home: &std::path::Path, id: &str) -> std::path::PathBuf {
    harness_home.join("projects").join(id)
}

/// Normalized, provider-neutral turn-event kind. Maps both codex
/// (`type`/`item`) and claude (stream-json `type`/subtype) vocabularies onto
/// one taxonomy so the dashboard and a 3rd provider need no per-provider branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessTurnEventKind {
    TurnStarted,
    TurnCompleted,
    MessageDelta,
    Message,
    ToolCall,
    ToolResult,
    Reasoning,
    Usage,
    Error,
    ProviderMeta,
    Unknown,
}

/// A normalized tool invocation (`ToolCall` kind).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub args: serde_json::Value,
}

/// A normalized tool result (`ToolResult` kind).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessToolResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub content: String,
    pub is_error: bool,
}

/// Normalized token usage (`Usage`/`TurnCompleted` kinds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_output_tokens: Option<u64>,
}

/// One normalized turn event. `raw_provider_event` always retains the original
/// provider JSON for audit / debugging / a "show raw" toggle. `seq` is a
/// harness-assigned monotonic per-session counter; `ts` is harness-assigned at
/// ingest. `session_id` is the harness provider-session row id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessTurnEvent {
    pub session_id: String,
    pub provider: String,
    pub seq: u64,
    pub ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_item_id: Option<String>,
    pub kind: HarnessTurnEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<HarnessToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<HarnessToolResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<HarnessTokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub raw_provider_event: serde_json::Value,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// A durable product vision that can guide Missions and Company OS modules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vision {
    pub id: String,
    pub summary: String,
    /// PRD / design-basis doc paths backing the vision.
    pub source_refs: Vec<String>,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Mission / Wave product contracts (ADR 0026)
//
// A Mission owns durable intent and outcome; each Wave owns a small, ordered
// execution attempt and delegates its internal execution semantics to its
// selected executor.
// ---------------------------------------------------------------------------

/// Lifecycle of a [`Mission`]. Executor-specific progress belongs to Waves and
/// their runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    #[default]
    Planned,
    Running,
    Blocked,
    Completed,
    Cancelled,
}

/// Durable operator intent. `desired_outcome` captures the intended result;
/// `outcome_summary` is filled only after execution has produced one. A Mission
/// does not contain a task graph or executor-specific state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub title: String,
    pub objective: String,
    #[serde(default)]
    pub desired_outcome: Option<String>,
    #[serde(default)]
    pub status: MissionStatus,
    /// Ordered Wave identities. Wave rows remain their own append-only ledger;
    /// this is a convenient explicit membership projection, not a replacement
    /// for reading the Wave ledger by `mission_id`.
    #[serde(default)]
    pub wave_ids: Vec<String>,
    #[serde(default)]
    pub outcome_summary: Option<String>,
    /// Actor that explicitly performed Mission closeout. Wave acceptance does
    /// not infer this responsibility.
    #[serde(default)]
    pub completed_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
}

/// The executor selected for a [`Wave`]. Its execution records live in the
/// executor's own ledger: `AgentTeamRun`, `WorkflowRun`, or a Host-owned run
/// reference. This enum intentionally has no task-graph variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveExecutorKind {
    AgentTeam,
    DynamicWorkflow,
    Host,
}

/// Lifecycle of a [`Wave`], kept separate from its lightweight gate result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaveStatus {
    #[default]
    Planned,
    Running,
    Waiting,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

/// Lightweight acceptance state for a [`Wave`]. Repositories may retain
/// stricter governance on top of this product contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaveGateStatus {
    #[default]
    Pending,
    Accepted,
    Revise,
    Blocked,
}

/// One ordered unit of a Mission. Retries and replacement attempts are recorded
/// in `executor_run_ids`; `accepted_run_id` identifies the attempt accepted by
/// the Wave gate. A Wave has no task graph or executor-specific child model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wave {
    pub id: String,
    pub mission_id: String,
    pub index: u32,
    pub title: String,
    pub objective: String,
    #[serde(default)]
    pub exit_criteria: Option<String>,
    #[serde(default)]
    pub status: WaveStatus,
    pub executor_kind: WaveExecutorKind,
    #[serde(default)]
    pub executor_run_ids: Vec<String>,
    #[serde(default)]
    pub accepted_run_id: Option<String>,
    #[serde(default)]
    pub plan_note: Option<String>,
    #[serde(default)]
    pub outcome_summary: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub gate_status: WaveGateStatus,
    #[serde(default)]
    pub gate_note: Option<String>,
    #[serde(default)]
    pub accepted_by: Option<String>,
    #[serde(default)]
    pub accepted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
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

impl Validate for Vision {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Vision.id")?;
        require_non_empty(&self.summary, "Vision.summary")?;
        require_non_empty(&self.created_at, "Vision.created_at")
    }
}

impl Validate for Mission {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Mission.id")?;
        require_non_empty(&self.title, "Mission.title")?;
        require_non_empty(&self.objective, "Mission.objective")?;
        require_non_empty(&self.created_at, "Mission.created_at")?;
        require_non_empty(&self.updated_at, "Mission.updated_at")
    }
}

impl Validate for Wave {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "Wave.id")?;
        require_non_empty(&self.mission_id, "Wave.mission_id")?;
        require_non_empty(&self.title, "Wave.title")?;
        require_non_empty(&self.objective, "Wave.objective")?;
        require_non_empty(&self.created_at, "Wave.created_at")?;
        require_non_empty(&self.updated_at, "Wave.updated_at")
    }
}

// ---------------------------------------------------------------------------
// Dynamic workflow runtime objects (WP1)
//
// A `WorkflowRun` is a standalone object with its own id and lifecycle. Each
// `WorkflowStep` is the workflow-layer wrapper around one `agent()` call and references the
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

/// Machine-readable class describing how a workflow run or step terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTerminalReason {
    CanceledByOperator,
    DriverExited,
    OrphanReaped,
    LeafTimeout,
    IdleTimeout,
    ProviderFailed,
    VerdictFailed,
    Completed,
}

impl WorkflowTerminalReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CanceledByOperator => "canceled_by_operator",
            Self::DriverExited => "driver_exited",
            Self::OrphanReaped => "orphan_reaped",
            Self::LeafTimeout => "leaf_timeout",
            Self::IdleTimeout => "idle_timeout",
            Self::ProviderFailed => "provider_failed",
            Self::VerdictFailed => "verdict_failed",
            Self::Completed => "completed",
        }
    }
}

/// Durable lifecycle for a patch captured from a writable workflow leaf.
///
/// A patch starts as `pending_apply` when the worker's throwaway worktree
/// produced a diff. It then moves by latest-wins rows to `applied`, `rejected`,
/// or `conflict` after an explicit operator/Lead/workflow decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPatchStatus {
    PendingApply,
    Applied,
    Rejected,
    Conflict,
}

/// Validation status of files recorded in a workflow artifact manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowArtifactManifestStatus {
    Current,
    Missing,
    Stale,
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
    /// Optional JSON parameterization the run was authored with (the dynamic
    /// `run-script` path carries the Starlark `args` global). `None` for registry
    /// runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// How many agent steps this run spawned (the per-run agent count). Defaults
    /// to 0 for legacy rows that predate the field.
    #[serde(default)]
    pub agents_spawned: u64,
    /// The collected structured output of the run (e.g. each step's result),
    /// set when the run reaches a terminal state. `None` while running / legacy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_output: Option<serde_json::Value>,
    /// Who initiated this run — an agent member id (e.g. a Codex / Claude member)
    /// or "operator" for a human-triggered CLI run. `None` for legacy rows that
    /// predate the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<String>,
    /// The mandatory `design_intent` a Starlark program declares via its
    /// `workflow(name, design_intent)` header — the WHY behind the run's shape.
    /// Every dynamic (`run-script`) run carries it; `None` for registry runs and
    /// legacy rows that predate the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_intent: Option<String>,
    /// The authored source the dynamic path was run with — for `run-script` the
    /// raw Starlark program text, snapshotted as the small durable audit record
    /// of the run shape. `None` for registry runs / legacy rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<serde_json::Value>,
    /// Retention policy for the heavy per-node provider turn-event trace:
    /// "durable" (default) persists the trace so any completed run can be drilled
    /// into; "live" streams the trace over SSE during execution but does not
    /// retain it. Live streaming is independent of this and always happens.
    #[serde(default = "default_trace_retention")]
    pub trace_retention: String,
    /// OS process id of the `harness workflow run-script`/`run` invocation that
    /// drives this run, stamped on the initial `running` row. The serve-side
    /// reaper uses it to detect an ABANDONED run: if the run is still `running`
    /// but this pid is no longer alive on the host, the driver died (killed /
    /// crashed / Ctrl-C) before journaling a terminal outcome, so the reaper
    /// flips it (and its non-terminal steps) to `failed`. `None` for legacy rows
    /// that predate the field — those fall back to a stale-activity timeout.
    /// Same-host only (the store, serve, and driver all run locally).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_pid: Option<u32>,
    /// True when this run was a `--dry-run` validation (mock driver, no provider
    /// spawned, no tokens spent), false for a real (live) run. A dry-run journals
    /// the SAME `workflow_name` into the SAME store, so without this marker a dry
    /// validation run is easily mistaken for a real one when reading the jsonl or
    /// the dashboard (issue #89 item 2). `#[serde(default)]` → legacy rows read as
    /// `false` (they predate the flag; dry-run journaling is newer).
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<WorkflowTerminalReason>,
    #[serde(default)]
    pub partial_output_available: bool,
}

/// Default retention policy for a [`WorkflowRun`]'s turn-event trace. Legacy rows
/// and registry runs that predate the field deserialize as "durable".
fn default_trace_retention() -> String {
    "durable".to_string()
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
    /// Optional structured result for this step (beyond the human-facing
    /// `output_summary`). The dynamic IR path carries each `StepResult`'s
    /// structured payload here. `None` for legacy / summary-only steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<WorkflowTerminalReason>,
    #[serde(default)]
    pub partial: bool,
}

/// A durable patch captured from a writable workflow step.
///
/// The actual unified diff lives at `patch_ref` so dashboard snapshots stay
/// compact while CLI `workflow patch show/apply` can still retrieve the complete
/// patch text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPatch {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub label: String,
    pub phase: String,
    pub provider: String,
    pub status: WorkflowPatchStatus,
    #[serde(default)]
    pub changed_paths: Vec<String>,
    /// Absolute or store-relative path to the `.patch` file.
    pub patch_ref: String,
    #[serde(default)]
    pub base_sha: Option<String>,
    #[serde(default)]
    pub owned_paths: Vec<String>,
    #[serde(default)]
    pub persist_changes: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub conflict_detail: Option<String>,
    #[serde(default)]
    pub applied_at: Option<String>,
    #[serde(default)]
    pub rejected_at: Option<String>,
}

/// One file entry inside a workflow artifact manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowArtifactFile {
    /// Repo-relative path when under the project root, else the absolute path the
    /// workflow explicitly declared.
    pub path: String,
    #[serde(default)]
    pub exists: bool,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// Durable manifest for files a workflow claims as artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowArtifactManifest {
    pub id: String,
    pub run_id: String,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub artifact_root: Option<String>,
    pub status: WorkflowArtifactManifestStatus,
    #[serde(default)]
    pub files: Vec<WorkflowArtifactFile>,
    #[serde(default)]
    pub write_roots: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
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

impl Validate for WorkflowPatch {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowPatch.id")?;
        require_non_empty(&self.run_id, "WorkflowPatch.run_id")?;
        require_non_empty(&self.step_id, "WorkflowPatch.step_id")?;
        require_non_empty(&self.label, "WorkflowPatch.label")?;
        require_non_empty(&self.patch_ref, "WorkflowPatch.patch_ref")?;
        require_non_empty(&self.created_at, "WorkflowPatch.created_at")
    }
}

impl Validate for WorkflowArtifactFile {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.path, "WorkflowArtifactFile.path")
    }
}

impl Validate for WorkflowArtifactManifest {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowArtifactManifest.id")?;
        require_non_empty(&self.run_id, "WorkflowArtifactManifest.run_id")?;
        require_non_empty(&self.created_at, "WorkflowArtifactManifest.created_at")?;
        for file in &self.files {
            file.validate()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Agent Team v0 runtime ledger objects
//
// A team run is one execution of an agent team against an objective, hosted on
// a single host surface (codex-app / kimi-cli / claude-cli). `MemberRun`s are
// the per-member session rows inside it; `TeamMessage`s the routed mail;
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

/// One execution of an agent team against `objective`. `definition_id` links
/// back to a TeamDefinition when one exists (v0 runs may be ad-hoc, so it is
/// nullable). `host_surface` names the hosting surface ("codex-app" /
/// "kimi-cli" / "claude-cli") and `host_thread_id` its native thread.
/// `previous_run_id` records retry/replacement lineage. For native Mission/Wave
/// execution it points to an earlier attempt of the same Wave; legacy unlinked
/// runs may retain older ad-hoc lineage semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTeamRun {
    pub id: String,
    #[serde(default)]
    pub definition_id: Option<String>,
    #[serde(default)]
    pub previous_run_id: Option<String>,
    /// Optional outer product identity (ADR 0026). Existing v0 team-run rows
    /// remain readable without these joins during the staged migration.
    #[serde(default)]
    pub mission_id: Option<String>,
    #[serde(default)]
    pub wave_id: Option<String>,
    pub host_surface: String,
    #[serde(default)]
    pub host_thread_id: Option<String>,
    pub objective: String,
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

/// Lifecycle of a [`MemberRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRunStatus {
    Starting,
    Idle,
    Queued,
    Running,
    Waiting,
    Reviewing,
    Blocked,
    Completed,
    Failed,
    Stopped,
}

/// One member's session inside an [`AgentTeamRun`]. `provider` is the neutral
/// provider spelling (codex|claude|kimi). `provider_session_id` links the
/// harness [`ProviderSession`] while `acp_session_id` is the provider-side
/// session handle (e.g. a kimi ACP sessionId).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberRun {
    pub id: String,
    pub team_run_id: String,
    #[serde(default)]
    pub slot_id: Option<String>,
    pub name: String,
    pub role: String,
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    /// Immutable-at-start snapshot of the concrete provider execution path.
    /// This distinguishes provider-native capability from what this adapter
    /// and execution mode have actually wired for the run.
    #[serde(default)]
    pub provider_profile: Option<ProviderIntegrationProfile>,
    pub status: MemberRunStatus,
    #[serde(default)]
    pub provider_session_id: Option<String>,
    #[serde(default)]
    pub acp_session_id: Option<String>,
    #[serde(default)]
    pub worktree_ref: Option<String>,
    #[serde(default)]
    pub owned_paths: Vec<String>,
    pub started_at: String,
    #[serde(default)]
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

/// How one provider member is executed by Harness. Capability claims are
/// mode-specific: `codex_exec` and `kimi_acp` are different products even when
/// their user-facing provider names are simply Codex and Kimi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderIntegrationProfile {
    pub provider: String,
    pub execution_mode: String,
    #[serde(default)]
    pub provider_version: Option<String>,
    #[serde(default)]
    pub adapter_contract_version: Option<String>,
    #[serde(default)]
    pub reviewed_provider_versions: Vec<String>,
    #[serde(default)]
    pub compatibility_status: ProviderCompatibilityStatus,
    #[serde(default)]
    pub adapter_reviewed_at: Option<String>,
    #[serde(default)]
    pub compatibility_note: Option<String>,
    pub interaction_mode: ProviderInteractionMode,
    pub tool_event_fidelity: ProviderEventFidelity,
    pub artifact_event_fidelity: ProviderEventFidelity,
    pub supports_cancel: bool,
    pub supports_resume: bool,
    pub observes_native_subagents: bool,
    pub observes_background_tasks: bool,
    /// Product policy, not a provider claim. Thinking may only appear through
    /// the sanitized transient live channel and is never durable or replayed.
    pub thinking_transient_only: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCompatibilityStatus {
    Current,
    ReviewRequired,
    Incompatible,
    Unavailable,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInteractionMode {
    /// The provider can pause the same turn until the client answers.
    PauseAndResume,
    /// The execution mode cannot accept mid-turn input; end the round with a
    /// blocker and start a follow-up after the Host answers.
    EndRoundAndFollowUp,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderEventFidelity {
    None,
    Summary,
    Structured,
}

/// A provider-originated request that pauses or blocks a MemberRun until an
/// authorized actor responds. It is product state; unlike thinking it is
/// durable, replayable, and visible to the Host/Dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingInteraction {
    pub id: String,
    pub team_run_id: String,
    pub member_run_id: String,
    pub provider: String,
    pub provider_request_id: String,
    pub method: String,
    pub kind: PendingInteractionKind,
    pub route: PendingInteractionRoute,
    pub status: PendingInteractionStatus,
    pub title: String,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<PendingInteractionOption>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub response_option_id: Option<String>,
    #[serde(default)]
    pub response_text: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub resolved_at: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingInteractionOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingInteractionKind {
    Question,
    ToolApproval,
    PlanReview,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingInteractionRoute {
    Lead,
    Human,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingInteractionStatus {
    Pending,
    Answered,
    Approved,
    Denied,
    Dismissed,
    Unsupported,
    Cancelled,
}

/// Kind of a routed [`TeamMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageKind {
    Assignment,
    Question,
    Answer,
    Progress,
    Blocker,
    Handoff,
    ReviewRequest,
    ReviewResult,
    Control,
    Broadcast,
}

/// How a [`TeamMessage`] should be delivered to one recipient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamDeliveryPolicy {
    Queue,
    Inject,
    Interrupt,
    ManualAck,
}

/// Per-recipient delivery state of a [`TeamMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamDeliveryStatus {
    Queued,
    Delivered,
    Acknowledged,
    Failed,
    Expired,
}

/// One recipient's delivery record inside a [`TeamMessage`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMessageDelivery {
    pub member_id: String,
    pub policy: TeamDeliveryPolicy,
    pub status: TeamDeliveryStatus,
    pub attempt: u32,
    pub updated_at: String,
}

/// A routed message inside an [`AgentTeamRun`]. `from_member_id` is either the
/// reserved `"host"` id or a `MemberRun` id. `correlation_id` groups a message
/// with its replies; `causation_id` points at the message this one answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMessage {
    pub id: String,
    pub team_run_id: String,
    pub from_member_id: String,
    #[serde(default)]
    pub to_member_ids: Vec<String>,
    pub kind: TeamMessageKind,
    pub body: String,
    pub correlation_id: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub deliveries: Vec<TeamMessageDelivery>,
    pub created_at: String,
}

/// Status of a single [`MemberAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberActionStatus {
    Started,
    Progress,
    Succeeded,
    Failed,
    Cancelled,
}

/// One journaled action by a member inside an [`AgentTeamRun`]. `seq` is
/// monotonically increasing per team run and is assigned by the caller.
/// `action_type` is a free-form string in v0 (conventional values:
/// plan_updated, message_sent, message_received, tool_started, tool_completed,
/// file_changed, command_started, command_completed, test_started,
/// test_completed, delegation_started, delegation_completed, review_started,
/// review_completed, waiting_for_input, waiting_for_approval, blocked, error,
/// completed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberAction {
    pub id: String,
    pub seq: u64,
    pub team_run_id: String,
    pub member_run_id: String,
    #[serde(default)]
    pub task_id: Option<String>,
    /// Provider-native call/item id for correlating start, progress, result,
    /// permission, and artifact frames without leaking provider semantics into
    /// the generic action id.
    #[serde(default)]
    pub provider_call_id: Option<String>,
    pub action_type: String,
    pub status: MemberActionStatus,
    /// Raw lifecycle status reported by the provider transport.
    #[serde(default)]
    pub provider_status: Option<String>,
    /// Harness interpretation after interaction/result semantics are known.
    /// `provider_status=completed` must not imply `semantic_status=succeeded`.
    #[serde(default)]
    pub semantic_status: Option<String>,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub started_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
}

/// How a [`DelegationRun`] is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationMode {
    ProviderNative,
    HarnessWorker,
    DynamicWorkflow,
}

/// Lifecycle of a [`DelegationRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationStatus {
    Planned,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

/// One delegation of work out of a [`MemberRun`]: a provider-native child
/// thread, a harness worker, or a dynamic workflow run. Exactly one of
/// `provider_child_thread_id` / `workflow_run_id` is typically set, matching
/// `mode`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationRun {
    pub id: String,
    pub team_run_id: String,
    pub parent_member_run_id: String,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    pub mode: DelegationMode,
    pub provider: String,
    #[serde(default)]
    pub provider_child_thread_id: Option<String>,
    #[serde(default)]
    pub workflow_run_id: Option<String>,
    pub objective: String,
    pub status: DelegationStatus,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Where a [`TeamRunEvent`] originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunEventSourceKind {
    Host,
    Member,
    Delegation,
}

/// One folded event in an [`AgentTeamRun`]'s per-run event log. `seq` is
/// monotonically increasing per team run and is assigned by the caller.
/// `entity_type` (team_run|member_run|assignment|action|message|delegation|
/// artifact) + `entity_id` + `operation` (created|updated|completed) reference
/// the ledger row this event summarizes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRunEvent {
    pub id: String,
    pub seq: u64,
    pub team_run_id: String,
    pub source_kind: TeamRunEventSourceKind,
    #[serde(default)]
    pub member_run_id: Option<String>,
    #[serde(default)]
    pub delegation_run_id: Option<String>,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub summary: String,
    pub occurred_at: String,
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
    fn provider_price_per_mtok_preserves_provider_rates() {
        assert_eq!(provider_price_per_mtok("claude"), (3.0, 15.0));
        assert_eq!(provider_price_per_mtok("codex"), (1.25, 10.0));
        assert_eq!(provider_price_per_mtok("gemini"), (1.25, 10.0));
        // Kimi has its own placeholder row (NOT priced as gpt-5-class), so spend
        // estimates don't wildly over-bound a cheaper provider
        // (goal-provider-neutral S4). Confirm it diverges from the default.
        assert_eq!(provider_price_per_mtok("kimi"), (0.60, 2.50));
        assert_ne!(
            provider_price_per_mtok("kimi"),
            provider_price_per_mtok("codex")
        );
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
        assert_eq!(
            parsed.verdict,
            ReviewVerdict::Other("conditional_pass".to_string())
        );
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
    fn vision_round_trips_json() {
        let vision = Vision {
            id: "vision-1".to_string(),
            summary: "Generic harness object-model with a closed learning loop.".to_string(),
            source_refs: vec!["docs/company-os/vision.md".to_string()],
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
    fn project_id_for_path_home_is_global() {
        let home = std::path::Path::new("/Users/me");
        assert_eq!(project_id_for_path(home, home), GLOBAL_PROJECT_ID);
    }

    #[test]
    fn project_id_for_path_under_home_flattens_to_slug() {
        let home = std::path::Path::new("/Users/me");
        assert_eq!(
            project_id_for_path(std::path::Path::new("/Users/me/multi-agent-harness"), home),
            "multi-agent-harness"
        );
        assert_eq!(
            project_id_for_path(std::path::Path::new("/Users/me/ai-luodi/jyx3d"), home),
            "ai-luodi-jyx3d"
        );
    }

    #[test]
    fn project_id_for_path_outside_home_is_stable_hash() {
        let home = std::path::Path::new("/Users/me");
        let id = project_id_for_path(std::path::Path::new("/opt/work/thing"), home);
        assert!(id.starts_with("proj-"), "external path → hashed id: {id}");
        // Stable across calls (a durable id must not change run-to-run).
        assert_eq!(
            id,
            project_id_for_path(std::path::Path::new("/opt/work/thing"), home)
        );
        // Distinct paths → distinct ids.
        assert_ne!(
            id,
            project_id_for_path(std::path::Path::new("/opt/work/other"), home)
        );
    }

    #[test]
    fn project_store_root_is_under_projects() {
        let home = std::path::Path::new("/Users/me/.harness");
        assert_eq!(
            project_store_root(home, "ai-luodi-jyx3d"),
            std::path::Path::new("/Users/me/.harness/projects/ai-luodi-jyx3d")
        );
        assert_eq!(
            project_store_root(home, GLOBAL_PROJECT_ID),
            std::path::Path::new("/Users/me/.harness/projects/_global")
        );
    }

    #[test]
    fn project_context_round_trips_json() {
        let ctx = ProjectContext {
            id: "ai-luodi-jyx3d".into(),
            project_root: std::path::PathBuf::from("/Users/me/ai-luodi/jyx3d"),
            store_root: std::path::PathBuf::from("/Users/me/.harness/projects/ai-luodi-jyx3d"),
            kind: ProjectKind::Repo,
            is_git_repo: true,
        };
        let json = serde_json::to_string(&ctx).expect("serialize");
        assert_eq!(
            serde_json::from_str::<ProjectContext>(&json).expect("deserialize"),
            ctx
        );
        // kind is snake_case on the wire.
        assert!(json.contains("\"kind\":\"repo\""));
    }

    #[test]
    fn harness_turn_event_round_trips_json_and_omits_absent_optionals() {
        let tool_event = HarnessTurnEvent {
            session_id: "session-1".to_string(),
            provider: "codex".to_string(),
            seq: 1,
            ts: "2026-06-13T00:00:00Z".to_string(),
            provider_thread_id: None,
            provider_turn_id: Some("turn-1".to_string()),
            provider_item_id: None,
            kind: HarnessTurnEventKind::ToolCall,
            role: None,
            text: None,
            delta: None,
            tool_call: Some(HarnessToolCall {
                id: Some("call-1".to_string()),
                name: "shell".to_string(),
                args: serde_json::json!({ "cmd": "cargo test -p harness-core" }),
            }),
            tool_result: None,
            usage: None,
            model: Some("gpt-5".to_string()),
            duration_ms: None,
            cost_usd: None,
            status: None,
            error: None,
            raw_provider_event: serde_json::json!({
                "type": "item",
                "item": { "type": "tool_call", "id": "call-1" }
            }),
        };

        let usage_event = HarnessTurnEvent {
            session_id: "session-1".to_string(),
            provider: "codex".to_string(),
            seq: 2,
            ts: "2026-06-13T00:00:01Z".to_string(),
            provider_thread_id: None,
            provider_turn_id: None,
            provider_item_id: None,
            kind: HarnessTurnEventKind::Usage,
            role: None,
            text: None,
            delta: None,
            tool_call: None,
            tool_result: None,
            usage: Some(HarnessTokenUsage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                cached_input_tokens: None,
                reasoning_output_tokens: Some(5),
            }),
            model: None,
            duration_ms: None,
            cost_usd: None,
            status: None,
            error: None,
            raw_provider_event: serde_json::json!({
                "type": "usage",
                "input_tokens": 10,
                "output_tokens": 20
            }),
        };

        for event in [tool_event, usage_event] {
            let json = serde_json::to_value(&event).expect("serialize turn event");
            let parsed: HarnessTurnEvent =
                serde_json::from_value(json.clone()).expect("deserialize turn event");

            assert_eq!(parsed, event);
            assert!(json.get("raw_provider_event").is_some());
            assert!(json.get("provider_thread_id").is_none());
            assert!(json.get("provider_item_id").is_none());
            assert!(json.get("role").is_none());
            assert!(json.get("text").is_none());
            assert!(json.get("delta").is_none());
            assert!(json.get("duration_ms").is_none());
            assert!(json.get("cost_usd").is_none());
            assert!(json.get("status").is_none());
            assert!(json.get("error").is_none());
        }
    }

    #[test]
    fn harness_turn_event_kind_wire_spellings_are_snake_case() {
        // These wire strings are the API contract the SSE stream, the
        // /v1/.../events endpoints, and the dashboard key on — pin every variant.
        let cases = [
            (HarnessTurnEventKind::TurnStarted, "turn_started"),
            (HarnessTurnEventKind::TurnCompleted, "turn_completed"),
            (HarnessTurnEventKind::MessageDelta, "message_delta"),
            (HarnessTurnEventKind::Message, "message"),
            (HarnessTurnEventKind::ToolCall, "tool_call"),
            (HarnessTurnEventKind::ToolResult, "tool_result"),
            (HarnessTurnEventKind::Reasoning, "reasoning"),
            (HarnessTurnEventKind::Usage, "usage"),
            (HarnessTurnEventKind::Error, "error"),
            (HarnessTurnEventKind::ProviderMeta, "provider_meta"),
            (HarnessTurnEventKind::Unknown, "unknown"),
        ];
        for (kind, wire) in cases {
            assert_eq!(
                serde_json::to_value(&kind).expect("serialize kind"),
                serde_json::Value::String(wire.to_string()),
                "kind {kind:?} should serialize to {wire:?}"
            );
            let back: HarnessTurnEventKind =
                serde_json::from_value(serde_json::json!(wire)).expect("deserialize kind");
            assert_eq!(back, kind);
        }
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
            kind: MessageKind::Assignment,
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
            kind: MessageKind::Assignment,
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
        member.provider_config.effort = Some("high".to_string());
        member.runtime_workspace_roots = vec!["crates/harness-core".to_string()];
        member.provider_config.runtime_workspace_roots = vec!["crates/harness-cli".to_string()];
        let message = sample_message();

        let spec = build_launch_spec(&member, &message);

        // Pillar 1 base configuration flows through unchanged.
        assert_eq!(
            spec.prompt_ref.as_deref(),
            Some(".harness/prompts/worker.md")
        );
        assert_eq!(spec.model.as_deref(), Some("o3"));
        assert_eq!(spec.effort.as_deref(), Some("high"));
        assert_eq!(spec.skill_refs, vec!["harness-workflow".to_string()]);
        // Pillar 2 workspace flows through as the cwd / worktree root.
        assert_eq!(spec.workspace.as_deref(), Some("../worktrees/task-1"));
        // The turn input carries the message envelope + content.
        assert!(spec.message_content.contains("message_id: msg-1"));
        assert!(spec.message_content.contains("kind: assignment"));
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
            assert_eq!(
                spec.permission, expected,
                "policy {policy} should map to {expected:?}"
            );
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
        member.provider_config.effort = Some("medium".to_string());
        member.provider_config.output_schema = Some(serde_json::json!({
            "type": "object",
            "properties": { "verdict": { "type": "string" } },
            "required": ["verdict"]
        }));
        member.runtime_workspace_roots = vec!["crates".to_string()];
        let spec = build_launch_spec(&member, &sample_message());

        let json = serde_json::to_string(&spec).expect("serialize launch spec");
        let parsed: LaunchSpec = serde_json::from_str(&json).expect("deserialize launch spec");
        assert_eq!(parsed, spec);
        // The neutral permission serializes to its snake_case wire spelling, not
        // the Codex `workspaceWrite` vocabulary it was mapped from.
        assert!(json.contains("\"permission\":\"workspace_write\""));
        assert!(json.contains("\"effort\":\"medium\""));
        assert!(json.contains("\"output_schema\""));
        assert_eq!(
            parsed.output_schema, member.provider_config.output_schema,
            "launch spec should round-trip the optional output schema"
        );
        assert!(!json.contains("workspaceWrite"));
    }

    #[test]
    fn effort_defaults_to_none_for_legacy_json() {
        let provider_config: AgentProviderConfig = serde_json::from_value(serde_json::json!({
            "service_tier": "default"
        }))
        .expect("legacy provider config without effort should deserialize");
        assert!(provider_config.effort.is_none());
        assert!(provider_config.output_schema.is_none());

        let spec: LaunchSpec = serde_json::from_value(serde_json::json!({
            "message_content": "legacy turn",
            "model": "o3",
            "permission": "workspace_write"
        }))
        .expect("legacy launch spec without effort should deserialize");
        assert!(spec.effort.is_none());
        assert!(spec.output_schema.is_none());
    }

    #[test]
    fn build_launch_spec_carries_output_schema_from_provider_config() {
        let mut member = sample_member();
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"]
        });
        member.provider_config.output_schema = Some(schema.clone());
        let spec = build_launch_spec(&member, &sample_message());
        assert_eq!(spec.output_schema, Some(schema));
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
            let parsed: DeliveryHandle = serde_json::from_str(&json).expect("deserialize handle");
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
        assert!(
            spec.mcp.is_some(),
            "launch spec should carry mcp from provider_config"
        );
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
        assert!(
            !cap.mid_turn_approval,
            "Codex exec has policy pre-approve, no mid-turn"
        );
        assert!(cap.subagents, "Codex supports subagents");
        assert!(cap.mcp, "Codex exec has --config mcp_servers");
        assert!(!cap.hooks, "Codex exec has limited hooks");
        assert!(cap.schema, "Codex exec has --output-schema");
        assert!(!cap.cost, "Codex reports token usage only, no USD");
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
        assert!(cap.schema, "Claude has --json-schema");
        assert!(cap.cost, "Claude reports result.total_cost_usd");
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
        assert!(
            !display.contains("mid_turn_approval"),
            "disabled features should not show"
        );
    }

    #[test]
    fn supports_streaming_exec_check() {
        let mut cap = ProviderCapabilities::codex_exec();
        assert!(
            cap.supports_streaming_exec(),
            "streaming + no mid-turn should be ok"
        );
        cap.mid_turn_approval = true;
        assert!(
            !cap.supports_streaming_exec(),
            "mid-turn approval blocks streaming exec"
        );
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
        IoError { skill_id: String, reason: String },
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
        let content =
            std::fs::read_to_string(&skill_path).map_err(|e| SkillResolutionError::IoError {
                skill_id: skill_id.to_string(),
                reason: e.to_string(),
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
    /// Platform supports a NATIVE structured-output / JSON-schema flag (codex
    /// `--output-schema`, claude `--json-schema`). When `false`, schema-mode
    /// nodes degrade to the prompt-coaxed text-extraction fallback rather than a
    /// special code path (goal-provider-neutral capability matrix: `schema`).
    /// Defaults to `false` for providers that don't declare it.
    #[serde(default)]
    pub schema: bool,
    /// Platform reports billed USD in its terminal frame (claude
    /// `result.total_cost_usd`; codex reports token usage only). When `false`,
    /// spend degrades to a token-based estimate or `null` (goal-provider-neutral
    /// capability matrix: `cost`). Defaults to `false`.
    #[serde(default)]
    pub cost: bool,
    /// Platform can run a leaf that is PHYSICALLY prevented from mutating the
    /// workspace — codex `--sandbox read-only`, claude a read-only tool allowlist
    /// (`Read,Grep,Glob`). When `false` the provider has NO read-only mode (kimi's
    /// headless `kimi -p` rejects every permission flag), so a read-only leaf must be
    /// isolated in a throwaway worktree to keep its writes off the live repo rather
    /// than trusted to stay read-only. Defaults to `false` = assume-unenforceable
    /// (the safe default: isolate an unknown provider's read-only leaves too).
    #[serde(default)]
    pub enforces_read_only: bool,
}

impl ProviderCapabilities {
    /// Codex exec capabilities per the capability declaration table in
    /// docs/agent-integration-model.md.
    pub fn codex_exec() -> Self {
        ProviderCapabilities {
            streaming: true,          // --json NDJSON
            resume: true,             // --session
            mid_turn_approval: false, // policy pre-approve only
            subagents: true,          // observed in Codex
            mcp: true,                // --config mcp_servers.*
            hooks: false,             // limited in exec mode
            schema: true,             // --output-schema <file>
            cost: false,              // token usage only, no total_cost_usd
            enforces_read_only: true, // --sandbox read-only
        }
    }

    /// Claude exec capabilities per the capability declaration table.
    pub fn claude_exec() -> Self {
        ProviderCapabilities {
            streaming: true,          // --output-format stream-json
            resume: true,             // --resume
            mid_turn_approval: false, // not documented for -p; Tier-3 only
            subagents: true,          // observed in Claude
            mcp: true,                // --mcp-config JSON
            hooks: false,             // not documented
            schema: true,             // --json-schema → result.structured_output
            cost: true,               // result.total_cost_usd
            enforces_read_only: true, // --allowedTools Read,Grep,Glob (no Edit/Write/Bash)
        }
    }

    /// Kimi exec capabilities (goal-provider-neutral S4) — a HONEST, partly
    /// UNKNOWN preset for a provider whose live CLI has not been verified.
    ///
    /// ASSUMES the `kimi` CLI is invoked like claude (stream-json NDJSON, a
    /// terminal `result` frame), so `streaming` is the only axis claimed `true`.
    /// Every other axis is marked `false` = DEGRADED-until-proven, NOT a positive
    /// claim of absence: resume/MCP/schema/cost/hooks all need to be confirmed
    /// against the real binary (see the goal's S3 spike) before being flipped on.
    /// Marking them `false` is the safe default — a missing axis degrades to the
    /// shared fallback (text-extract for schema, token-estimate for cost,
    /// leaf-only for resume) rather than a per-provider branch.
    pub fn kimi_exec() -> Self {
        ProviderCapabilities {
            streaming: true,          // assumed: --output-format stream-json
            resume: false,            // UNKNOWN: resumable session id unverified
            mid_turn_approval: false, // UNKNOWN
            subagents: false,         // UNKNOWN
            mcp: false,               // UNKNOWN
            hooks: false,             // UNKNOWN: no lifecycle hook bridge
            schema: false,            // UNKNOWN: degrade to text-extract fallback
            cost: false,              // UNKNOWN: degrade to token-estimate
            // VERIFIED false: `kimi -p` rejects every permission flag (-y/--auto/
            // --plan) and has no tool allowlist, so it has NO read-only mode. A
            // read-only kimi leaf must be worktree-isolated, not trusted (the live
            // CLI was confirmed to edit the shared tree from a read-only leaf).
            enforces_read_only: false,
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
            ("schema", self.schema),
            ("cost", self.cost),
            ("enforces_read_only", self.enforces_read_only),
        ];
        let enabled: Vec<&str> = features
            .iter()
            .filter_map(|(name, enabled)| if *enabled { Some(*name) } else { None })
            .collect();
        write!(f, "{{{}}}", enabled.join(", "))
    }
}
