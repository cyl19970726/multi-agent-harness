use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub mod company_os;
pub mod docs_v2;
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
/// Compatibility execution registry row.
///
/// This pre-ADR-0051 shape mixes organization identity with mutable provider,
/// runtime, session, and task state. New Organization writes use
/// [`DurableAgentMember`]; this row remains readable for explicit convergence
/// and for the existing runtime surfaces until their cutover is complete.
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
        resume: member
            .native_session
            .as_ref()
            .map(|session| session.native_session_id.clone())
            .or_else(|| member.provider_thread_id.clone()),
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
    /// Stable identity of the Host Agent that created and coordinates this
    /// team. The wire name is retained for compatibility; product surfaces
    /// call this actor the Team Lead. A Lead is not a MemberRun unless it
    /// explicitly joins the team as an executing member.
    pub owner_agent_id: String,
    #[serde(default = "default_agent_team_status")]
    pub status: AgentTeamStatus,
    pub member_ids: Vec<String>,
    /// Recursive Organization relation (ADR 0052): parent AgentTeam id.
    /// `None` means a root team. Rows written before the recursive-topology
    /// slice predate this field and read as roots.
    #[serde(default)]
    pub parent_team_id: Option<String>,
    /// Recursive Organization relation (ADR 0052): durable AgentMember that
    /// Hosts this team. A non-root team must name one and it must be a direct
    /// member of the parent team; a member hosts at most one team in V1.
    /// Optional only so compatibility rows carrying `owner_agent_id` alone
    /// remain readable.
    #[serde(default)]
    pub host_member_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Durable Company/Organization identity of one Agent (ADR 0052).
///
/// Mutable execution state deliberately does not live here. A durable member
/// binds to zero or more replaceable [`MemberRun`] generations, and each run
/// may bind to a provider-native session owned by that provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableAgentMember {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    #[serde(default)]
    pub provider_profile: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub workspace_policy: Option<String>,
    #[serde(default)]
    pub project_binding_id: Option<String>,
    #[serde(default)]
    pub business_access_ceiling_refs: Vec<String>,
    pub status: DurableAgentMemberStatus,
    #[serde(default)]
    pub created_by_member_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableAgentMemberStatus {
    Active,
    Paused,
    Retired,
}

/// A compatibility or target Team row does not resolve to one durable Host.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostAuthorityError {
    #[error("AgentTeam {team_id} has no durable Host identity; set host_member_id explicitly")]
    Missing { team_id: String },
    #[error(
        "AgentTeam {team_id} has conflicting Host authorities: owner_agent_id={owner_agent_id}, host_member_id={host_member_id}"
    )]
    Conflicting {
        team_id: String,
        owner_agent_id: String,
        host_member_id: String,
    },
    #[error("AgentTeam {team_id} Host references missing durable AgentMember: {host_member_id}")]
    UnknownMember {
        team_id: String,
        host_member_id: String,
    },
    #[error(
        "AgentTeam {team_id} has not completed Host cutover: owner_agent_id must equal host_member_id {host_member_id}"
    )]
    CompatibilityAuthorityStillActive {
        team_id: String,
        host_member_id: String,
    },
}

/// Deterministically map the legacy `owner_agent_id` wire field onto the
/// target durable Host identity.
///
/// An explicit `host_member_id` wins only when the legacy field is the neutral
/// `host` placeholder or the same identity. Two different concrete identities
/// are refused instead of choosing one silently. A compatibility-only row can
/// map a concrete legacy owner; the neutral placeholder cannot manufacture an
/// identity and therefore maps to `None`.
pub fn compat_host_member_id(team: &AgentTeam) -> Result<Option<&str>, HostAuthorityError> {
    let owner = team.owner_agent_id.trim();
    match team.host_member_id.as_deref().map(str::trim) {
        Some("") => Err(HostAuthorityError::Missing {
            team_id: team.id.clone(),
        }),
        Some(host) if owner == "host" || owner == host => Ok(Some(host)),
        Some(host) => Err(HostAuthorityError::Conflicting {
            team_id: team.id.clone(),
            owner_agent_id: owner.to_string(),
            host_member_id: host.to_string(),
        }),
        None if !owner.is_empty() && owner != "host" => Ok(Some(owner)),
        None => Ok(None),
    }
}

/// Resolve exactly one Host identity for a Team, including deterministic
/// compatibility mapping. Use [`validate_host_authority_cutover`] before
/// switching product authority to ensure every row is explicitly migrated.
pub fn resolve_team_host_authority(team: &AgentTeam) -> Result<&str, HostAuthorityError> {
    compat_host_member_id(team)?.ok_or_else(|| HostAuthorityError::Missing {
        team_id: team.id.clone(),
    })
}

/// Refuse Organization authority cutover unless every Team names an explicit,
/// existing durable Host and the legacy wire field aliases that same identity.
/// This makes the migration boundary visible and prevents dual Host authority.
pub fn validate_host_authority_cutover(
    teams: &BTreeMap<String, AgentTeam>,
    members: &BTreeMap<String, DurableAgentMember>,
) -> Result<(), HostAuthorityError> {
    for team in teams.values() {
        let Some(explicit_host) = team.host_member_id.as_deref().map(str::trim) else {
            return Err(HostAuthorityError::CompatibilityAuthorityStillActive {
                team_id: team.id.clone(),
                host_member_id: team.owner_agent_id.clone(),
            });
        };
        let resolved = resolve_team_host_authority(team)?;
        if team.owner_agent_id.trim() != explicit_host {
            return Err(HostAuthorityError::CompatibilityAuthorityStillActive {
                team_id: team.id.clone(),
                host_member_id: explicit_host.to_string(),
            });
        }
        if !members.contains_key(resolved) {
            return Err(HostAuthorityError::UnknownMember {
                team_id: team.id.clone(),
                host_member_id: resolved.to_string(),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Recursive AgentTeam topology (ADR 0052, target contract slice).
//
// Organization is the persistent recursive projection of Agent Teams: every
// non-root team names its `parent_team_id` and the durable `host_member_id`
// that created and coordinates it. These pure functions validate the graph
// invariants and answer recursive reads; they perform no I/O so the store,
// CLI, and future API/UI surfaces share one authority.
// ---------------------------------------------------------------------------

/// Graph invariant violation in the recursive AgentTeam topology.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TeamTopologyError {
    #[error("AgentTeam {team_id} references missing parent AgentTeam: {parent_team_id}")]
    UnknownParent {
        team_id: String,
        parent_team_id: String,
    },
    #[error("non-root AgentTeam {team_id} must name a durable host_member_id")]
    MissingHost { team_id: String },
    #[error(
        "AgentTeam {team_id} host {host_member_id} is not a direct member of parent AgentTeam {parent_team_id}"
    )]
    HostNotParentMember {
        team_id: String,
        host_member_id: String,
        parent_team_id: String,
    },
    #[error("AgentMember {host_member_id} hosts more than one AgentTeam: {first_team_id}, {second_team_id}")]
    MultipleHostedTeams {
        host_member_id: String,
        first_team_id: String,
        second_team_id: String,
    },
    #[error("AgentTeam topology cycle detected at AgentTeam {team_id}")]
    Cycle { team_id: String },
}

/// Validate the recursive AgentTeam graph invariants over the latest-row-wins
/// team projection (ADR 0052):
///
/// 1. every `parent_team_id` resolves to an existing team;
/// 2. every non-root team names a durable `host_member_id` that is a direct
///    member of its parent team;
/// 3. a member hosts at most one team (V1 primary-child-team rule, enforced
///    across teams of any status);
/// 4. the parent graph is acyclic.
///
/// Teams predating the topology slice carry neither field, read as roots, and
/// never trip an invariant.
pub fn validate_agent_team_topology(
    teams: &BTreeMap<String, AgentTeam>,
) -> Result<(), TeamTopologyError> {
    for team in teams.values() {
        let Some(parent_team_id) = team.parent_team_id.as_deref() else {
            continue;
        };
        let parent = teams
            .get(parent_team_id)
            .ok_or_else(|| TeamTopologyError::UnknownParent {
                team_id: team.id.clone(),
                parent_team_id: parent_team_id.to_string(),
            })?;
        let host_member_id =
            team.host_member_id
                .as_deref()
                .ok_or_else(|| TeamTopologyError::MissingHost {
                    team_id: team.id.clone(),
                })?;
        if !parent.member_ids.iter().any(|id| id == host_member_id) {
            return Err(TeamTopologyError::HostNotParentMember {
                team_id: team.id.clone(),
                host_member_id: host_member_id.to_string(),
                parent_team_id: parent_team_id.to_string(),
            });
        }
    }
    let mut host_claims: BTreeMap<&str, &str> = BTreeMap::new();
    for team in teams.values() {
        let Some(host_member_id) = team.host_member_id.as_deref() else {
            continue;
        };
        if let Some(first_team_id) = host_claims.insert(host_member_id, team.id.as_str()) {
            return Err(TeamTopologyError::MultipleHostedTeams {
                host_member_id: host_member_id.to_string(),
                first_team_id: first_team_id.to_string(),
                second_team_id: team.id.clone(),
            });
        }
    }
    for team in teams.values() {
        let mut visited = BTreeSet::new();
        let mut cursor = team.id.as_str();
        while let Some(parent_team_id) = teams
            .get(cursor)
            .and_then(|current| current.parent_team_id.as_deref())
        {
            if !visited.insert(cursor) {
                return Err(TeamTopologyError::Cycle {
                    team_id: cursor.to_string(),
                });
            }
            cursor = parent_team_id;
        }
    }
    Ok(())
}

/// Direct child AgentTeam ids of `team_id`, sorted for deterministic reads.
pub fn child_team_ids(teams: &BTreeMap<String, AgentTeam>, team_id: &str) -> Vec<String> {
    teams
        .values()
        .filter(|team| team.parent_team_id.as_deref() == Some(team_id))
        .map(|team| team.id.clone())
        .collect()
}

/// `root_team_id` plus every descendant team id, depth-first with sorted
/// children, so Organization tree renders share one deterministic order.
/// Returns an empty vec when `root_team_id` does not resolve.
pub fn team_subtree_ids(teams: &BTreeMap<String, AgentTeam>, root_team_id: &str) -> Vec<String> {
    if !teams.contains_key(root_team_id) {
        return Vec::new();
    }
    let mut ordered = Vec::new();
    let mut stack = vec![root_team_id.to_string()];
    while let Some(team_id) = stack.pop() {
        ordered.push(team_id.clone());
        let mut children = child_team_ids(teams, &team_id);
        children.reverse();
        stack.extend(children);
    }
    ordered
}

/// Ancestor chain of `team_id`, parent first up to the root. Stops before an
/// unresolved parent so partial compatibility graphs still read; cycle-free
/// input is guaranteed by [`validate_agent_team_topology`].
pub fn team_ancestor_ids(teams: &BTreeMap<String, AgentTeam>, team_id: &str) -> Vec<String> {
    let mut ancestors = Vec::new();
    let mut cursor = team_id;
    while let Some(parent_team_id) = teams
        .get(cursor)
        .and_then(|current| current.parent_team_id.as_deref())
    {
        ancestors.push(parent_team_id.to_string());
        cursor = parent_team_id;
    }
    ancestors
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    /// Standing-agent inbox ownership. Agent Team ownership uses `Work` and
    /// never maps this variant into `TeamMessageKind`.
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
pub enum ProviderExecutionStatus {
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
    /// Harness-owned id of this delivery attempt. It coordinates claim/control
    /// lifecycle and is not a provider session id.
    #[serde(default)]
    pub delivery_id: Option<String>,
    #[serde(default)]
    pub execution_status: Option<ProviderExecutionStatus>,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
    /// Harness-owned start time for this delivery attempt. Provider-native
    /// session timestamps remain in the provider's own store.
    #[serde(default)]
    pub started_at: Option<String>,
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

// ---------------------------------------------------------------------------
// Project Binding identity (pure layer, no I/O).
//
// Native Project Binding identity is independent from Execution Space storage.
// `ProjectContext` remains a compatibility adapter for project-derived stores.
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

/// Transitional resolved-project adapter. `project_root` has native Project
/// Binding semantics. `store_root` is only the legacy project-derived
/// compatibility-store locator; native coordination belongs to Execution Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContext {
    pub id: String,
    pub project_root: std::path::PathBuf,
    pub store_root: std::path::PathBuf,
    pub kind: ProjectKind,
    pub is_git_repo: bool,
}

/// A provider workspace/configuration boundary.
///
/// Unlike the transitional [`ProjectContext`], a Project Binding never owns an
/// execution store. It says where a provider may run and which repository,
/// instruction, Skill-discovery, permission, and worktree boundary applies.
/// Mission/Wave/Agent Team/Workflow records belong to an independent Execution
/// Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBinding {
    pub id: String,
    pub project_root: std::path::PathBuf,
    pub kind: ProjectKind,
    pub is_git_repo: bool,
    #[serde(default)]
    pub repository_url: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub git_common_dir: Option<std::path::PathBuf>,
    /// Canonical directory above which project instruction discovery must not
    /// be inferred by Harness. Providers still apply their native discovery
    /// rules inside the selected cwd; Harness does not copy instruction text.
    pub instruction_boundary: std::path::PathBuf,
    /// Canonical directory that defines project-local Skill discovery. The
    /// actual effective Skill list remains provider-native and version-specific.
    pub skill_discovery_boundary: std::path::PathBuf,
    /// `same_git_common_dir` for Git bindings and `within_project_root` for
    /// ordinary directory bindings.
    pub worktree_policy: String,
    /// Named policy snapshot used when validating workspace overrides.
    pub permission_policy: String,
}

/// A provider-neutral coordination namespace. The CLI registry supplies the
/// physical store root; this native identity is intentionally independent from
/// Company Store and Project Binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionSpace {
    pub id: String,
    pub name: String,
    pub store_root: std::path::PathBuf,
    #[serde(default)]
    pub default_project_binding_id: Option<String>,
    #[serde(default)]
    pub company_id: Option<String>,
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

/// The centralized store root for a project id, under a harness home (`~/.firm`):
/// `<firm_home>/projects/<id>`.
pub fn project_store_root(firm_home: &std::path::Path, id: &str) -> std::path::PathBuf {
    firm_home.join("projects").join(id)
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

/// Durable bridge from a stable Agent identity Inbox message into one concrete
/// Agent Team MemberRun. The source Message is retained as identity-level
/// truth; the routed TeamMessage owns runtime delivery and correlation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessageRoute {
    pub id: String,
    pub agent_message_id: String,
    pub agent_member_id: String,
    pub team_run_id: String,
    pub member_run_id: String,
    pub team_message_id: String,
    pub routed_at: String,
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

/// Evidence attached to a Work acceptance by the Host.
///
/// The Host must provide at least one of `pr_url`, `verification_output`, or
/// `screenshot_path` to prove delivery before accepting a Work. The
/// `bypass_reason` field is set only when the Host explicitly overrides this
/// requirement via `--bypass-evidence`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkAcceptanceEvidence {
    /// URL of the merged/approved pull request that delivered the Work.
    #[serde(default)]
    pub pr_url: Option<String>,
    /// Free-form verification output (e.g. command stdout proving success).
    #[serde(default)]
    pub verification_output: Option<String>,
    /// Path to a screenshot or image file that proves delivery.
    #[serde(default)]
    pub screenshot_path: Option<String>,
    /// If set, evidence requirement was bypassed. Stores the mandatory reason.
    #[serde(default)]
    pub bypass_reason: Option<String>,
}

impl WorkAcceptanceEvidence {
    /// Requires at least one evidence ref or a bypass reason.
    pub fn validate(&self) -> Result<(), String> {
        if self.bypass_reason.is_some() {
            return Ok(());
        }
        if self.pr_url.is_none()
            && self.verification_output.is_none()
            && self.screenshot_path.is_none()
        {
            return Err(
                "evidence: at least one of --pr-url, --verification-output, or --screenshot-path \
                 is required; use --bypass-evidence --reason <reason> for non-code deliverables"
                    .to_string(),
            );
        }
        Ok(())
    }

    pub fn is_bypass(&self) -> bool {
        self.bypass_reason.is_some()
    }
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
// A Mission owns durable intent, context, linked independent Teams, and
// outcome. Each Wave is one versioned Host plan/judgment memo. Execution
// records remain independently addressable and are related through Mission,
// assignment messages, correlations, and optional origin_wave_id.
// ---------------------------------------------------------------------------

/// Lifecycle of a [`Mission`]. Execution progress belongs to the selected
/// TeamRun, WorkflowRun, Host, and provider-native sessions—not to a Wave.
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
    /// Durable Markdown brief used by the Host when planning and revising
    /// Waves. Older rows deserialize as an empty brief.
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub desired_outcome: Option<String>,
    #[serde(default)]
    pub status: MissionStatus,
    /// Ordered Wave identities. Wave rows remain their own append-only ledger;
    /// this is a convenient explicit membership projection, not a replacement
    /// for reading the Wave ledger by `mission_id`.
    #[serde(default)]
    pub wave_ids: Vec<String>,
    /// Independent reusable AgentTeam definitions available to this Mission.
    /// Linking does not transfer ownership or couple team lifecycle to Mission
    /// closeout.
    #[serde(default)]
    pub agent_team_ids: Vec<String>,
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

/// Compatibility/projection hint retained on [`Wave`] rows. New Host-plan
/// Waves default to `Host`; they do not own the TeamRun, WorkflowRun, or native
/// session that informed the Host's plan. This enum intentionally has no
/// task-graph variant.
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

/// One ordered, versioned Host plan/judgment memo in a Mission. A Wave has no
/// task graph, runtime children, synchronization barrier, or session lifecycle.
/// `executor_run_ids` and `accepted_run_id` remain only for reading historical
/// direct-executor Wave rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wave {
    pub id: String,
    pub mission_id: String,
    pub index: u32,
    pub title: String,
    pub objective: String,
    /// Versioned Markdown operational memo: the Host's current plan, judgment,
    /// assignments, carry-over, and important deviations.
    #[serde(default)]
    pub context: String,
    /// Monotonic revision within this Wave id. Append-only Wave rows retain the
    /// prior revisions.
    #[serde(default)]
    pub revision: u32,
    /// Actor that authored the latest revision.
    #[serde(default)]
    pub updated_by: Option<String>,
    #[serde(default)]
    pub exit_criteria: Option<String>,
    #[serde(default)]
    pub status: WaveStatus,
    /// Historical direct-executor hint; new authoring uses `Host`.
    pub executor_kind: WaveExecutorKind,
    /// Historical direct-executor attempt references.
    #[serde(default)]
    pub executor_run_ids: Vec<String>,
    /// Historical accepted direct-executor attempt.
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
    #[error("{field} is invalid: {reason}")]
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
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

impl Validate for DurableAgentMember {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "DurableAgentMember.id")?;
        require_non_empty(&self.name, "DurableAgentMember.name")?;
        require_non_empty(&self.description, "DurableAgentMember.description")?;
        require_non_empty(&self.role, "DurableAgentMember.role")?;
        require_non_empty(&self.created_at, "DurableAgentMember.created_at")?;
        require_non_empty(&self.updated_at, "DurableAgentMember.updated_at")
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

impl Validate for AgentMessageRoute {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "AgentMessageRoute.id")?;
        require_non_empty(&self.agent_message_id, "AgentMessageRoute.agent_message_id")?;
        require_non_empty(&self.agent_member_id, "AgentMessageRoute.agent_member_id")?;
        require_non_empty(&self.team_run_id, "AgentMessageRoute.team_run_id")?;
        require_non_empty(&self.member_run_id, "AgentMessageRoute.member_run_id")?;
        require_non_empty(&self.team_message_id, "AgentMessageRoute.team_message_id")?;
        require_non_empty(&self.routed_at, "AgentMessageRoute.routed_at")
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
// Mission Log (ADR 0051)
//
// Mission absorbs Wave as an append-only Mission Log. A MissionLogEntry is
// one immutable, monotonically revisioned Markdown record of Host judgment,
// re-plan, recovery narration, or closeout evidence. Unlike Wave it has no
// lifecycle, gate, or "advance" operation — there is nothing to accept or
// reject, only entries to append and read. The Log is required reading, not
// optional narration: the recovery entrypoint and session re-entry injection
// are mandatory readers of its tail so a Host (or its replacement) resumes
// from durable judgment instead of re-deriving intent from provider-native
// state that a compaction can destroy.
// ---------------------------------------------------------------------------

/// The nature of one [`MissionLogEntry`]. There is deliberately no variant for
/// routine narration — every entry is one of these four material kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionLogEntryKind {
    /// A Host decision at a material point: a new Work tranche, a composition
    /// change, or a model/provider switch.
    Judgment,
    /// A material change to the Host's plan since the previous entry.
    Replan,
    /// Narration written while recovering a Mission, TeamRun, or Host session.
    Recovery,
    /// The evidence or outcome that justifies Mission closeout.
    CloseoutEvidence,
}

/// One immutable, append-only Mission Log row (ADR 0051). `revision` is
/// monotonic per `mission_id` and store-assigned, exactly like `Wave.index`;
/// callers never choose it. There is no `updated_at` because a
/// [`MissionLogEntry`] is never revised in place — a correction is a new
/// entry, not a mutation of an old one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionLogEntry {
    pub id: String,
    pub mission_id: String,
    pub revision: u32,
    pub kind: MissionLogEntryKind,
    /// Markdown body. Must be non-empty: an append-only judgment log with a
    /// blank entry is indistinguishable from Wave's write-only failure.
    pub body: String,
    /// The actor that authored this entry (a Host identity, "host", or an
    /// explicit operator/agent id).
    pub actor: String,
    pub created_at: String,
}

impl Validate for MissionLogEntry {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "MissionLogEntry.id")?;
        require_non_empty(&self.mission_id, "MissionLogEntry.mission_id")?;
        require_non_empty(&self.body, "MissionLogEntry.body")?;
        require_non_empty(&self.actor, "MissionLogEntry.actor")?;
        require_non_empty(&self.created_at, "MissionLogEntry.created_at")
    }
}

