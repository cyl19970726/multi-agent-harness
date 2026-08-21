use super::*;

/// Dispatch discriminant for the provider seam.
///
/// This is **not** a schema field: `ProviderLaunchProfile.provider` (and the other
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTeamStatus {
    Active,
    Inactive,
    Trashed,
}

fn default_agent_team_status() -> AgentTeamStatus {
    AgentTeamStatus::Active
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTeam {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Immutable vNext placement fence. Runtime recovery may advance daemon
    /// and Supervisor generations, but it never rewrites Team placement.
    pub node_id: String,
    #[serde(default = "default_agent_team_status")]
    pub status: AgentTeamStatus,
    /// Team identity revision. Membership and runtime revisions remain
    /// independent; this value changes only with the durable Team record.
    pub revision: u64,
    /// Optional read-only provenance for a pre-vNext Mission-owned Team. New
    /// Team creation never requires or derives identity from this relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_mission_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trashed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Non-serialized legacy read projection populated by the Store from
    /// `legacy_mission_id`. It is never accepted as Team identity authority.
    #[serde(skip)]
    pub mission_id: String,
    /// Non-serialized compatibility read projection populated from the one
    /// retained Host TeamMembership. It is never persisted on AgentTeam.
    #[serde(skip)]
    pub host_agent_id: String,
    /// Non-serialized compatibility read projection of active non-Host
    /// TeamMemberships. Durable roster authority remains TeamMembership.
    #[serde(skip)]
    pub member_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryMessageIntent {
    Message,
    Report,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryDeliveryStatus {
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

/// Identity class of a [`RegistryMessage`] sender. Distinguishes harness-managed agents
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
pub struct RegistryDeliveryAttempt {
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
/// Mission/Agent Team/Workflow records belong to an independent Execution
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
pub enum ProviderProcessStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderProcessHealth {
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
pub struct ProviderProcess {
    pub id: String,
    pub agent_member_id: String,
    pub provider: String,
    pub status: ProviderProcessStatus,
    pub pid: Option<u32>,
    pub control_endpoint: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub health: ProviderProcessHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDispatchEvent {
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
pub struct RegistryMessage {
    pub id: String,
    pub task_id: Option<String>,
    pub from_agent_id: String,
    pub to_agent_id: Option<String>,
    pub channel: Option<String>,
    pub kind: RegistryMessageIntent,
    pub delivery_status: RegistryDeliveryStatus,
    pub content: String,
    pub evidence_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub delivery: Option<RegistryDeliveryAttempt>,
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

/// First-class evaluator/critic output. Today an unstructured report RegistryMessage; the
/// Review object captures verdict + findings + residual risk as structured data.
///
/// Concept-model invariant: a Review is *evidence for* a Decision, not the global
/// decision itself — a Lead/gate still issues the Decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(default, deserialize_with = "deserialize_present_review_actor")]
    pub performed_by_actor: Option<TeamActorRef>,
    #[serde(default, deserialize_with = "deserialize_present_review_actor")]
    pub authority_actor: Option<TeamActorRef>,
    pub created_at: String,
}

fn deserialize_present_review_actor<'de, D>(
    deserializer: D,
) -> Result<Option<TeamActorRef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    TeamActorRef::deserialize(deserializer).map(Some)
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
