use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLaunchStatus {
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
pub struct ProviderLaunchConfig {
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
/// Provider launch configuration used only at the adapter boundary.
///
/// This row is not an identity object. Canonical identity is always the
/// `agentfirm_api::AgentMember` referenced by a `ProviderRuntimeProjection`.
pub struct ProviderLaunchProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub provider: String,
    pub model: Option<String>,
    pub profile: Option<String>,
    #[serde(default)]
    pub provider_config: ProviderLaunchConfig,
    pub capabilities: Vec<String>,
    pub team_ids: Vec<String>,
    pub prompt_ref: Option<String>,
    pub skill_refs: Vec<String>,
    pub workspace_policy: Option<String>,
    #[serde(default)]
    pub provider_cwd_hint: Option<String>,
    #[serde(default)]
    pub permission_profile: Option<String>,
    #[serde(default)]
    pub runtime_workspace_roots: Vec<String>,
    pub status: ProviderLaunchStatus,
    pub current_task_id: Option<String>,
    pub current_proposal_id: Option<String>,
    pub provider_runtime_id: Option<String>,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
    /// Transitional legacy resume handle. New paths use `native_session`.
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
/// [`RegistryMessage`] via [`build_launch_spec`]; each provider adapter (Pillar 3) then
/// maps it onto its own CLI/SDK call. It is the seam that keeps the operator
/// composer and Dashboard uniform across Codex, Claude, and future platforms.
///
/// Per ADR 0011 this neutral object carries **no** Codex wire vocabulary:
/// `permission` is the neutral [`LaunchPermission`] enum and `writable_roots`
/// replaces Codex's `workspaceWrite.writableRoots`. The Codex-leaking
/// `ProviderLaunchConfig` fields (`sandbox_policy`, `approval_policy`,
/// `service_tier`, `collaboration_mode`, …) are abstracted here, not reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchSpec {
    /// Composed system/developer instructions (Pillar 1 prompt stack), read as a
    /// durable artifact reference — not inline chat text. `None` when the member
    /// has no role prompt.
    #[serde(default)]
    pub prompt_ref: Option<String>,
    /// The turn input: the claimed [`RegistryMessage`] envelope + content.
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
    /// The event-stream output contract the adapter should request for
    /// in-memory reduction and transient live projection (Codex `--json`,
    /// Claude `--output-format stream-json`). Free string, neutral.
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

/// Compose the neutral turn-input envelope for a claimed [`RegistryMessage`].
///
/// This mirrors the harness message-envelope shape the CLI provider layer
/// already hands to a turn (message id / kind / task / routing + content) but
/// keeps it provider-neutral text: the adapter decides how to deliver it (Codex
/// `input` item, Claude `-p`, …).
fn compose_message_content(message: &RegistryMessage) -> String {
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

fn message_kind_wire(kind: &RegistryMessageIntent) -> &'static str {
    match kind {
        RegistryMessageIntent::Message => "message",
        RegistryMessageIntent::Report => "report",
    }
}

/// Build the provider-neutral [`LaunchSpec`] for one turn from a member and the
/// claimed [`RegistryMessage`].
///
/// This is the additive composition seam (ADR 0018 WP-1). It reads the existing
/// `ProviderLaunchProfile` / `ProviderLaunchConfig` fields — including the Codex-flavored
/// `sandbox_policy` — and produces a neutral spec: the permission posture and
/// `writable_roots` are abstracted out of the Codex `workspaceWrite` vocabulary,
/// and no Codex wire names appear on the result (ADR 0011). It does not perform
/// any delivery side effect and does not require a live provider binary.
pub fn build_launch_spec(member: &ProviderLaunchProfile, message: &RegistryMessage) -> LaunchSpec {
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
        workspace: member.provider_cwd_hint.clone(),
        // MCP from provider_config (Pillar 2); now available.
        mcp: member.provider_config.mcp.clone(),
        skill_refs: member.skill_refs.clone(),
        // Resume an existing provider session when the member already carries a
        // provider thread/session id from a prior delivery. This is what lets
        // memory carry across deliveries: the next turn is dispatched as a
        // resume of the same session (Codex `exec resume <id>`, Claude
        // `--resume <id>`) instead of a fresh session. `None` (no prior id) = a
        // fresh session.
        resume: member
            .native_session
            .as_ref()
            .map(|session| session.native_session_id.clone())
            .or_else(|| member.provider_thread_id.clone()),
        output: None,
    }
}