// ---------------------------------------------------------------------------
// Dynamic workflow runtime objects (WP1)
//
// A `WorkflowRun` is a standalone object with its own id and lifecycle. Each
// `WorkflowStep` is the workflow-layer wrapper around one `agent()` call and references the
// provider-owned native session rather than re-recording the execution. Both
// journal to their own append-only JSONL with latest-wins
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
    /// Project Binding that owns provider cwd, repository instructions, Skills,
    /// Git/worktree policy, and delivery paths for this run. The surrounding
    /// Execution Space owns this row but never substitutes for a workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_binding_id: Option<String>,
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

/// One agent step inside a [`WorkflowRun`]. `phase` is the declarative grouping
/// marker (e.g. "audit", "synthesize"); `label` names the step within the phase.
/// `native_session` links to the provider-owned execution record. Harness keeps
/// the Workflow outcome and evidence here, but never mirrors the provider turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub run_id: String,
    pub phase: String,
    pub label: String,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
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
        if let Some(binding) = &self.project_binding_id {
            require_non_empty(binding, "WorkflowRun.project_binding_id")?;
        }
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
    /// Stable independent AgentTeam definition used by this run. New writes
    /// use this field; `definition_id` remains a transitional alias for older
    /// rows.
    #[serde(default)]
    pub agent_team_id: Option<String>,
    #[serde(default)]
    pub previous_run_id: Option<String>,
    /// Optional outer product identity (ADR 0026). Existing v0 team-run rows
    /// remain readable without these joins during the staged migration.
    #[serde(default)]
    pub mission_id: Option<String>,
    #[serde(default)]
    pub wave_id: Option<String>,
    /// Optional execution-resource binding selected for this run. This relation
    /// does not transfer Mission/Team ownership to the project.
    #[serde(default)]
    pub project_binding_id: Option<String>,
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

/// Durable ownership record for the one process/service allowed to control a
/// TeamRun's provider-native sessions. Latest row wins by `team_run_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamSupervisorLease {
    pub team_run_id: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMemberCloseStatus {
    Pending,
    Applied,
}

/// Durable Host request to end one MemberRun runtime. The owning Supervisor
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

/// Lifecycle of a [`MemberRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRunStatus {
    Starting,
    Idle,
    Queued,
    Running,
    Waiting,
    /// The durable MemberRun and native-session binding still exist, but the
    /// Supervisor currently has no healthy provider transport. This is
    /// recoverable and intentionally distinct from `Failed` or `Stopped`.
    Disconnected,
    Reviewing,
    Blocked,
    Completed,
    Failed,
    Stopped,
}

/// Durable coordination lifecycle of one MemberRun, separate from its
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

/// Whether a MemberRun may claim and consume its Assignment.
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
pub struct MemberRun {
    pub id: String,
    pub team_run_id: String,
    #[serde(default)]
    pub slot_id: Option<String>,
    /// Optional stable link to [`DurableAgentMember`]. Absence means this
    /// remains a temporary execution participant; callers must never infer the
    /// link from display fields, provider sessions, or the compatibility
    /// [`AgentMember`] registry.
    #[serde(default)]
    pub agent_member_id: Option<String>,
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
    /// Durable mailbox/participation state, independent of the process state
    /// represented by `status`.
    #[serde(default)]
    pub coordination_status: MemberCoordinationStatus,
    /// Monotonic activation generation. Explicit Reopen increments this so a
    /// live Supervisor can start a new process for the same MemberRun id.
    #[serde(default = "default_member_runtime_generation")]
    pub runtime_generation: u64,
    pub status: MemberRunStatus,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
    #[serde(default)]
    pub worktree_ref: Option<String>,
    /// Facts actually observed from the spawned member's working directory and
    /// non-secret instruction/skill roots discovered from that environment.
    #[serde(default)]
    pub workspace_snapshot: Option<MemberWorkspaceSnapshot>,
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

impl MemberRun {
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
        require_non_empty(&self.host_surface, "AgentTeamRun.host_surface")?;
        require_non_empty(&self.objective, "AgentTeamRun.objective")?;
        require_non_empty(&self.created_at, "AgentTeamRun.created_at")?;
        require_non_empty(&self.updated_at, "AgentTeamRun.updated_at")?;
        if let Some(execution_root) = &self.execution_root {
            require_non_empty(execution_root, "AgentTeamRun.execution_root")?;
        }
        if let Some(binding) = &self.project_binding_id {
            require_non_empty(binding, "AgentTeamRun.project_binding_id")?;
        }
        if let Some(actor) = &self.host_actor {
            require_non_empty(&actor.id, "AgentTeamRun.host_actor.id")?;
        }
        Ok(())
    }
}

impl Validate for TeamSupervisorLease {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.team_run_id, "TeamSupervisorLease.team_run_id")?;
        require_non_empty(&self.supervisor_id, "TeamSupervisorLease.supervisor_id")?;
        require_non_empty(&self.owner_locator, "TeamSupervisorLease.owner_locator")
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

impl Validate for MemberRun {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "MemberRun.id")?;
        require_non_empty(&self.team_run_id, "MemberRun.team_run_id")?;
        if let Some(agent_member_id) = &self.agent_member_id {
            require_non_empty(agent_member_id, "MemberRun.agent_member_id")?;
        }
        require_non_empty(&self.name, "MemberRun.name")?;
        require_non_empty(&self.role, "MemberRun.role")?;
        require_non_empty(&self.provider, "MemberRun.provider")?;
        require_non_empty(&self.started_at, "MemberRun.started_at")?;
        if self.runtime_generation == 0 {
            return Err(ValidationError::Invalid {
                field: "MemberRun.runtime_generation",
                reason: "must be at least 1",
            });
        }
        if let Some(worktree_ref) = &self.worktree_ref {
            require_non_empty(worktree_ref, "MemberRun.worktree_ref")?;
        }
        if let Some(snapshot) = &self.workspace_snapshot {
            snapshot.validate()?;
        }
        Ok(())
    }
}

/// Execution mode of a declared non-driven member: the user's own
/// already-open interactive provider CLI session (Kimi Code, Codex, or Claude
/// Code), which Harness never spawns or drives. The session polls its Harness
/// inbox and replies through the trusted loopback CLI/MCP; there is no
/// provider-native session record, so evidence claims about this member's
/// work cannot resolve to provider-native execution truth.
pub const EXECUTION_MODE_EXTERNAL_INTERACTIVE: &str = "external_interactive";

/// How one provider member is executed by Harness. Capability claims are
/// mode-specific: `codex_exec` and `kimi_acp` are different products even when
/// their user-facing provider names are simply Codex and Kimi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderIntegrationProfile {
    pub provider: String,
    pub execution_mode: String,
    /// The exclusive owner allowed to start top-level provider execution
    /// cycles for this MemberRun. Agent Team modes currently default to
    /// Harness-owned mailbox delivery; provider-owned continuation must be
    /// reviewed explicitly before it can be selected.
    #[serde(default)]
    pub execution_driver: MemberExecutionDriver,
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
    /// When ordinary queued TeamMessages become visible to this live mode.
    /// Provider-native records remain the execution/transcript authority.
    #[serde(default)]
    pub ordinary_message_boundary: OrdinaryMessageBoundary,
    /// How this exact execution mode implements Member plan negotiation.
    #[serde(default)]
    pub plan_mode: ProviderFeatureMode,
    /// Whether the provider exposes a native session Goal that can mirror the
    /// Harness Assignment objective. Assignment remains canonical either way.
    #[serde(default)]
    pub goal_mode: ProviderFeatureMode,
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
pub enum OrdinaryMessageBoundary {
    InTurn,
    NextRound,
    NextRoundBatched,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberExecutionDriver {
    #[default]
    HostDriven,
    ProviderDriven,
    /// Declared external interactive members only: the human drives their own
    /// already-open provider session out-of-band. Harness never starts a
    /// provider cycle for this member and no native continuation loop exists.
    UserDriven,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFeatureMode {
    Native,
    Emulated,
    Unsupported,
    #[default]
    Unknown,
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
    /// Ordinary correlated conversation. Planning requests and responses use
    /// this kind; providers may use native planning internally, but Harness
    /// does not create a Plan lifecycle or gate.
    Message,
    /// Historical compatibility labels. They remain readable but have no
    /// special validation, permission, or runtime semantics after ADR 0039.
    PlanRequest,
    PlanProposal,
    PlanFeedback,
    PlanApproval,
    /// Historical intent labels; new writes use `Message` with a readable
    /// first-line intent such as QUESTION, PROGRESS, or BLOCKER.
    Question,
    Answer,
    Progress,
    Blocker,
    /// Explicit completion proposal with outcome/evidence for Host review.
    Handoff,
    /// Historical review intent labels; new writes use `Message`.
    ReviewRequest,
    ReviewResult,
    /// A real runtime control record, not ordinary chat.
    Control,
    /// Historical fan-out label; new writes use one `Message` with multiple
    /// recipients.
    Broadcast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamActorKind {
    Host,
    MemberRun,
    AgentMember,
    Operator,
    Service,
}

/// Authorship provenance for a coordination message. `authn_source` names the
/// trusted local connection or gateway that selected the actor; it never
/// contains a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamActorRef {
    pub kind: TeamActorKind,
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub authn_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRecipientKind {
    Host,
    MemberRun,
    AgentMember,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRecipientRef {
    pub kind: TeamRecipientKind,
    pub id: String,
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
    Claimed,
    Delivered,
    Acknowledged,
    Failed,
    Expired,
}

/// Explicit response intent carried by a [`TeamMessage`] (ADR 0046 §4). A
/// transport delivery and a semantic reply are distinct facts: mail that only
/// informs or acknowledges must stay durable and correlated without starting
/// another provider round, so two Agents can converge instead of bouncing
/// acknowledgement-only mail back and forth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageResponseIntent {
    /// Durable, correlated mail that does not by itself start a provider
    /// round. It is batched into the next round some response-required
    /// message triggers, and it never fences a same-correlation Handoff.
    Informational,
    /// The sender asks for a semantic reply: an idle recipient starts a new
    /// provider round for this message, and a pending delivery fences a
    /// same-correlation Handoff as stale.
    ResponseRequired,
}

/// One recipient's delivery record inside a [`TeamMessage`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMessageDelivery {
    pub member_id: String,
    pub policy: TeamDeliveryPolicy,
    pub status: TeamDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_by_supervisor_id: Option<String>,
    #[serde(default)]
    pub claimed_generation: Option<u64>,
    #[serde(default)]
    pub claimed_unix_ms: Option<u64>,
    #[serde(default)]
    pub claim_expires_unix_ms: Option<u64>,
    /// Provider-native turn/request id returned after the selected protocol
    /// accepted this content. Absence on a claimed delivery is intentionally
    /// treated as uncertain after a Supervisor crash.
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    /// Why this delivery failed. Only set when status is
    /// [`TeamDeliveryStatus::Failed`].
    #[serde(default)]
    pub failure_reason: Option<String>,
    pub updated_at: String,
}

/// A routed message inside an [`AgentTeamRun`]. `from_member_id` is either the
/// reserved `"host"` id or a `MemberRun` id. `correlation_id` groups a message
/// with its replies; `causation_id` points at the message this one answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMessage {
    pub id: String,
    pub team_run_id: String,
    /// Optional Work discussed by this message. The relation is navigational
    /// and conversational only: ownership and lifecycle remain authoritative
    /// on `Work`/`WorkEvent`, never on the message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_id: Option<String>,
    /// Optional Host-plan Wave that explains why this message was authored.
    /// It is navigation metadata only and never controls message or member
    /// lifecycle.
    #[serde(default)]
    pub origin_wave_id: Option<String>,
    /// Typed provenance for new writes. Historical rows infer it from
    /// `from_member_id`.
    #[serde(default)]
    pub sender: Option<TeamActorRef>,
    pub from_member_id: String,
    /// Typed recipients for new writes. `to_member_ids` remains the historical
    /// TeamRun projection.
    #[serde(default)]
    pub recipients: Vec<TeamRecipientRef>,
    #[serde(default)]
    pub to_member_ids: Vec<String>,
    pub kind: TeamMessageKind,
    pub body: String,
    pub correlation_id: String,
    #[serde(default)]
    pub causation_id: Option<String>,
    /// Explicit response intent. Absent on historical rows; the effective
    /// intent then derives from `kind` (see
    /// [`TeamMessage::effective_response_intent`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_intent: Option<TeamMessageResponseIntent>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub deliveries: Vec<TeamMessageDelivery>,
    pub created_at: String,
}

impl TeamMessage {
    /// Effective response intent: the explicit field always wins; otherwise
    /// kind **and sender** decide (ADR 0046 §4).
    ///
    /// Handoffs and Control records carry real review or runtime semantics and
    /// always require a response round regardless of sender. Durable work
    /// ownership lives in `Work`; messages never impersonate assignments.
    ///
    /// Ordinary `message` mail is sender-aware, because `message` is the only
    /// legal carrier for every remaining semantic category after ADR 0039
    /// retired the typed question/blocker/review kinds:
    /// - a coordination-plane sender (Host, Operator, Service) is directing the
    ///   member — questions, revisions, acceptance decisions — so it defaults to
    ///   `response_required` and wakes an idle member;
    /// - a peer member sender is confirming or informing another member, so it
    ///   defaults to `informational` and the team converges without
    ///   confirmation ping-pong.
    pub fn effective_response_intent(&self) -> TeamMessageResponseIntent {
        if let Some(intent) = self.response_intent {
            return intent;
        }
        match self.kind {
            TeamMessageKind::Handoff | TeamMessageKind::Control => {
                TeamMessageResponseIntent::ResponseRequired
            }
            _ if self.sent_by_peer_member() => TeamMessageResponseIntent::Informational,
            _ => TeamMessageResponseIntent::ResponseRequired,
        }
    }

    /// True when this message was authored by another team member rather than
    /// by the coordination plane (Host, Operator, Service). Historical rows
    /// carry no typed `sender`, so they fall back to the reserved `"host"`
    /// `from_member_id` convention.
    fn sent_by_peer_member(&self) -> bool {
        match self.sender.as_ref().map(|sender| sender.kind) {
            Some(TeamActorKind::MemberRun) | Some(TeamActorKind::AgentMember) => true,
            Some(TeamActorKind::Host)
            | Some(TeamActorKind::Operator)
            | Some(TeamActorKind::Service) => false,
            None => self.from_member_id != "host",
        }
    }

    /// True when this message may trigger a new provider round for an idle
    /// recipient and fences a same-correlation Handoff while still pending.
    pub fn requires_response(&self) -> bool {
        self.effective_response_intent() == TeamMessageResponseIntent::ResponseRequired
    }
}

/// Agent Team Work is durable responsibility inside one AgentTeam. A
/// `team_run_id` is the current execution attempt, not the Work's lifetime.
/// WorkEvent is the append-only authority; this row is the latest rebuildable
/// projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Open,
    InProgress,
    Blocked,
    Review,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkClaimMode {
    HostAssign,
    TeamClaim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPriority {
    Low,
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRef {
    pub team_run_id: String,
    pub work_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCausationRef {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCommandContext {
    pub event_id: String,
    pub performed_by_actor: TeamActorRef,
    #[serde(default)]
    pub authority_actor: Option<TeamActorRef>,
    #[serde(default)]
    pub causation_ref: Option<WorkCausationRef>,
    pub idempotency_key: String,
    pub created_at: String,
    /// When true, skip the duplicate-title guard (recovery flows reuse existing
    /// Work ids; explicit creation of a same-title Work is opt-in).
    #[serde(default)]
    pub duplicate_ok: bool,
}

/// Where a Work executes. The harness creates the workspace before the first
/// member start, injects it as the member's cwd, and cleans it up on Work
/// completion or cancellation (when `auto_cleanup` is true).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkWorkspaceKind {
    /// A git worktree: isolated checkout with its own branch. Required for
    /// code-producing Work where parallel members need disjoint paths.
    Worktree,
    /// A plain directory (no git isolation). For exploration, research, or
    /// single-file documentation work.
    Dir,
    /// The project root. For read-only analysis or ops work that doesn't need
    /// isolation. Member's cwd is the project root.
    Inherit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkWorkspace {
    pub kind: WorkWorkspaceKind,
    /// Absolute or project-relative path. For worktrees, this is OUTSIDE the
    /// main repository (e.g. "../repo-feat-login").
    pub path: String,
    /// For worktrees: the base ref to branch from (e.g. "origin/master").
    #[serde(default)]
    pub base_ref: Option<String>,
    /// Whether the workspace should be removed after Work completes.
    #[serde(default = "default_workspace_auto_cleanup")]
    pub auto_cleanup: bool,
}

fn default_workspace_auto_cleanup() -> bool {
    true
}

/// Kind of GitHub object a [`Work`] is linked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitHubLinkKind {
    Issue,
    PullRequest,
}

/// A GitHub issue/PR link attached to a [`Work`] by
/// `work create --github-issue` / `work submit --github-pr`.
///
/// The link is a durable snapshot: `status`/`ci_status`/`ci_url` are captured
/// from the GitHub API (via the `gh` CLI) at link time and never silently
/// re-synced, so a stored link states the observation made when it was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubLink {
    pub kind: GitHubLinkKind,
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub url: String,
    /// GitHub object state at snapshot time: `OPEN`/`CLOSED` for issues,
    /// `OPEN`/`CLOSED`/`MERGED` for pull requests.
    #[serde(default)]
    pub status: Option<String>,
    /// PR CI outcome at snapshot time: `success`, `failure`, `pending`, or
    /// `unknown` when no checks are reported for the PR.
    #[serde(default)]
    pub ci_status: Option<String>,
    /// Link to the PR checks page / the check that determined `ci_status`.
    #[serde(default)]
    pub ci_url: Option<String>,
}

/// A declared verification gate for a [`Work`]. Each gate is an independent
/// check that must pass before the Work can be accepted. Gates are composable:
/// a Work with zero gates preserves today's manual-accept behaviour; a Work
/// with several gates must satisfy all of them.
///
/// The `plugin` field names a registered gate implementation. The `config`
/// payload is plugin-specific (e.g. `{"require_merged": true}` for github-pr).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSpec {
    /// Gate plugin identifier: "github-pr" | "code-review" | "check-pass"
    /// | "artifact-exists" | "owned-path-check" | "goal-design"
    pub plugin: String,
    /// Plugin-specific configuration (free JSON).
    #[serde(default)]
    pub config: serde_json::Value,
}

/// Result of evaluating a single [`GateSpec`] against a Work and its
/// delivery/evidence context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    /// The gate is satisfied.
    Pass,
    /// The gate is not satisfied (the Work must be revised).
    Fail { reason: String },
    /// A prerequisite is not yet met (e.g. PR not opened); the Work is stuck,
    /// not failed.
    Blocked { reason: String },
}

impl GateVerdict {
    pub fn is_pass(&self) -> bool {
        matches!(self, GateVerdict::Pass)
    }
}

/// The result of evaluating one [`GateSpec`] against a Work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    pub gate: GateSpec,
    pub verdict: GateVerdict,
}

/// A pluggable registry of gate evaluation functions. Built-in gates are
/// pre-registered via [`GateRegistry::default`]; external code can register
/// custom gates with [`GateRegistry::register`].
///
/// Each registered function receives the full context available to the
/// engine: the gate spec, the work, and any reviews.
pub struct GateRegistry {
    gates: std::collections::HashMap<String, Box<dyn Fn(&GateSpec, &Work, &[Review]) -> GateVerdict>>,
}

impl GateRegistry {
    /// Create an empty registry (useful for testing or embedders that want
    /// full control over registration). Prefer [`GateRegistry::default`] for
    /// the standard built-in set.
    pub fn new() -> Self {
        Self {
            gates: std::collections::HashMap::new(),
        }
    }

    /// Register a custom gate plugin. If a gate with the same name already
    /// exists it is replaced.
    pub fn register<F>(&mut self, plugin: &str, gate: F)
    where
        F: Fn(&GateSpec, &Work, &[Review]) -> GateVerdict + 'static,
    {
        self.gates.insert(plugin.to_string(), Box::new(gate));
    }

    /// Evaluate a single gate spec, dispatching to the registered function.
    /// Returns `Fail` with a descriptive message when the plugin is unknown.
    pub fn evaluate(
        &self,
        gate: &GateSpec,
        work: &Work,
        reviews: &[Review],
    ) -> GateVerdict {
        match self.gates.get(gate.plugin.as_str()) {
            Some(f) => f(gate, work, reviews),
            None => GateVerdict::Fail {
                reason: format!("unknown gate plugin: {}", gate.plugin),
            },
        }
    }
}

impl Default for GateRegistry {
    fn default() -> Self {
        let mut registry = Self::new();
        registry.register("github-pr", |gate, work, _reviews| {
            GateEngine::evaluate_github_pr(gate, work)
        });
        registry.register("code-review", |gate, work, reviews| {
            GateEngine::evaluate_code_review(gate, work, reviews)
        });
        registry.register("artifact-exists", |gate, work, _reviews| {
            GateEngine::evaluate_artifact_exists(gate, work)
        });
        registry.register("check-pass", |gate, work, _reviews| {
            GateEngine::evaluate_check_pass(gate, work)
        });
        registry
    }
}

/// Stateless engine that evaluates a Work's declared [`GateSpec`]s. Uses
/// a [`GateRegistry`] for dispatch; the default registry includes all
/// built-in gates. Embedders that register custom gates should create a
/// custom registry and pass it to the engine methods that accept one.
///
/// The engine remains stateless — all gate evaluation functions are pure
/// (they only read the Work, GateSpec, and optionally Review records).
pub struct GateEngine;

impl GateEngine {
    /// Evaluate every gate declared on `work` using the default built-in
    /// gate set.
    pub fn evaluate_work_gates(work: &Work) -> Vec<GateResult> {
        Self::evaluate_work_gates_with_reviews(work, &[])
    }

    /// Evaluate every gate with access to [`Review`] records.
    pub fn evaluate_work_gates_with_reviews(work: &Work, reviews: &[Review]) -> Vec<GateResult> {
        Self::evaluate_work_gates_with_registry(work, reviews, &GateRegistry::default())
    }

    /// Evaluate every gate using a custom registry. External code that
    /// registered additional gates should use this entry point.
    pub fn evaluate_work_gates_with_registry(
        work: &Work,
        reviews: &[Review],
        registry: &GateRegistry,
    ) -> Vec<GateResult> {
        work.gates
            .iter()
            .map(|gate| GateResult {
                verdict: registry.evaluate(gate, work, reviews),
                gate: gate.clone(),
            })
            .collect()
    }

    // ── Built-in gate implementations (pub so registry can reference) ─

    fn evaluate_github_pr(gate: &GateSpec, work: &Work) -> GateVerdict {
        let require_merged = gate
            .config
            .get("require_merged")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let require_ci_pass = gate
            .config
            .get("require_ci_pass")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let pr_link = work
            .github_links
            .iter()
            .find(|link| link.kind == GitHubLinkKind::PullRequest);

        let Some(pr_link) = pr_link else {
            return GateVerdict::Blocked {
                reason: "no GitHub pull request linked to this work".to_string(),
            };
        };

        if require_merged {
            match pr_link.status.as_deref() {
                Some("MERGED") => {} // ok
                None => {
                    return GateVerdict::Blocked {
                        reason: format!(
                            "PR {}/{}#{} has unknown merge status (run `work poll-github-ci` to refresh)",
                            pr_link.owner, pr_link.repo, pr_link.number
                        ),
                    };
                }
                Some(other) => {
                    return GateVerdict::Blocked {
                        reason: format!(
                            "PR {}/{}#{} is not merged (status: {other})",
                            pr_link.owner, pr_link.repo, pr_link.number
                        ),
                    };
                }
            }
        }

        if require_ci_pass {
            match pr_link.ci_status.as_deref() {
                Some("success") => {} // ok
                Some("failure") => {
                    return GateVerdict::Fail {
                        reason: format!(
                            "PR {}/{}#{} CI checks failed",
                            pr_link.owner, pr_link.repo, pr_link.number
                        ),
                    };
                }
                Some("pending") | None => {
                    return GateVerdict::Blocked {
                        reason: format!(
                            "PR {}/{}#{} CI checks not yet complete (run `work poll-github-ci` to refresh)",
                            pr_link.owner, pr_link.repo, pr_link.number
                        ),
                    };
                }
                _ => {
                    return GateVerdict::Blocked {
                        reason: format!(
                            "PR {}/{}#{} CI status unknown: {:?}",
                            pr_link.owner, pr_link.repo, pr_link.number, pr_link.ci_status
                        ),
                    };
                }
            }
        }

        GateVerdict::Pass
    }

    // ── code-review gate ────────────────────────────────────────────

    fn evaluate_code_review(
        _gate: &GateSpec,
        work: &Work,
        reviews: &[Review],
    ) -> GateVerdict {
        // Match reviews by task_id (legacy field name; for Work this is the work id).
        let matching: Vec<&Review> = reviews
            .iter()
            .filter(|r| r.task_id.as_deref() == Some(work.id.as_str()))
            .collect();

        if matching.is_empty() {
            return GateVerdict::Blocked {
                reason: "code review not yet completed (no Review record found)".to_string(),
            };
        }

        // Use the latest matching review (most recently created).
        let review = matching
            .iter()
            .max_by_key(|r| &r.created_at)
            .expect("non-empty");

        match &review.verdict {
            ReviewVerdict::Pass => GateVerdict::Pass,
            ReviewVerdict::Fail => GateVerdict::Fail {
                reason: format!(
                    "code review failed by {}: {}",
                    review.reviewer_agent_id, review.summary
                ),
            },
            ReviewVerdict::NeedsChanges => GateVerdict::Fail {
                reason: format!(
                    "code review requested changes (reviewer: {}): {}",
                    review.reviewer_agent_id, review.summary
                ),
            },
            ReviewVerdict::Blocked => GateVerdict::Blocked {
                reason: format!(
                    "code review blocked by {}: {}",
                    review.reviewer_agent_id, review.summary
                ),
            },
            ReviewVerdict::Other(label) => GateVerdict::Fail {
                reason: format!(
                    "code review returned verdict '{label}' by {}: {}",
                    review.reviewer_agent_id, review.summary
                ),
            },
        }
    }

    // ── artifact-exists gate ────────────────────────────────────────

    fn evaluate_artifact_exists(gate: &GateSpec, work: &Work) -> GateVerdict {
        // If config specifies required paths, check that all of them are
        // present in work.artifact_refs.
        if let Some(paths) = gate.config.get("paths").and_then(|v| v.as_array()) {
            if paths.is_empty() {
                return GateVerdict::Fail {
                    reason: "artifact-exists gate configured with empty paths list".to_string(),
                };
            }
            let mut missing: Vec<String> = Vec::new();
            for path in paths {
                let path_str = path.as_str().unwrap_or("");
                if path_str.is_empty() {
                    continue;
                }
                if !work.artifact_refs.iter().any(|r| r.contains(path_str)) {
                    missing.push(path_str.to_string());
                }
            }
            if !missing.is_empty() {
                return GateVerdict::Fail {
                    reason: format!(
                        "required artifacts not found: {}",
                        missing.join(", ")
                    ),
                };
            }
            return GateVerdict::Pass;
        }

        // No specific paths — check that artifact_refs is non-empty.
        if work.artifact_refs.is_empty() {
            return GateVerdict::Blocked {
                reason: "no artifacts declared (work.artifact_refs is empty)".to_string(),
            };
        }
        GateVerdict::Pass
    }

    // ── check-pass gate ─────────────────────────────────────────────

    fn evaluate_check_pass(gate: &GateSpec, work: &Work) -> GateVerdict {
        // If config specifies expected check names, verify they're present.
        if let Some(names) = gate.config.get("checks").and_then(|v| v.as_array()) {
            if names.is_empty() {
                return GateVerdict::Fail {
                    reason: "check-pass gate configured with empty checks list".to_string(),
                };
            }
            let mut missing: Vec<String> = Vec::new();
            for name in names {
                let name_str = name.as_str().unwrap_or("");
                if name_str.is_empty() {
                    continue;
                }
                if !work.check_refs.iter().any(|r| r.contains(name_str)) {
                    missing.push(name_str.to_string());
                }
            }
            if !missing.is_empty() {
                return GateVerdict::Fail {
                    reason: format!("required checks not found: {}", missing.join(", ")),
                };
            }
            return GateVerdict::Pass;
        }

        // No specific checks — just require check_refs to be non-empty.
        if work.check_refs.is_empty() {
            return GateVerdict::Blocked {
                reason: "no checks declared (work.check_refs is empty)".to_string(),
            };
        }
        GateVerdict::Pass
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    pub id: String,
    pub team_run_id: String,
    /// Durable AgentTeam scope (ADR 0052). `None` reads as a compatibility
    /// TeamRun-scoped Work written before the Team-scope promotion slice.
    /// When set, `team_run_id` names only the current execution attempt: the
    /// Work's responsibility survives that TeamRun's completion and a later
    /// execution attempt rebinds `team_run_id` without changing `team_id`.
    #[serde(default)]
    pub team_id: Option<String>,
    /// Same-TeamRun hierarchy only. Cross-Team delegation uses
    /// [`WorkDelegation`].
    #[serde(default)]
    pub parent_work_id: Option<String>,
    #[serde(default)]
    pub source_work_item_ref: Option<String>,
    pub title: String,
    pub context_markdown: String,
    pub completion_criteria_markdown: String,
    pub status: WorkStatus,
    /// Stable AgentMember/slot identity. Runtime generations bind through
    /// `active_member_run_id`.
    #[serde(default)]
    pub owner_member_id: Option<String>,
    #[serde(default)]
    pub active_member_run_id: Option<String>,
    pub claim_mode: WorkClaimMode,
    #[serde(default)]
    pub eligible_member_ids: Vec<String>,
    #[serde(default)]
    pub prerequisite_work_ids: Vec<String>,
    pub priority: WorkPriority,
    pub created_by_actor: TeamActorRef,
    /// Durable AgentMember identity of the creator (ADR 0052 provenance).
    /// `None` for Host, Supervising Operator, or external intake; populated
    /// from the bound MemberRun's stable identity when a Member creates Work.
    #[serde(default)]
    pub created_by_member_id: Option<String>,
    #[serde(default)]
    pub result_summary: Option<String>,
    #[serde(default)]
    pub blocker_reason: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub check_refs: Vec<String>,
    /// GitHub issue/PR linkage snapshot (see [`GitHubLink`]). `#[serde(default)]`
    /// keeps pre-linkage works.jsonl records readable.
    #[serde(default)]
    pub github_links: Vec<GitHubLink>,
    /// Declared verification gates this Work must pass before acceptance.
    /// Empty vec → manual-accept semantics preserved (back-compat).
    #[serde(default)]
    pub gates: Vec<GateSpec>,
    /// Evidence the Host provided when accepting this Work (issue #387 P1-2).
    /// `None` for Works accepted before this gate was enforced; `Some` after.
    #[serde(default)]
    pub acceptance_evidence: Option<WorkAcceptanceEvidence>,
    /// If set, the Host must be alerted when this Work stays in `Review` longer
    /// than this many hours. A `WorkReviewTimedOut` HostAttention is generated
    /// when the timeout is exceeded (issue #387 P1-2).
    #[serde(default)]
    pub review_timeout_hours: Option<u32>,
    /// Where this Work executes. `None` → Member inherits the project root
    /// (back-compat with today's implicit behaviour). The harness creates
    /// the workspace before first member start and cleans it up on Work
    /// completion when `auto_cleanup` is true.
    #[serde(default)]
    pub workspace: Option<WorkWorkspace>,
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
}

impl Work {
    pub fn is_terminal(&self) -> bool {
        matches!(self.status, WorkStatus::Done | WorkStatus::Cancelled)
    }

    /// Whether every declared prerequisite has reached Host-accepted `Done`.
    ///
    /// This intentionally says nothing about the Work's own lifecycle state.
    /// A delivery for a revision can be actionable while the Work is already
    /// `in_progress`, `blocked`, or `review`; only a *new claim* is restricted
    /// to an open Work.
    pub fn prerequisites_satisfied<'a>(&self, works: impl IntoIterator<Item = &'a Work>) -> bool {
        let by_id = works
            .into_iter()
            .map(|work| (work.id.as_str(), work.status))
            .collect::<std::collections::HashMap<_, _>>();
        self.prerequisite_work_ids
            .iter()
            .all(|id| by_id.get(id.as_str()) == Some(&WorkStatus::Done))
    }

    /// Whether this Work can be newly claimed from the shared Works board.
    pub fn is_claim_ready<'a>(&self, works: impl IntoIterator<Item = &'a Work>) -> bool {
        self.status == WorkStatus::Open && self.prerequisites_satisfied(works)
    }

    /// Compatibility spelling retained for existing callers. Readiness here
    /// means *claim* readiness, not delivery readiness.
    pub fn is_ready<'a>(&self, works: impl IntoIterator<Item = &'a Work>) -> bool {
        self.is_claim_ready(works)
    }

    /// Whether this Work carries a durable AgentTeam scope (ADR 0052) rather
    /// than only a compatibility TeamRun scope.
    pub fn is_team_scoped(&self) -> bool {
        self.team_id.is_some()
    }

    /// Assigned/unassigned is a derived view over `owner_member_id`, never a
    /// stored lifecycle of its own (ADR 0050/0051).
    pub fn is_assigned(&self) -> bool {
        self.owner_member_id.is_some()
    }

    pub fn is_unassigned(&self) -> bool {
        self.owner_member_id.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkEventKind {
    Created,
    Assigned,
    Claimed,
    Started,
    Released,
    Blocked,
    Resumed,
    Submitted,
    ChangesRequested,
    Accepted,
    Cancelled,
    Updated,
    Rebound,
    /// A compatibility TeamRun-scoped Work was explicitly promoted onto the
    /// durable AgentTeam named by its current execution attempt.
    TeamScopePromoted,
    /// The execution attempt (`team_run_id`) of a Team-scoped Work moved to a
    /// successor TeamRun of the same AgentTeam. Durable scope (`team_id`),
    /// owner, and provenance are unchanged (ADR 0052).
    ExecutionRetargeted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkCutoverIssueKind {
    LegacyTeamRunScope,
    MissingExecutionRun,
    TeamScopeMismatch,
    MissingCompanyWorkItem,
    ActiveCompanyWorkItemOverlap,
    DuplicateCompanyWorkItemLink,
    MissingCompanyWorkItemFence,
    ConflictingCompanyWorkItemFence,
    IncompleteCompanyWorkItemFence,
    CompanyWorkItemChangedAfterFence,
}

/// Durable one-way refusal marker written into the Company Store before a
/// source-linked Work becomes Team-scoped authority.
///
/// This is deliberately not a cross-store transaction claim. The Company
/// Store persists this marker first and thereafter refuses every revision of
/// the retired WorkItem. Only then may the Execution Store append the
/// `TeamScopePromoted` WorkOperation. A crash between those appends leaves no
/// mutable responsibility authority and an idempotent retry may finish the
/// promotion using the same marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCutoverFence {
    pub company_work_item_id: String,
    pub work_id: String,
    pub team_id: String,
    /// Event intended by the command that first installed the fence. A crash
    /// gap may allow compatibility Work versions to advance; completion may
    /// therefore use a later `TeamScopePromoted` event after a refreshed retry.
    pub promotion_event_id: String,
    pub expected_work_version: u64,
    pub company_work_item_status: WorkItemStatus,
    pub company_work_item_updated_at: String,
    /// Canonical typed snapshot of the retired WorkItem. Store APIs refuse all
    /// later revisions, and verification compares the full value so manual
    /// ledger mutation cannot hide behind an unchanged status/timestamp.
    #[serde(default)]
    pub company_work_item_snapshot: serde_json::Value,
    pub idempotency_key: String,
    pub created_at: String,
}

/// One reason the Company WorkItem -> persistent Team Work cutover cannot be
/// accepted. The validator is deliberately read-only: it proves a snapshot is
/// unambiguous but never dual-writes either responsibility system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCutoverIssue {
    pub kind: WorkCutoverIssueKind,
    #[serde(default)]
    pub work_id: Option<String>,
    #[serde(default)]
    pub company_work_item_id: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCutoverReport {
    pub valid: bool,
    pub checked_work_count: usize,
    pub team_scoped_work_count: usize,
    pub source_linked_work_count: usize,
    #[serde(default)]
    pub fenced_source_count: usize,
    pub issues: Vec<WorkCutoverIssue>,
}

/// Validate the breaking responsibility cutover required by ADR 0052.
///
/// A source-linked persistent Work is accepted only after the compatibility
/// Company WorkItem is no longer live. This prevents two mutable owner/status
/// records from surviving the cutover. Duplicate source links are refused
/// because they would make one Company responsibility fan out into competing
/// Work authorities.
pub fn validate_work_cutover(
    works: &[Work],
    team_runs: &[AgentTeamRun],
    company_work_items: &[WorkItem],
) -> WorkCutoverReport {
    validate_work_cutover_impl(works, team_runs, company_work_items, &[], &[], false)
}

/// Validate the concurrency-safe cutover protocol, including the durable
/// Company refusal markers and their matching Work promotion events.
pub fn validate_work_cutover_with_fences(
    works: &[Work],
    team_runs: &[AgentTeamRun],
    company_work_items: &[WorkItem],
    fences: &[WorkCutoverFence],
    work_events: &[WorkEvent],
) -> WorkCutoverReport {
    validate_work_cutover_impl(
        works,
        team_runs,
        company_work_items,
        fences,
        work_events,
        true,
    )
}

fn validate_work_cutover_impl(
    works: &[Work],
    team_runs: &[AgentTeamRun],
    company_work_items: &[WorkItem],
    fences: &[WorkCutoverFence],
    work_events: &[WorkEvent],
    enforce_fences: bool,
) -> WorkCutoverReport {
    let runs = team_runs
        .iter()
        .map(|run| (run.id.as_str(), run))
        .collect::<BTreeMap<_, _>>();
    let company_items = company_work_items
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let works_by_id = works
        .iter()
        .map(|work| (work.id.as_str(), work))
        .collect::<BTreeMap<_, _>>();
    let events_by_id = work_events
        .iter()
        .map(|event| (event.id.as_str(), event))
        .collect::<BTreeMap<_, _>>();
    let mut fences_by_source = BTreeMap::<&str, Vec<&WorkCutoverFence>>::new();
    for fence in fences {
        fences_by_source
            .entry(fence.company_work_item_id.as_str())
            .or_default()
            .push(fence);
    }
    let mut source_links = BTreeMap::<&str, Vec<&str>>::new();
    let mut issues = Vec::new();

    for work in works {
        let Some(team_id) = work.team_id.as_deref() else {
            issues.push(WorkCutoverIssue {
                kind: WorkCutoverIssueKind::LegacyTeamRunScope,
                work_id: Some(work.id.clone()),
                company_work_item_id: work.source_work_item_ref.clone(),
                detail: format!(
                    "Work {} is still scoped only to TeamRun {}",
                    work.id, work.team_run_id
                ),
            });
            continue;
        };
        match runs.get(work.team_run_id.as_str()) {
            None => issues.push(WorkCutoverIssue {
                kind: WorkCutoverIssueKind::MissingExecutionRun,
                work_id: Some(work.id.clone()),
                company_work_item_id: work.source_work_item_ref.clone(),
                detail: format!(
                    "Work {} references missing execution TeamRun {}",
                    work.id, work.team_run_id
                ),
            }),
            Some(run) => {
                let run_team_id = run
                    .agent_team_id
                    .as_deref()
                    .or(run.definition_id.as_deref());
                if run_team_id != Some(team_id) {
                    issues.push(WorkCutoverIssue {
                        kind: WorkCutoverIssueKind::TeamScopeMismatch,
                        work_id: Some(work.id.clone()),
                        company_work_item_id: work.source_work_item_ref.clone(),
                        detail: format!(
                            "Work {} is scoped to AgentTeam {} but execution TeamRun {} belongs to {:?}",
                            work.id, team_id, work.team_run_id, run_team_id
                        ),
                    });
                }
            }
        }

        let Some(source_id) = work.source_work_item_ref.as_deref() else {
            continue;
        };
        source_links.entry(source_id).or_default().push(&work.id);
        match company_items.get(source_id) {
            None => issues.push(WorkCutoverIssue {
                kind: WorkCutoverIssueKind::MissingCompanyWorkItem,
                work_id: Some(work.id.clone()),
                company_work_item_id: Some(source_id.to_string()),
                detail: format!(
                    "Work {} references missing Company WorkItem {}",
                    work.id, source_id
                ),
            }),
            Some(item)
                if !matches!(
                    item.status,
                    WorkItemStatus::Draft
                        | WorkItemStatus::Completed
                        | WorkItemStatus::Cancelled
                        | WorkItemStatus::Archived
                ) =>
            {
                issues.push(WorkCutoverIssue {
                    kind: WorkCutoverIssueKind::ActiveCompanyWorkItemOverlap,
                    work_id: Some(work.id.clone()),
                    company_work_item_id: Some(source_id.to_string()),
                    detail: format!(
                        "Company WorkItem {} is {:?} while persistent Work {} is also authoritative",
                        source_id, item.status, work.id
                    ),
                });
            }
            Some(_) => {}
        }

        if enforce_fences {
            match fences_by_source.get(source_id).map(Vec::as_slice) {
                None | Some([]) => issues.push(WorkCutoverIssue {
                    kind: WorkCutoverIssueKind::MissingCompanyWorkItemFence,
                    work_id: Some(work.id.clone()),
                    company_work_item_id: Some(source_id.to_string()),
                    detail: format!(
                        "persistent Work {} has no durable Company WorkItem refusal fence",
                        work.id
                    ),
                }),
                Some([fence])
                    if fence.work_id == work.id
                        && fence.team_id == team_id
                        && (events_by_id
                            .get(fence.promotion_event_id.as_str())
                            .is_some_and(|event| {
                                event.work_id == work.id
                                    && event.kind == WorkEventKind::TeamScopePromoted
                                    && event.expected_version == fence.expected_work_version
                            })
                            || work_events.iter().any(|event| {
                                event.work_id == work.id
                                    && event.kind == WorkEventKind::TeamScopePromoted
                                    && event.expected_version >= fence.expected_work_version
                            })) =>
                {
                    if let Some(item) = company_items.get(source_id) {
                        if serde_json::to_value(item).ok().as_ref()
                            != Some(&fence.company_work_item_snapshot)
                        {
                            issues.push(WorkCutoverIssue {
                                kind: WorkCutoverIssueKind::CompanyWorkItemChangedAfterFence,
                                work_id: Some(work.id.clone()),
                                company_work_item_id: Some(source_id.to_string()),
                                detail: format!(
                                    "Company WorkItem {source_id} changed after its cutover fence"
                                ),
                            });
                        }
                    }
                }
                Some([fence]) if fence.work_id == work.id && fence.team_id == team_id => {
                    issues.push(WorkCutoverIssue {
                        kind: WorkCutoverIssueKind::IncompleteCompanyWorkItemFence,
                        work_id: Some(work.id.clone()),
                        company_work_item_id: Some(source_id.to_string()),
                        detail: format!(
                            "Company WorkItem {source_id} fence references missing or invalid promotion event {}",
                            fence.promotion_event_id
                        ),
                    });
                }
                Some(_) => issues.push(WorkCutoverIssue {
                    kind: WorkCutoverIssueKind::ConflictingCompanyWorkItemFence,
                    work_id: Some(work.id.clone()),
                    company_work_item_id: Some(source_id.to_string()),
                    detail: format!(
                        "Company WorkItem {source_id} has multiple or mismatched cutover fences"
                    ),
                }),
            }
        }
    }

    if enforce_fences {
        for fence in fences {
            if !works_by_id.get(fence.work_id.as_str()).is_some_and(|work| {
                work.team_id.as_deref() == Some(fence.team_id.as_str())
                    && work.source_work_item_ref.as_deref()
                        == Some(fence.company_work_item_id.as_str())
            }) {
                issues.push(WorkCutoverIssue {
                    kind: WorkCutoverIssueKind::IncompleteCompanyWorkItemFence,
                    work_id: Some(fence.work_id.clone()),
                    company_work_item_id: Some(fence.company_work_item_id.clone()),
                    detail: format!(
                        "Company WorkItem {} is fenced for Work {}, but that Work is not yet the matching Team authority",
                        fence.company_work_item_id, fence.work_id
                    ),
                });
            }
        }
    }

    for (source_id, linked_work_ids) in source_links {
        if linked_work_ids.len() > 1 {
            issues.push(WorkCutoverIssue {
                kind: WorkCutoverIssueKind::DuplicateCompanyWorkItemLink,
                work_id: None,
                company_work_item_id: Some(source_id.to_string()),
                detail: format!(
                    "Company WorkItem {} is linked by multiple Works: {}",
                    source_id,
                    linked_work_ids.join(", ")
                ),
            });
        }
    }

    WorkCutoverReport {
        valid: issues.is_empty(),
        checked_work_count: works.len(),
        team_scoped_work_count: works.iter().filter(|work| work.is_team_scoped()).count(),
        source_linked_work_count: works
            .iter()
            .filter(|work| work.source_work_item_ref.is_some())
            .count(),
        fenced_source_count: fences_by_source.len(),
        issues,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkEvent {
    pub id: String,
    pub team_run_id: String,
    pub work_id: String,
    pub sequence: u64,
    pub kind: WorkEventKind,
    pub expected_version: u64,
    pub resulting_version: u64,
    pub performed_by_actor: TeamActorRef,
    #[serde(default)]
    pub authority_actor: Option<TeamActorRef>,
    #[serde(default)]
    pub causation_ref: Option<WorkCausationRef>,
    pub idempotency_key: String,
    #[serde(default)]
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkDeliveryStatus {
    Queued,
    Claimed,
    ProviderReceived,
    Failed,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkDelivery {
    pub id: String,
    pub work_event_id: String,
    pub team_run_id: String,
    pub work_id: String,
    pub work_version: u64,
    pub recipient_member_run_id: String,
    pub status: WorkDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_by_supervisor_id: Option<String>,
    #[serde(default)]
    pub claimed_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    pub updated_at: String,
}

/// One crash-atomic store row: event, resulting projection, and initial outbox
/// deliveries are serialized as one JSONL record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOperation {
    pub event: WorkEvent,
    pub work: Work,
    #[serde(default)]
    pub deliveries: Vec<WorkDelivery>,
    #[serde(default)]
    pub delivery_updates: Vec<WorkDeliveryUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkDeliveryUpdate {
    pub delivery_id: String,
    /// Store-global ordering for delivery projection updates. Legacy rows
    /// deserialize as zero and are folded before sequenced writes.
    #[serde(default)]
    pub update_sequence: u64,
    pub status: WorkDeliveryStatus,
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_by_supervisor_id: Option<String>,
    #[serde(default)]
    pub claimed_generation: Option<u64>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    pub updated_at: String,
}

/// Why the exact bound Host must inspect durable Agent Team state.
///
/// This is deliberately separate from [`TeamMessageKind`] and
/// [`WorkEventKind`]. Work remains the responsibility/status plane, while a
/// Host attention row is only a durable notification that a particular Work
/// state now needs Host action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAttentionKind {
    WorkReviewRequested,
    WorkBlocked,
    WorkAccepted,
    WorkChangesRequested,
    WorkCancelled,
    WorkPrerequisiteCompleted,
    WorkDeliveryFailed,
    /// A Work has stayed in `Review` beyond its `review_timeout_hours`.
    WorkReviewTimedOut,
    MemberStoppedWithOwnedReadyWork,
    MemberFailedWithOwnedReadyWork,
}

/// Transport/intake state for one Host attention row.
///
/// `Delivered` proves only that the exact provider-native Host task accepted
/// the notification. `Acknowledged` proves Host intake. `EscalationRequired`
/// is set by a headless host dispatcher when the attention needs explicit human
/// decision (accept/merge/cancel) that the triage-only host cannot make.
/// Neither `Acknowledged` nor `EscalationRequired` mutates the referenced Work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostAttentionStatus {
    Actionable,
    Claimed,
    Delivered,
    Acknowledged,
    EscalationRequired,
}

/// Durable notification derived from a Work-state or member-runtime fact.
///
/// Host binding is intentionally not copied into this row. Read projections
/// resolve the latest [`AgentTeamRun`] binding, so an item created while
/// unbound cannot leak to another task and becomes deliverable only after an
/// explicit binding exists. Claim fields snapshot the exact binding that owns
/// an in-flight delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAttention {
    pub id: String,
    pub team_run_id: String,
    pub kind: HostAttentionKind,
    pub work_id: String,
    pub work_version: u64,
    /// Exact WorkEvent, TeamRunEvent, or provider control event that caused
    /// this notification. Runtime integration should derive `id`
    /// deterministically from this reference so retries remain idempotent.
    pub source_event_ref: String,
    #[serde(default)]
    pub member_run_id: Option<String>,
    pub status: HostAttentionStatus,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub claim_id: Option<String>,
    #[serde(default)]
    pub claimed_host_surface: Option<String>,
    #[serde(default)]
    pub claimed_host_thread_id: Option<String>,
    #[serde(default)]
    pub provider_receipt_id: Option<String>,
    #[serde(default)]
    pub last_failure_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl HostAttention {
    /// Delivered rows remain actionable until the exact Host explicitly ACKs
    /// intake or escalates. A claim is also visible so another transport cannot
    /// double-wake the same Host while the first attempt is in flight.
    pub fn needs_host_action(&self) -> bool {
        self.status != HostAttentionStatus::Acknowledged
            && self.status != HostAttentionStatus::EscalationRequired
    }
}

impl Validate for HostAttention {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "HostAttention.id")?;
        require_non_empty(&self.team_run_id, "HostAttention.team_run_id")?;
        require_non_empty(&self.work_id, "HostAttention.work_id")?;
        require_non_empty(&self.source_event_ref, "HostAttention.source_event_ref")?;
        require_non_empty(&self.created_at, "HostAttention.created_at")?;
        require_non_empty(&self.updated_at, "HostAttention.updated_at")?;
        if let Some(member_run_id) = &self.member_run_id {
            require_non_empty(member_run_id, "HostAttention.member_run_id")?;
        }
        if let Some(claim_id) = &self.claim_id {
            require_non_empty(claim_id, "HostAttention.claim_id")?;
        }
        if let Some(surface) = &self.claimed_host_surface {
            require_non_empty(surface, "HostAttention.claimed_host_surface")?;
        }
        if let Some(thread_id) = &self.claimed_host_thread_id {
            require_non_empty(thread_id, "HostAttention.claimed_host_thread_id")?;
        }
        if let Some(receipt_id) = &self.provider_receipt_id {
            require_non_empty(receipt_id, "HostAttention.provider_receipt_id")?;
        }
        if let Some(reason) = &self.last_failure_reason {
            require_non_empty(reason, "HostAttention.last_failure_reason")?;
        }
        Ok(())
    }
}

/// TeamRun-scoped read projection. `warning` is populated for an unbound run;
/// exact native-thread queries never return such a projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAttentionInbox {
    pub team_run_id: String,
    pub host_surface: String,
    #[serde(default)]
    pub host_thread_id: Option<String>,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub attentions: Vec<HostAttention>,
}

/// Configuration for the daemon-driven headless host dispatcher.
///
/// The dispatcher watches for actionable [`HostAttention`] rows older than
/// `attention_age_threshold_secs` and spawns a headless host round when the
/// host binding lease is not held by a live human session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDispatchConfig {
    /// Minimum age in seconds before a pending attention is eligible for
    /// headless dispatch. Default 300 (5 minutes).
    #[serde(default = "HostDispatchConfig::default_age_threshold")]
    pub attention_age_threshold_secs: u64,
    /// How often the supervisor daemon polls for actionable attentions, in
    /// seconds. Default 60.
    #[serde(default = "HostDispatchConfig::default_poll_interval_secs")]
    pub poll_interval_secs: u64,
    /// When false (default), the headless host is triage-only: it may inspect
    /// attentions, reply to members, and escalate, but MUST NOT accept, merge,
    /// or cancel Work. Set true to allow those mutations without human review.
    #[serde(default)]
    pub accept_merge_enabled: bool,
}

impl Default for HostDispatchConfig {
    fn default() -> Self {
        Self {
            attention_age_threshold_secs: Self::default_age_threshold(),
            poll_interval_secs: Self::default_poll_interval_secs(),
            accept_merge_enabled: false,
        }
    }
}

impl HostDispatchConfig {
    pub const fn default_age_threshold() -> u64 {
        300
    }
    pub const fn default_poll_interval_secs() -> u64 {
        60
    }
}

/// Result from one invocation of the headless host dispatcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDispatchOutcome {
    /// Number of attentions the headless host inspected.
    pub inspected: usize,
    /// Attentions escalated to human (terminal `EscalationRequired`).
    pub escalated: Vec<String>,
    /// Attentions the headless host was able to handle (replied / noted).
    pub handled: Vec<String>,
    /// Attentions the dispatcher could not process (error / unavailable).
    pub failed: Vec<String>,
    /// Human-readable summary of what the headless host did.
    pub summary: Option<String>,
}

impl HostDispatchOutcome {
    pub fn empty() -> Self {
        Self {
            inspected: 0,
            escalated: Vec::new(),
            handled: Vec::new(),
            failed: Vec::new(),
            summary: None,
        }
    }

    pub fn is_noop(&self) -> bool {
        self.inspected == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkDelegation {
    pub id: String,
    pub parent_work_ref: WorkRef,
    pub parent_owner_member_run_id: String,
    pub child_agent_team_id: String,
    pub child_team_run_id: String,
    pub child_host_actor: TeamActorRef,
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
/// `action_type` is a free-form Harness coordination/outcome summary. Provider
/// tool, command, file, turn, chat, and reasoning streams stay exclusively in
/// the provider-native session and must not be converted into MemberActions.
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
    Operator,
    Service,
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

    fn bare_team_message(kind: TeamMessageKind) -> TeamMessage {
        TeamMessage {
            id: "tmsg-1".to_string(),
            team_run_id: "run-1".to_string(),
            work_id: None,
            origin_wave_id: None,
            sender: None,
            from_member_id: "host".to_string(),
            recipients: Vec::new(),
            to_member_ids: Vec::new(),
            kind,
            body: "body".to_string(),
            correlation_id: "corr-1".to_string(),
            causation_id: None,
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: Vec::new(),
            created_at: "now".to_string(),
        }
    }

    fn peer_team_message(kind: TeamMessageKind) -> TeamMessage {
        let mut message = bare_team_message(kind);
        message.from_member_id = "member-run-2".to_string();
        message.sender = Some(TeamActorRef {
            kind: TeamActorKind::MemberRun,
            id: "member-run-2".to_string(),
            display_name: None,
            authn_source: None,
        });
        message
    }

    #[test]
    fn team_message_response_intent_defaults_from_kind() {
        // Review and runtime-control kinds require a response round from every sender.
        for kind in [TeamMessageKind::Handoff, TeamMessageKind::Control] {
            assert!(bare_team_message(kind).requires_response(), "{kind:?}");
            assert!(peer_team_message(kind).requires_response(), "{kind:?}");
        }
    }

    #[test]
    fn ordinary_message_response_intent_defaults_from_sender() {
        for kind in [
            TeamMessageKind::Message,
            TeamMessageKind::Question,
            TeamMessageKind::Answer,
            TeamMessageKind::Progress,
            TeamMessageKind::Blocker,
        ] {
            // `message` is the only legal carrier for Host questions,
            // revisions, and acceptance decisions: Host mail must stay waking.
            assert!(
                bare_team_message(kind).requires_response(),
                "host {kind:?} must default to response_required"
            );
            // Peer-to-peer confirmations converge without a new round.
            assert!(
                !peer_team_message(kind).requires_response(),
                "peer {kind:?} must default to informational"
            );
        }
    }

    #[test]
    fn ordinary_message_intent_treats_operator_and_service_as_coordination_plane() {
        // ADR 0012: the Dashboard is the control plane, so an Operator reply
        // must wake an idle member exactly like a Host reply. Routed Company OS
        // inbox mail arrives as a Service sender and must execute, not idle.
        for actor_kind in [
            TeamActorKind::Host,
            TeamActorKind::Operator,
            TeamActorKind::Service,
        ] {
            let mut message = bare_team_message(TeamMessageKind::Message);
            message.from_member_id = format!("{actor_kind:?}-sender");
            message.sender = Some(TeamActorRef {
                kind: actor_kind,
                id: "sender-1".to_string(),
                display_name: None,
                authn_source: None,
            });
            assert!(message.requires_response(), "{actor_kind:?}");
        }
    }

    #[test]
    fn historical_rows_without_sender_fall_back_to_from_member_id() {
        let mut historical = bare_team_message(TeamMessageKind::Message);
        historical.sender = None;
        assert!(historical.requires_response(), "from_member_id == host");
        historical.from_member_id = "member-run-9".to_string();
        assert!(
            !historical.requires_response(),
            "historical peer mail stays informational"
        );
    }

    #[test]
    fn team_message_explicit_response_intent_wins_over_kind_default() {
        // Override upward: an ack-only peer note that genuinely needs action.
        let mut ack_only = peer_team_message(TeamMessageKind::Message);
        assert_eq!(
            ack_only.effective_response_intent(),
            TeamMessageResponseIntent::Informational
        );
        ack_only.response_intent = Some(TeamMessageResponseIntent::ResponseRequired);
        assert!(ack_only.requires_response());
        // Override downward: Host mail that is deliberately FYI-only.
        let mut host_fyi = bare_team_message(TeamMessageKind::Message);
        assert!(host_fyi.requires_response());
        host_fyi.response_intent = Some(TeamMessageResponseIntent::Informational);
        assert!(!host_fyi.requires_response());
        // Override downward on a work-carrying kind too.
        let mut control = bare_team_message(TeamMessageKind::Control);
        control.response_intent = Some(TeamMessageResponseIntent::Informational);
        assert!(!control.requires_response());
        // The explicit field round-trips through serde; an absent field keeps
        // historical rows on their kind+sender-derived default.
        let json = serde_json::to_string(&ack_only).expect("serialize");
        assert!(json.contains("\"response_intent\":\"response_required\""));
        let without = peer_team_message(TeamMessageKind::Message);
        let json = serde_json::to_string(&without).expect("serialize");
        assert!(!json.contains("response_intent"));
        let historical: TeamMessage =
            serde_json::from_str(&json).expect("deserialize without the field");
        assert_eq!(historical.response_intent, None);
        assert!(!historical.requires_response());
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
            closing_test_ref: Some("crates/firm-cli/src/main.rs::snapshot_test".to_string()),
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
        let home = std::path::Path::new("/Users/me/.firm");
        assert_eq!(
            project_store_root(home, "ai-luodi-jyx3d"),
            std::path::Path::new("/Users/me/.firm/projects/ai-luodi-jyx3d")
        );
        assert_eq!(
            project_store_root(home, GLOBAL_PROJECT_ID),
            std::path::Path::new("/Users/me/.firm/projects/_global")
        );
    }

    #[test]
    fn project_context_round_trips_json() {
        let ctx = ProjectContext {
            id: "ai-luodi-jyx3d".into(),
            project_root: std::path::PathBuf::from("/Users/me/ai-luodi/jyx3d"),
            store_root: std::path::PathBuf::from("/Users/me/.firm/projects/ai-luodi-jyx3d"),
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
            native_session: None,
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
            prompt_ref: Some(".firm/prompts/worker.md".to_string()),
            skill_refs: vec!["firm-workflow".to_string()],
            workspace_policy: None,
            worktree_ref: Some("../worktrees/task-1".to_string()),
            permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            status: AgentMemberStatus::Idle,
            current_task_id: None,
            current_proposal_id: None,
            provider_runtime_id: None,
            native_session: None,
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
        member.runtime_workspace_roots = vec!["crates/firm-core".to_string()];
        member.provider_config.runtime_workspace_roots = vec!["crates/firm-cli".to_string()];
        let message = sample_message();

        let spec = build_launch_spec(&member, &message);

        // Pillar 1 base configuration flows through unchanged.
        assert_eq!(
            spec.prompt_ref.as_deref(),
            Some(".firm/prompts/worker.md")
        );
        assert_eq!(spec.model.as_deref(), Some("o3"));
        assert_eq!(spec.effort.as_deref(), Some("high"));
        assert_eq!(spec.skill_refs, vec!["firm-workflow".to_string()]);
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

    #[test]
    fn workspace_observability_fields_round_trip_without_contents() {
        let snapshot = MemberWorkspaceSnapshot {
            cwd: "/projects/harness/worktrees/member-1".into(),
            project_binding_id: Some("harness".into()),
            resolution_source: Some("member_worktree".into()),
            git_head: Some("0123456789abcdef".into()),
            git_branch: Some("feature/member-1".into()),
            instruction_roots: vec!["/projects/harness".into()],
            skill_roots: vec!["/projects/harness/.agents/skills".into()],
        };
        assert!(snapshot.validate().is_ok());

        let json = serde_json::to_value(&snapshot).expect("serialize workspace snapshot");
        assert_eq!(json["cwd"], "/projects/harness/worktrees/member-1");
        assert!(json.get("instruction_contents").is_none());
        assert!(json.get("skill_contents").is_none());
        assert!(json.get("credentials").is_none());
        assert!(json.get("transcript").is_none());
        assert!(json.get("thinking").is_none());
        assert_eq!(
            serde_json::from_value::<MemberWorkspaceSnapshot>(json).expect("deserialize snapshot"),
            snapshot
        );
    }

    #[test]
    fn workspace_observability_validation_rejects_empty_locators() {
        let snapshot = MemberWorkspaceSnapshot {
            cwd: " ".into(),
            project_binding_id: None,
            resolution_source: None,
            git_head: None,
            git_branch: None,
            instruction_roots: Vec::new(),
            skill_roots: Vec::new(),
        };
        assert_eq!(
            snapshot.validate(),
            Err(ValidationError::Required {
                field: "MemberWorkspaceSnapshot.cwd"
            })
        );

        let snapshot = MemberWorkspaceSnapshot {
            cwd: "/projects/harness".into(),
            project_binding_id: None,
            resolution_source: None,
            git_head: None,
            git_branch: None,
            instruction_roots: vec![String::new()],
            skill_roots: Vec::new(),
        };
        assert_eq!(
            snapshot.validate(),
            Err(ValidationError::Required {
                field: "MemberWorkspaceSnapshot.instruction_roots"
            })
        );
    }

    #[test]
    fn legacy_workspace_rows_deserialize_with_absent_new_fields() {
        let team: AgentTeamRun = serde_json::from_str(
            r#"{"id":"tr-legacy","host_surface":"codex-app","objective":"work","status":"planning","created_at":"unix-ms:1","updated_at":"unix-ms:1"}"#,
        )
        .expect("deserialize legacy team run");
        assert!(team.execution_root.is_none());

        let member: MemberRun = serde_json::from_str(
            r#"{"id":"mr-legacy","team_run_id":"tr-legacy","name":"worker","role":"worker","provider":"codex","status":"idle","started_at":"unix-ms:1"}"#,
        )
        .expect("deserialize legacy member run");
        assert!(member.worktree_ref.is_none());
        assert!(member.workspace_snapshot.is_none());
        assert_eq!(
            member.provider_controls,
            ProviderExecutionControls::default(),
            "historical rows stay readable without inventing requested or effective controls"
        );
    }

    #[test]
    fn provider_execution_controls_separate_intent_from_native_receipt() {
        let mut controls = ProviderExecutionControls::requested(
            Some("gpt-5.6-sol".into()),
            Some("max".into()),
            Some("priority".into()),
        );

        assert_eq!(controls.model.status, ProviderControlStatus::Requested);
        assert_eq!(controls.model.effective, None);
        controls
            .model
            .mark_effective(Some("gpt-5.6-sol".into()), "confirmed by provider response");
        controls
            .service_tier
            .mark_unsupported("provider exposes no service tier");
        controls
            .reasoning_effort
            .mark_review_required("installed provider version is not reviewed");

        assert_eq!(controls.model.status, ProviderControlStatus::Effective);
        assert_eq!(
            controls.service_tier.status,
            ProviderControlStatus::Unsupported
        );
        assert_eq!(
            controls.reasoning_effort.status,
            ProviderControlStatus::ReviewRequired
        );
        assert_eq!(controls.reasoning_effort.effective, None);

        let encoded = serde_json::to_string(&controls).expect("serialize controls");
        let decoded: ProviderExecutionControls =
            serde_json::from_str(&encoded).expect("deserialize controls");
        assert_eq!(decoded, controls);
    }

    fn capacity_snapshot(
        state: ProviderCapacityState,
        observed_unix_ms: u64,
    ) -> ProviderCapacitySnapshot {
        ProviderCapacitySnapshot {
            provider: "kimi".to_string(),
            execution_mode: "kimi_acp".to_string(),
            account: ProviderAccountRef {
                source: "oauth_credentials_file".to_string(),
                identifier: None,
                plan: None,
            },
            state,
            observed_at: "unix-ms:1000".to_string(),
            observed_unix_ms,
            reset_at: None,
            evidence_source: ProviderCapacityEvidence::ProviderError,
            confidence: ProviderCapacityConfidence::Observed,
            windows: Vec::new(),
            diagnosis: None,
            runtime_context: Vec::new(),
            detail: None,
        }
    }

    #[test]
    fn capacity_default_state_is_unknown_and_never_available() {
        assert_eq!(
            ProviderCapacityState::default(),
            ProviderCapacityState::Unknown
        );
        assert!(!ProviderCapacityState::Unknown.is_known_unavailable());
        assert!(!ProviderCapacityState::Available.is_known_unavailable());
        assert!(!ProviderCapacityState::Limited.is_known_unavailable());
        assert!(ProviderCapacityState::Exhausted.is_known_unavailable());
        assert!(ProviderCapacityState::Unauthorized.is_known_unavailable());
    }

    #[test]
    fn capacity_freshness_uses_the_observation_timestamp() {
        let snapshot = capacity_snapshot(ProviderCapacityState::Exhausted, 1_000);
        assert_eq!(
            snapshot.freshness(1_500, 1_000),
            ProviderCapacityFreshness::Fresh
        );
        assert_eq!(
            snapshot.freshness(5_000, 1_000),
            ProviderCapacityFreshness::Stale
        );
        // A future-dated or unstamped observation is never treated as fresh.
        assert_eq!(
            snapshot.freshness(500, 1_000),
            ProviderCapacityFreshness::Unknown
        );
        assert_eq!(
            capacity_snapshot(ProviderCapacityState::Exhausted, 0).freshness(5_000, 1_000),
            ProviderCapacityFreshness::Unknown
        );
    }

    #[test]
    fn fresh_known_unavailable_capacity_blocks_start() {
        for state in [
            ProviderCapacityState::Exhausted,
            ProviderCapacityState::Unauthorized,
        ] {
            let snapshot = capacity_snapshot(state, 1_000);
            let decision = provider_capacity_start_decision(Some(&snapshot), 1_100, 1_000);
            assert!(decision.is_blocked(), "{state:?} must block a fresh start");
            assert!(
                decision.reason().contains("kimi_acp"),
                "the blocking reason names the execution mode: {}",
                decision.reason()
            );
        }
    }

    #[test]
    fn unknown_absent_and_stale_capacity_never_block_and_never_claim_available() {
        let unknown = capacity_snapshot(ProviderCapacityState::Unknown, 1_000);
        assert!(!provider_capacity_start_decision(Some(&unknown), 1_100, 1_000).is_blocked());
        assert_ne!(unknown.state, ProviderCapacityState::Available);

        assert!(!provider_capacity_start_decision(None, 1_100, 1_000).is_blocked());

        let stale = capacity_snapshot(ProviderCapacityState::Exhausted, 1_000);
        let decision = provider_capacity_start_decision(Some(&stale), 100_000, 1_000);
        assert!(!decision.is_blocked());
        assert!(decision.reason().contains("no longer fresh"));
    }

    #[test]
    fn capacity_is_independent_of_adapter_compatibility_and_round_trips_json() {
        // A reviewed-current adapter says nothing about runtime availability:
        // this is the Wave 2 evidence (`current` adapter, 403 at request time).
        let profile = ProviderIntegrationProfile {
            provider: "claude".to_string(),
            execution_mode: "claude_agent_sdk".to_string(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: Some("2.1.220".to_string()),
            adapter_contract_version: Some("claude-agent-sdk-v1".to_string()),
            reviewed_provider_versions: vec!["2.1.220".to_string()],
            compatibility_status: ProviderCompatibilityStatus::Current,
            adapter_reviewed_at: None,
            compatibility_note: None,
            interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
            ordinary_message_boundary: OrdinaryMessageBoundary::InTurn,
            plan_mode: ProviderFeatureMode::Emulated,
            goal_mode: ProviderFeatureMode::Emulated,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Structured,
            supports_cancel: true,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
        };
        let mut snapshot = capacity_snapshot(ProviderCapacityState::Unauthorized, 1_000);
        snapshot.provider = "claude".to_string();
        snapshot.execution_mode = "claude_agent_sdk".to_string();
        snapshot.diagnosis = Some("no HTTPS_PROXY in the Harness process".to_string());
        snapshot.runtime_context = vec![ProviderRuntimeContextFact {
            key: "HTTPS_PROXY".to_string(),
            present: false,
            note: Some("absent".to_string()),
        }];

        assert_eq!(
            profile.compatibility_status,
            ProviderCompatibilityStatus::Current
        );
        assert!(snapshot.state.is_known_unavailable());

        let encoded = serde_json::to_string(&snapshot).expect("serialize snapshot");
        let decoded: ProviderCapacitySnapshot =
            serde_json::from_str(&encoded).expect("deserialize snapshot");
        assert_eq!(decoded, snapshot);
        assert!(
            !encoded.contains("compatibility"),
            "capacity JSON must not carry adapter compatibility: {encoded}"
        );
    }

    #[test]
    fn member_run_rows_without_capacity_stay_readable_and_absent_is_not_available() {
        let row = serde_json::json!({
            "id": "member-run-1",
            "team_run_id": "team-run-1",
            "name": "Integration",
            "role": "Integration Engineer",
            "provider": "claude",
            "status": "idle",
            "started_at": "unix-ms:1"
        });
        let member: MemberRun = serde_json::from_value(row).expect("legacy member run");
        assert_eq!(member.provider_capacity, None);
        assert!(!provider_capacity_start_decision(
            member.provider_capacity.as_ref(),
            1_000,
            PROVIDER_CAPACITY_DEFAULT_TTL_MS
        )
        .is_blocked());
    }

    /// The emit/schema contract for MemberRun.
    ///
    /// `schemas/member-run.schema.json` keeps `additionalProperties: false`, so
    /// any field the emitter serialises that the schema does not declare makes
    /// an emitted MemberRun fail validation against its own schema. This test
    /// round-trips a MemberRun carrying `provider_capacity` and asserts every
    /// emitted key — top level and inside the capacity snapshot — is declared.
    /// It fails on the next undeclared field, not just on `provider_capacity`.
    #[test]
    fn emitted_member_run_keys_are_declared_in_member_run_schema() {
        let schema: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../schemas/member-run.schema.json"
            ))
            .expect("read member-run schema"),
        )
        .expect("parse member-run schema");
        assert_eq!(
            schema["additionalProperties"],
            serde_json::Value::Bool(false),
            "this test only means something while the schema is closed"
        );

        let snapshot = ProviderCapacitySnapshot {
            provider: "claude".to_string(),
            execution_mode: "sdk".to_string(),
            account: ProviderAccountRef {
                source: "oauth_credentials_file".to_string(),
                identifier: Some("acct-primary".to_string()),
                plan: Some("max".to_string()),
            },
            state: ProviderCapacityState::Limited,
            observed_at: "unix-ms:1785591600000".to_string(),
            observed_unix_ms: 1_785_591_600_000,
            reset_at: Some("unix-ms:1785595200000".to_string()),
            evidence_source: ProviderCapacityEvidence::ProviderQuotaApi,
            confidence: ProviderCapacityConfidence::Observed,
            windows: vec![ProviderCapacityWindow {
                label: "five_hour".to_string(),
                limit_id: Some("limit-5h".to_string()),
                used_percent: Some(82),
                window_duration_mins: Some(300),
                resets_at: Some("unix-ms:1785595200000".to_string()),
            }],
            diagnosis: Some("Account usage is high but not blocking.".to_string()),
            runtime_context: vec![ProviderRuntimeContextFact {
                key: "HTTPS_PROXY".to_string(),
                present: true,
                note: Some("set".to_string()),
            }],
            detail: Some("Provider quota API reported 82% of the five-hour window.".to_string()),
        };
        let row = serde_json::json!({
            "id": "member-run-capacity-1",
            "team_run_id": "team-run-capacity-1",
            "name": "Platform Development",
            "role": "Platform Development",
            "provider": "claude",
            "status": "idle",
            "started_at": "unix-ms:1785591600000"
        });
        let mut member: MemberRun = serde_json::from_value(row).expect("member run");
        member.provider_capacity = Some(snapshot.clone());

        let encoded = serde_json::to_value(&member).expect("encode member run");
        let declared = schema["properties"].as_object().expect("schema properties");
        let undeclared = encoded
            .as_object()
            .expect("encoded member run")
            .keys()
            .filter(|key| !declared.contains_key(key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            undeclared.is_empty(),
            "emitted MemberRun fields are not declared in member-run.schema.json              (additionalProperties is false, so these cannot validate): {undeclared:?}"
        );

        let declared_capacity = declared["provider_capacity"]["properties"]
            .as_object()
            .expect("schema must declare provider_capacity as an object with properties");
        let undeclared_capacity = encoded["provider_capacity"]
            .as_object()
            .expect("provider_capacity must serialise as an object when present")
            .keys()
            .filter(|key| !declared_capacity.contains_key(key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            undeclared_capacity.is_empty(),
            "emitted provider_capacity fields are not declared in member-run.schema.json:              {undeclared_capacity:?}"
        );

        // Round-trip: the snapshot survives encode/decode unchanged, so the
        // schema is describing the shape the runtime actually persists.
        let decoded: MemberRun = serde_json::from_value(encoded).expect("decode member run");
        assert_eq!(decoded.provider_capacity, Some(snapshot));
    }

    #[test]
    fn work_prerequisite_satisfaction_is_distinct_from_claim_readiness() {
        fn work(id: &str, status: WorkStatus, prerequisites: Vec<&str>) -> Work {
            Work {
                id: id.into(),
                team_run_id: "team-1".into(),
                team_id: None,
                created_by_member_id: None,
                parent_work_id: None,
                source_work_item_ref: None,
                title: id.into(),
                context_markdown: String::new(),
                completion_criteria_markdown: "done".into(),
                status,
                owner_member_id: None,
                active_member_run_id: None,
                claim_mode: WorkClaimMode::TeamClaim,
                eligible_member_ids: Vec::new(),
                prerequisite_work_ids: prerequisites.into_iter().map(str::to_string).collect(),
                priority: WorkPriority::Normal,
                created_by_actor: TeamActorRef {
                    kind: TeamActorKind::Host,
                    id: "host".into(),
                    display_name: None,
                    authn_source: None,
                },
                result_summary: None,
                blocker_reason: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                github_links: Vec::new(),
                gates: Vec::new(),
                workspace: None,
                version: 1,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            }
        }

        let prerequisite = work("prerequisite", WorkStatus::Done, vec![]);
        let in_progress = work("dependent", WorkStatus::InProgress, vec!["prerequisite"]);
        assert!(in_progress.prerequisites_satisfied([&prerequisite]));
        assert!(!in_progress.is_claim_ready([&prerequisite]));

        let open = work("dependent-open", WorkStatus::Open, vec!["prerequisite"]);
        assert!(open.is_claim_ready([&prerequisite]));

        let unfinished = work("prerequisite", WorkStatus::Review, vec![]);
        assert!(!open.prerequisites_satisfied([&unfinished]));
        assert!(!open.is_claim_ready([&unfinished]));
    }

    #[test]
    fn work_cutover_rejects_live_company_overlap_and_accepts_archived_source() {
        let run: AgentTeamRun = serde_json::from_value(serde_json::json!({
            "id": "run-1",
            "agent_team_id": "team-1",
            "host_surface": "test",
            "objective": "cut over",
            "status": "running",
            "member_run_ids": [],
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }))
        .expect("team run");
        let work: Work = serde_json::from_value(serde_json::json!({
            "id": "work-1",
            "team_run_id": "run-1",
            "team_id": "team-1",
            "source_work_item_ref": "company-work-1",
            "title": "Persistent Work",
            "context_markdown": "context",
            "completion_criteria_markdown": "done",
            "status": "open",
            "claim_mode": "team_claim",
            "eligible_member_ids": [],
            "prerequisite_work_ids": [],
            "priority": "normal",
            "created_by_actor": {"kind": "host", "id": "host"},
            "artifact_refs": [],
            "check_refs": [],
            "version": 1,
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }))
        .expect("Work");
        let item = |status: &str| -> WorkItem {
            serde_json::from_value(serde_json::json!({
                "id": "company-work-1",
                "title": "Compatibility WorkItem",
                "objective": "cut over",
                "status": status,
                "source_document_ref": "document-1",
                "source_record_refs": [],
                "result_document_ref": null,
                "result_record_refs": [],
                "submitted_by": {"actor_type": "human", "actor_id": "human-1"},
                "requested_by": null,
                "accountable_owner": {"actor_type": "human", "actor_id": "human-1"},
                "assignees": [],
                "contributors": [],
                "reviewer": null,
                "approver": null,
                "execution_mode": "agent_team",
                "execution_refs": [],
                "approval_refs": [],
                "evidence_refs": [],
                "artifact_refs": [],
                "outcome_summary": null,
                "due_at": null,
                "priority": null,
                "risk_level": null,
                "created_at": "unix-ms:1",
                "updated_at": "unix-ms:1",
                "completed_at": null
            }))
            .expect("WorkItem")
        };
        let promotion_event: WorkEvent = serde_json::from_value(serde_json::json!({
            "id": "event-promote-work-1",
            "team_run_id": "run-1",
            "work_id": "work-1",
            "sequence": 2,
            "kind": "team_scope_promoted",
            "expected_version": 0,
            "resulting_version": 1,
            "performed_by_actor": {"kind": "host", "id": "host"},
            "idempotency_key": "promote-work-1",
            "created_at": "unix-ms:1"
        }))
        .expect("promotion event");
        let fence = WorkCutoverFence {
            company_work_item_id: "company-work-1".into(),
            work_id: work.id.clone(),
            team_id: "team-1".into(),
            promotion_event_id: promotion_event.id.clone(),
            expected_work_version: promotion_event.expected_version,
            company_work_item_status: WorkItemStatus::Archived,
            company_work_item_updated_at: "unix-ms:1".into(),
            company_work_item_snapshot: serde_json::to_value(item("archived"))
                .expect("WorkItem snapshot"),
            idempotency_key: promotion_event.idempotency_key.clone(),
            created_at: "unix-ms:1".into(),
        };

        let active = validate_work_cutover_with_fences(
            std::slice::from_ref(&work),
            std::slice::from_ref(&run),
            &[item("in_progress")],
            std::slice::from_ref(&fence),
            std::slice::from_ref(&promotion_event),
        );
        assert!(!active.valid);
        assert!(active
            .issues
            .iter()
            .any(|issue| { issue.kind == WorkCutoverIssueKind::ActiveCompanyWorkItemOverlap }));

        let missing_fence = validate_work_cutover_with_fences(
            std::slice::from_ref(&work),
            std::slice::from_ref(&run),
            &[item("archived")],
            &[],
            std::slice::from_ref(&promotion_event),
        );
        assert!(missing_fence
            .issues
            .iter()
            .any(|issue| { issue.kind == WorkCutoverIssueKind::MissingCompanyWorkItemFence }));

        assert!(
            validate_work_cutover(
                std::slice::from_ref(&work),
                std::slice::from_ref(&run),
                &[item("archived")],
            )
            .valid
        );
        let archived = validate_work_cutover_with_fences(
            std::slice::from_ref(&work),
            std::slice::from_ref(&run),
            &[item("archived")],
            std::slice::from_ref(&fence),
            std::slice::from_ref(&promotion_event),
        );
        assert!(archived.valid, "unexpected issues: {:?}", archived.issues);
        let mut silently_mutated = item("archived");
        silently_mutated.title = "mutated without advancing updated_at".into();
        let mutated = validate_work_cutover_with_fences(
            std::slice::from_ref(&work),
            &[],
            &[silently_mutated],
            std::slice::from_ref(&fence),
            std::slice::from_ref(&promotion_event),
        );
        assert!(mutated
            .issues
            .iter()
            .any(|issue| { issue.kind == WorkCutoverIssueKind::CompanyWorkItemChangedAfterFence }));

        let duplicate = validate_work_cutover_with_fences(
            &[
                work.clone(),
                Work {
                    id: "work-2".into(),
                    ..work
                },
            ],
            &[],
            &[item("archived")],
            &[fence],
            &[promotion_event],
        );
        assert!(duplicate
            .issues
            .iter()
            .any(|issue| { issue.kind == WorkCutoverIssueKind::DuplicateCompanyWorkItemLink }));
    }

    #[test]
    fn legacy_work_delivery_update_defaults_to_unsequenced() {
        let update: WorkDeliveryUpdate = serde_json::from_value(serde_json::json!({
            "delivery_id": "delivery-legacy",
            "status": "queued",
            "attempt": 1,
            "updated_at": "unix-ms:1"
        }))
        .expect("legacy delivery update remains readable");
        assert_eq!(update.update_sequence, 0);
    }

    #[test]
    fn host_attention_keeps_transport_intake_distinct_from_work_semantics() {
        let mut attention = HostAttention {
            id: "host-attention-work-event-1".into(),
            team_run_id: "team-run-1".into(),
            kind: HostAttentionKind::WorkReviewRequested,
            work_id: "work-1".into(),
            work_version: 3,
            source_event_ref: "work-event-1".into(),
            member_run_id: Some("member-run-1".into()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        };
        assert!(attention.validate().is_ok());
        assert!(attention.needs_host_action());

        attention.status = HostAttentionStatus::Delivered;
        attention.provider_receipt_id = Some("provider-receipt-1".into());
        assert!(
            attention.needs_host_action(),
            "delivery is transport receipt, not Host intake or Work acceptance"
        );
        attention.status = HostAttentionStatus::Acknowledged;
        assert!(!attention.needs_host_action());

        let json = serde_json::to_value(&attention).expect("serialize Host attention");
        assert_eq!(json["kind"], "work_review_requested");
        assert_eq!(json["status"], "acknowledged");
        assert!(json.get("team_message_id").is_none());
        assert!(json.get("work_status").is_none());
    }

    fn topology_team(
        id: &str,
        member_ids: &[&str],
        parent_team_id: Option<&str>,
        host_member_id: Option<&str>,
    ) -> AgentTeam {
        AgentTeam {
            id: id.to_string(),
            name: format!("{id} name"),
            description: format!("{id} description"),
            owner_agent_id: "host".to_string(),
            status: AgentTeamStatus::Active,
            member_ids: member_ids.iter().map(|id| id.to_string()).collect(),
            parent_team_id: parent_team_id.map(str::to_string),
            host_member_id: host_member_id.map(str::to_string),
            created_at: "unix-ms:1".to_string(),
            updated_at: "unix-ms:1".to_string(),
        }
    }

    fn topology_map(teams: Vec<AgentTeam>) -> BTreeMap<String, AgentTeam> {
        teams
            .into_iter()
            .map(|team| (team.id.clone(), team))
            .collect()
    }

    #[test]
    fn agent_team_topology_fields_default_to_root_for_legacy_rows() {
        let team: AgentTeam = serde_json::from_value(serde_json::json!({
            "id": "team-legacy",
            "name": "Legacy",
            "description": "Row written before the topology slice.",
            "owner_agent_id": "host",
            "member_ids": [],
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1"
        }))
        .expect("legacy team row without topology fields remains readable");
        assert_eq!(team.parent_team_id, None);
        assert_eq!(team.host_member_id, None);
        let teams = topology_map(vec![team]);
        assert_eq!(validate_agent_team_topology(&teams), Ok(()));
    }

    fn durable_member(id: &str) -> DurableAgentMember {
        DurableAgentMember {
            id: id.to_string(),
            name: id.to_string(),
            description: format!("Durable identity for {id}"),
            role: "lead".to_string(),
            provider_profile: Some("kimi/qwen3.8-max".to_string()),
            model: Some("qwen/qwen3.8-max".to_string()),
            workspace_policy: Some("project_binding".to_string()),
            project_binding_id: Some("project-harness".to_string()),
            business_access_ceiling_refs: vec!["company_os.read".to_string()],
            status: DurableAgentMemberStatus::Active,
            created_by_member_id: None,
            created_at: "unix-ms:1".to_string(),
            updated_at: "unix-ms:1".to_string(),
        }
    }

    #[test]
    fn host_authority_compatibility_mapping_is_deterministic_and_refuses_conflicts() {
        let mut team = topology_team("root", &["lead"], None, None);
        team.owner_agent_id = "lead".to_string();
        assert_eq!(compat_host_member_id(&team), Ok(Some("lead")));

        team.host_member_id = Some("lead".to_string());
        assert_eq!(resolve_team_host_authority(&team), Ok("lead"));

        team.host_member_id = Some("different-lead".to_string());
        assert_eq!(
            resolve_team_host_authority(&team),
            Err(HostAuthorityError::Conflicting {
                team_id: "root".to_string(),
                owner_agent_id: "lead".to_string(),
                host_member_id: "different-lead".to_string(),
            })
        );
    }

    #[test]
    fn host_authority_cutover_requires_explicit_member_backed_single_authority() {
        let mut team = topology_team("root", &["lead"], None, Some("lead"));
        let mut teams = topology_map(vec![team.clone()]);
        let members = BTreeMap::from([("lead".to_string(), durable_member("lead"))]);

        assert!(matches!(
            validate_host_authority_cutover(&teams, &members),
            Err(HostAuthorityError::CompatibilityAuthorityStillActive { .. })
        ));

        team.owner_agent_id = "lead".to_string();
        teams.insert(team.id.clone(), team);
        assert_eq!(validate_host_authority_cutover(&teams, &members), Ok(()));

        assert!(matches!(
            validate_host_authority_cutover(&teams, &BTreeMap::new()),
            Err(HostAuthorityError::UnknownMember { .. })
        ));
    }

    #[test]
    fn agent_team_topology_accepts_valid_recursive_forest() {
        let teams = topology_map(vec![
            topology_team("root", &["lead", "cto"], None, Some("lead")),
            topology_team("child", &["worker"], Some("root"), Some("cto")),
            topology_team("grandchild", &["leaf"], Some("child"), Some("worker")),
            topology_team("legacy-flat", &[], None, None),
        ]);
        assert_eq!(validate_agent_team_topology(&teams), Ok(()));
        assert_eq!(child_team_ids(&teams, "root"), vec!["child".to_string()]);
        assert_eq!(child_team_ids(&teams, "grandchild"), Vec::<String>::new());
        assert_eq!(
            team_subtree_ids(&teams, "root"),
            vec![
                "root".to_string(),
                "child".to_string(),
                "grandchild".to_string()
            ]
        );
        assert_eq!(
            team_ancestor_ids(&teams, "grandchild"),
            vec!["child".to_string(), "root".to_string()]
        );
        assert_eq!(team_ancestor_ids(&teams, "root"), Vec::<String>::new());
        assert_eq!(team_subtree_ids(&teams, "missing"), Vec::<String>::new());
    }

    #[test]
    fn agent_team_topology_rejects_missing_parent() {
        let teams = topology_map(vec![topology_team(
            "child",
            &[],
            Some("missing"),
            Some("cto"),
        )]);
        assert_eq!(
            validate_agent_team_topology(&teams),
            Err(TeamTopologyError::UnknownParent {
                team_id: "child".to_string(),
                parent_team_id: "missing".to_string(),
            })
        );
    }

    #[test]
    fn agent_team_topology_rejects_non_root_without_host() {
        let teams = topology_map(vec![
            topology_team("root", &["cto"], None, None),
            topology_team("child", &[], Some("root"), None),
        ]);
        assert_eq!(
            validate_agent_team_topology(&teams),
            Err(TeamTopologyError::MissingHost {
                team_id: "child".to_string(),
            })
        );
    }

    #[test]
    fn agent_team_topology_rejects_host_outside_parent_membership() {
        let teams = topology_map(vec![
            topology_team("root", &["lead"], None, Some("lead")),
            topology_team("child", &[], Some("root"), Some("outsider")),
        ]);
        assert_eq!(
            validate_agent_team_topology(&teams),
            Err(TeamTopologyError::HostNotParentMember {
                team_id: "child".to_string(),
                host_member_id: "outsider".to_string(),
                parent_team_id: "root".to_string(),
            })
        );
    }

    #[test]
    fn agent_team_topology_rejects_member_hosting_two_teams() {
        let teams = topology_map(vec![
            topology_team("root", &["cto"], None, None),
            topology_team("child-a", &[], Some("root"), Some("cto")),
            topology_team("child-b", &[], Some("root"), Some("cto")),
        ]);
        assert_eq!(
            validate_agent_team_topology(&teams),
            Err(TeamTopologyError::MultipleHostedTeams {
                host_member_id: "cto".to_string(),
                first_team_id: "child-a".to_string(),
                second_team_id: "child-b".to_string(),
            })
        );
    }

    #[test]
    fn agent_team_topology_rejects_cycles() {
        let self_parent = topology_map(vec![topology_team(
            "loop",
            &["lead"],
            Some("loop"),
            Some("lead"),
        )]);
        assert_eq!(
            validate_agent_team_topology(&self_parent),
            Err(TeamTopologyError::Cycle {
                team_id: "loop".to_string(),
            })
        );

        let mut cycle = topology_map(vec![
            // a's host ha is a direct member of parent b; b's host hb is a
            // direct member of parent a — direct-host passes, so the acyclic
            // invariant is what must reject the pair.
            topology_team("a", &["hb"], Some("b"), Some("ha")),
            topology_team("b", &["ha"], Some("a"), Some("hb")),
        ]);
        assert_eq!(
            validate_agent_team_topology(&cycle),
            Err(TeamTopologyError::Cycle {
                team_id: "a".to_string(),
            })
        );
        // Breaking one parent link restores a valid two-level tree.
        cycle.get_mut("b").expect("team b").parent_team_id = None;
        assert_eq!(validate_agent_team_topology(&cycle), Ok(()));
        assert_eq!(
            team_subtree_ids(&cycle, "b"),
            vec!["b".to_string(), "a".to_string()]
        );
    }

    // ── GateEngine tests ──────────────────────────────────────────

    fn make_work(gates: Vec<GateSpec>, github_links: Vec<GitHubLink>) -> Work {
        Work {
            id: "work-1".into(),
            team_run_id: "run-1".into(),
            team_id: None,
            created_by_member_id: None,
            parent_work_id: None,
            source_work_item_ref: None,
            title: "test work".into(),
            context_markdown: String::new(),
            completion_criteria_markdown: "done".into(),
            status: WorkStatus::Review,
            owner_member_id: None,
            active_member_run_id: None,
            claim_mode: WorkClaimMode::HostAssign,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: WorkPriority::Normal,
            created_by_actor: TeamActorRef {
                kind: TeamActorKind::Host,
                id: "host".into(),
                display_name: None,
                authn_source: None,
            },
            result_summary: Some("done".into()),
            blocker_reason: None,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links,
            gates,
            workspace: None,
            version: 1,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:2".into(),
        }
    }

    fn make_pr_link(status: Option<&str>, ci_status: Option<&str>) -> GitHubLink {
        GitHubLink {
            kind: GitHubLinkKind::PullRequest,
            owner: "owner".into(),
            repo: "repo".into(),
            number: 42,
            url: "https://github.com/owner/repo/pull/42".into(),
            status: status.map(|s| s.to_string()),
            ci_status: ci_status.map(|s| s.to_string()),
            ci_url: None,
        }
    }

    #[test]
    fn empty_gates_all_pass() {
        let work = make_work(vec![], vec![]);
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(results.is_empty());
    }

    #[test]
    fn github_pr_gate_pass_when_merged_and_ci_ok() {
        let work = make_work(
            vec![GateSpec {
                plugin: "github-pr".into(),
                config: serde_json::json!({"require_merged": true, "require_ci_pass": true}),
            }],
            vec![make_pr_link(Some("MERGED"), Some("success"))],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert_eq!(results.len(), 1);
        assert!(results[0].verdict.is_pass());
    }

    #[test]
    fn github_pr_gate_blocked_when_no_pr() {
        let work = make_work(
            vec![GateSpec {
                plugin: "github-pr".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(matches!(results[0].verdict, GateVerdict::Blocked { .. }));
    }

    #[test]
    fn github_pr_gate_blocked_when_not_merged() {
        let work = make_work(
            vec![GateSpec {
                plugin: "github-pr".into(),
                config: serde_json::json!({"require_merged": true}),
            }],
            vec![make_pr_link(Some("OPEN"), None)],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(matches!(results[0].verdict, GateVerdict::Blocked { .. }));
    }

    #[test]
    fn github_pr_gate_fail_when_ci_failed() {
        let work = make_work(
            vec![GateSpec {
                plugin: "github-pr".into(),
                config: serde_json::json!({"require_ci_pass": true}),
            }],
            vec![make_pr_link(Some("MERGED"), Some("failure"))],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(matches!(results[0].verdict, GateVerdict::Fail { .. }));
    }

    #[test]
    fn github_pr_gate_pass_without_ci_when_not_required() {
        let work = make_work(
            vec![GateSpec {
                plugin: "github-pr".into(),
                config: serde_json::json!({"require_merged": true}),
            }],
            vec![make_pr_link(Some("MERGED"), None)],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(results[0].verdict.is_pass());
    }

    #[test]
    fn unknown_gate_plugin_fails() {
        let work = make_work(
            vec![GateSpec {
                plugin: "nonexistent".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(matches!(results[0].verdict, GateVerdict::Fail { .. }));
    }

    #[test]
    fn multiple_gates_report_all_results() {
        let work = make_work(
            vec![
                GateSpec {
                    plugin: "github-pr".into(),
                    config: serde_json::json!({"require_merged": true}),
                },
                GateSpec {
                    plugin: "github-pr".into(),
                    config: serde_json::json!({"require_ci_pass": true}),
                },
            ],
            vec![make_pr_link(Some("MERGED"), Some("success"))],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert_eq!(results.len(), 2);
        assert!(results[0].verdict.is_pass());
        assert!(results[1].verdict.is_pass());
    }

    // ── code-review gate tests ─────────────────────────────────────

    fn make_review(work_id: &str, verdict: ReviewVerdict, reviewer: &str) -> Review {
        Review {
            id: format!("review-{work_id}"),
            task_id: Some(work_id.to_string()),
            goal_id: None,
            reviewer_agent_id: reviewer.to_string(),
            review_kind: "code".to_string(),
            verdict,
            summary: "looks good".to_string(),
            blockers: vec![],
            residual_risk: None,
            missing_validation: vec![],
            evidence_ids: vec![],
            created_at: "unix-ms:10".to_string(),
        }
    }

    #[test]
    fn code_review_gate_blocked_when_no_review() {
        let work = make_work(
            vec![GateSpec {
                plugin: "code-review".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        let results = GateEngine::evaluate_work_gates_with_reviews(&work, &[]);
        assert!(matches!(results[0].verdict, GateVerdict::Blocked { .. }));
    }

    #[test]
    fn code_review_gate_pass_when_review_pass() {
        let work = make_work(
            vec![GateSpec {
                plugin: "code-review".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        let reviews = vec![make_review("work-1", ReviewVerdict::Pass, "critic-1")];
        let results = GateEngine::evaluate_work_gates_with_reviews(&work, &reviews);
        assert!(results[0].verdict.is_pass());
    }

    #[test]
    fn code_review_gate_fail_when_review_fail() {
        let work = make_work(
            vec![GateSpec {
                plugin: "code-review".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        let reviews = vec![make_review("work-1", ReviewVerdict::Fail, "critic-1")];
        let results = GateEngine::evaluate_work_gates_with_reviews(&work, &reviews);
        assert!(matches!(results[0].verdict, GateVerdict::Fail { .. }));
    }

    #[test]
    fn code_review_gate_fail_when_needs_changes() {
        let work = make_work(
            vec![GateSpec {
                plugin: "code-review".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        let reviews = vec![make_review("work-1", ReviewVerdict::NeedsChanges, "critic-1")];
        let results = GateEngine::evaluate_work_gates_with_reviews(&work, &reviews);
        assert!(matches!(results[0].verdict, GateVerdict::Fail { .. }));
    }

    // ── artifact-exists gate tests ──────────────────────────────────

    #[test]
    fn artifact_exists_pass_when_artifacts_present() {
        let work = make_work(
            vec![GateSpec {
                plugin: "artifact-exists".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        // work has no artifacts by default — test with artifacts added
        let mut work_with_artifact = work.clone();
        work_with_artifact.artifact_refs = vec!["docs/report.md".into()];
        let results = GateEngine::evaluate_work_gates(&work_with_artifact);
        assert!(results[0].verdict.is_pass());
    }

    #[test]
    fn artifact_exists_blocked_when_no_artifacts() {
        let work = make_work(
            vec![GateSpec {
                plugin: "artifact-exists".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(matches!(results[0].verdict, GateVerdict::Blocked { .. }));
    }

    #[test]
    fn artifact_exists_fail_when_specified_paths_missing() {
        let work = make_work(
            vec![GateSpec {
                plugin: "artifact-exists".into(),
                config: serde_json::json!({"paths": ["required/doc.md"]}),
            }],
            vec![],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(matches!(results[0].verdict, GateVerdict::Fail { .. }));
    }

    #[test]
    fn artifact_exists_pass_when_specified_paths_found() {
        let mut work = make_work(
            vec![GateSpec {
                plugin: "artifact-exists".into(),
                config: serde_json::json!({"paths": ["required/doc.md"]}),
            }],
            vec![],
        );
        work.artifact_refs = vec!["required/doc.md".into()];
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(results[0].verdict.is_pass());
    }

    // ── check-pass gate tests ───────────────────────────────────────

    #[test]
    fn check_pass_pass_when_checks_present() {
        let mut work = make_work(
            vec![GateSpec {
                plugin: "check-pass".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        work.check_refs = vec!["ci/build".into()];
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(results[0].verdict.is_pass());
    }

    #[test]
    fn check_pass_blocked_when_no_checks() {
        let work = make_work(
            vec![GateSpec {
                plugin: "check-pass".into(),
                config: serde_json::json!({}),
            }],
            vec![],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(matches!(results[0].verdict, GateVerdict::Blocked { .. }));
    }

    #[test]
    fn check_pass_fail_when_specified_checks_missing() {
        let work = make_work(
            vec![GateSpec {
                plugin: "check-pass".into(),
                config: serde_json::json!({"checks": ["cargo test", "cargo clippy"]}),
            }],
            vec![],
        );
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(matches!(results[0].verdict, GateVerdict::Fail { .. }));
    }

    #[test]
    fn check_pass_pass_when_specified_checks_found() {
        let mut work = make_work(
            vec![GateSpec {
                plugin: "check-pass".into(),
                config: serde_json::json!({"checks": ["cargo test"]}),
            }],
            vec![],
        );
        work.check_refs = vec!["cargo test".into(), "cargo clippy".into()];
        let results = GateEngine::evaluate_work_gates(&work);
        assert!(results[0].verdict.is_pass());
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
