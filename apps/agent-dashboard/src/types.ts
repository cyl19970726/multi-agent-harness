export type DeliveryStatus = "queued" | "delivered" | "acknowledged" | "failed";
/**
 * A Project Binding: provider cwd, repository instructions, Skills,
 * Git/worktree and permission boundary. `compatibility_store_root` is only a
 * legacy locator and never means this binding owns coordination truth.
 */
export interface Project {
  id: string;
  project_root: string;
  compatibility_store_root?: string;
  kind: "repo" | "global";
  is_git_repo: boolean;
  is_current: boolean;
  repository_url?: string | null;
  default_branch?: string | null;
  git_common_dir?: string | null;
  instruction_boundary?: string;
  skill_discovery_boundary?: string;
  worktree_policy?: string | null;
  permission_policy?: string | null;
  identity_boundary?: "project_binding" | string;
  owns_execution_store?: boolean;
}

/** Provider-neutral coordination namespace. */
export interface ExecutionSpace {
  id: string;
  name?: string;
  store_root: string;
  default_project_binding_id?: string | null;
  company_id?: string | null;
  is_current: boolean;
  identity_boundary?: "execution_space" | string;
}

/**
 * One Company Store in the Company OS control plane. Unlike a Project, this is
 * the company truth boundary for Docs / Work / Organization / Finance; project
 * binding is optional execution/source context.
 */
export interface Company {
  id: string;
  name?: string;
  store_root: string;
  is_current: boolean;
  identity_boundary?: "company_store" | string;
  execution_dependency?: "optional" | string;
  project_binding?: "external" | string;
}

export type RegistryMessageIntent = "message" | "report";
export type SenderKind = "agent" | "operator" | "system";
export type ProviderExecutionStatus = "queued" | "running" | "succeeded" | "failed" | "canceled" | "stale";

/**
 * The backend's four-layer runtime health snapshot (serialized
 * `ProviderProcessHealth`). A `null`/missing probe means "unknown" — it must NOT
 * be rendered as healthy/green; treat it as amber.
 */
export interface RuntimeHealth {
  process_alive?: boolean;
  socket_exists?: boolean;
  protocol_probe?: string | null;
  delivery_probe?: string | null;
  checked_at?: string | null;
}

export interface ProviderLaunchProfile {
  id: string;
  name?: string;
  description?: string;
  role?: string;
  provider?: string;
  model?: string | null;
  status?: string;
  runtime_status?: string | null;
  runtime_id?: string | null;
  runtime_pid?: number | null;
  runtime_alive?: boolean;
  runtime_health?: RuntimeHealth | null;
  control_endpoint?: string | null;
  native_session?: NativeSessionRef | null;
  provider_thread_id?: string | null;
  provider_agent_path?: string | null;
  provider_agent_nickname?: string | null;
  provider_agent_role?: string | null;
  current_proposal_id?: string | null;
  prompt_ref?: string | null;
  skill_refs?: string[];
  profile?: string | null;
  provider_config?: ProviderLaunchConfig | null;
  created_at?: string | null;
  last_seen_at?: string | null;
  queued_count?: number;
  inbox_count?: number;
  team_ids?: string[];
  provider_child_thread_count?: number;
}

/**
 * Durable Company/Organization identity (ADR 0052). Runtime/session state is
 * intentionally absent and remains on MemberRun / the compatibility
 * ProviderLaunchProfile projection.
 */
export interface AgentMember {
  id: string;
  name: string;
  description: string;
  role: string;
  capabilities: string[];
  skill_refs: string[];
  provider_profile_ref?: string | null;
  model_preference?: string | null;
  workspace_policy: string;
  permission_ceiling: "read_only" | "workspace_write" | "full_access" | string;
  organization_status: "active" | "paused" | "retired" | string;
  version: number;
  created_by: { kind: "human" | "agent_member" | "external" | "service"; id: string };
  created_at: string;
  updated_at: string;
}

export interface CompanyOsSnapshotProjection {
  agent_members?: AgentMember[];
  [key: string]: unknown;
}

/** Provider launch/runtime config carried on an ProviderLaunchProfile (mirrors the Rust
 * ProviderLaunchConfig). All optional; the Config tab renders what is set and
 * shows "Not configured" otherwise. */
export interface ProviderLaunchConfig {
  service_tier?: string | null;
  collaboration_mode?: string | null;
  approval_policy?: string | null;
  approvals_reviewer?: string | null;
  sandbox_policy?: string | null;
  permission_profile?: string | null;
  runtime_workspace_roots?: string[];
  environment_id?: string | null;
  mcp?: { servers?: AgentMcpServer[] } | null;
}

export interface AgentMcpServer {
  id: string;
  transport?: string | null;
  command?: string[];
  url?: string | null;
  allowed_tools?: string[];
}

export interface AgentTeam {
  id: string;
  name?: string;
  description?: string;
  /** Immutable machine placement for every Member in the Team. */
  node_id: string;
  /** Team identity revision; membership and runtime revisions stay independent. */
  revision?: number;
  status?: "active" | "closed" | "archived";
  /** Optional read-only provenance for a pre-vNext Mission-owned Team. Never identity authority. */
  legacy_mission_id?: string | null;
  trashed_at?: string | null;
  created_at?: string;
  updated_at?: string;
}

export interface Message {
  id: string;
  from_agent_id?: string;
  to_agent_id?: string | null;
  channel?: string | null;
  kind: RegistryMessageIntent;
  delivery_status: DeliveryStatus;
  content?: string;
  evidence_ids?: string[];
  created_at?: string;
  delivery?: MessageDelivery | null;
  // Identity class of the sender; absent on legacy rows (defaults to "agent"
  // server-side). Rendering distinction is handled in a later work package.
  sender_kind?: SenderKind;
}

export interface MessageDelivery {
  delivery_id?: string | null;
  execution_status?: ProviderExecutionStatus | string | null;
  native_session?: NativeSessionRef | null;
  started_at?: string | null;
  provider_request_id?: string | null;
  provider_thread_id?: string | null;
  provider_turn_id?: string | null;
  terminal_source?: string | null;
  delivered_at?: string | null;
  last_error?: string | null;
}

export interface ProviderDispatchEvent {
  id: string;
  agent_member_id: string;
  provider_runtime_id?: string | null;
  event_type?: string;
  summary?: string;
  payload_ref?: string | null;
  created_at?: string;
}

export interface ProviderChildThread {
  id: string;
  provider?: string;
  agent_member_id: string;
  provider_runtime_id?: string | null;
  parent_provider_thread_id?: string | null;
  provider_thread_id?: string;
  provider_agent_path?: string | null;
  provider_agent_nickname?: string | null;
  provider_agent_role?: string | null;
  status?: string;
  last_message_ref?: string | null;
  created_at?: string;
  updated_at?: string;
}

export interface Evidence {
  id: string;
  source_type?: string;
  source_ref?: string;
  summary?: string;
  evidence_kind?: string | null;
}

/**
 * One entry of `docs/registry.json` (schema agent_harness.docs_registry.v1) —
 * the machine-readable manifest of every project doc. The Docs surface fetches
 * the registry (via the allow-listed `GET /v1/docs?path=docs/registry.json`) and
 * builds its tree from these entries; only `path` is guaranteed present.
 */
export interface DocRegistryEntry {
  path: string;
  ownerRole?: string;
  status?: "idea" | "planned" | "stable" | "deprecated" | "archival";
  lifecycle?: "volatile" | "stable" | "archival";
  canonicalFor?: string[];
  dependsOn?: string[];
}

/* ------------------------------------------------------------------ */
/* Agent Team runs (team-run orchestration, WP team-console)           */
/* ------------------------------------------------------------------ */

/** Lifecycle of a durable Mission. */
export type MissionStatus =
  | "planned"
  | "running"
  | "blocked"
  | "completed"
  | "cancelled";

/** Durable intent container. Its AgentTeam relation survives only as optional legacy provenance. */
export interface Mission {
  id: string;
  title: string;
  objective: string;
  context?: string;
  desired_outcome?: string | null;
  status?: MissionStatus | string;
  outcome_summary?: string | null;
  completed_by?: string | null;
  created_at?: string;
  updated_at?: string;
  completed_at?: string | null;
}

/** @deprecated ADR 0051 pre-cutover history only. */
export type LegacyWaveExecutorKind = "agent_team" | "host";

/** @deprecated ADR 0051 pre-cutover history only. */
export type LegacyWaveStatus =
  | "planned"
  | "running"
  | "waiting"
  | "completed"
  | "blocked"
  | "failed"
  | "cancelled";

/** @deprecated ADR 0051 pre-cutover history only. */
export type LegacyWaveGateStatus = "pending" | "accepted" | "revise" | "blocked";

/**
 * One ADR 0051 pre-cutover Wave row. Legacy historical read-only: current
 * Mission status, closeout, TeamRun creation, and navigation never consume it.
 */
export interface LegacyWave {
  id: string;
  mission_id: string;
  index: number;
  title: string;
  objective: string;
  context?: string;
  revision?: number;
  updated_by?: string | null;
  exit_criteria?: string | null;
  status?: LegacyWaveStatus | string;
  executor_kind: LegacyWaveExecutorKind | string;
  executor_run_ids?: string[];
  accepted_run_id?: string | null;
  plan_note?: string | null;
  outcome_summary?: string | null;
  artifact_refs?: string[];
  gate_status?: LegacyWaveGateStatus | string;
  gate_note?: string | null;
  accepted_by?: string | null;
  accepted_at?: string | null;
  created_at?: string;
  updated_at?: string;
}

/** Kind of a {@link MissionLogEntry} (ADR 0051). */
export type MissionLogEntryKind =
  | "judgment"
  | "replan"
  | "recovery"
  | "closeout_evidence";

/**
 * One immutable, append-only Mission Log row (ADR 0051). Mission absorbs
 * Wave as this append-only judgment log: `revision` is monotonic per
 * `mission_id` and store-assigned. There is no update or delete — a
 * correction is a new entry, not a mutation of an old one.
 */
export interface MissionLogEntry {
  id: string;
  mission_id: string;
  revision: number;
  kind: MissionLogEntryKind | string;
  body: string;
  actor: string;
  created_at: string;
}

/** Lifecycle of a {@link TeamRun} (mirrors the harness team-run status). */
export type TeamRunStatus =
  | "planning"
  | "running"
  | "waiting"
  | "reviewing"
  | "completed"
  | "failed"
  | "cancelled";

/**
 * One execution of a required Mission-owned AgentTeam. Its members and native
 * sessions continue under Mission intent and append-only Mission Log judgment. Wire shape is snake_case;
 * timestamps are "unix-ms:<ms>" strings like the rest of the snapshot.
 */
export interface TeamRun {
  id: string;
  agent_team_id: string;
  execution_node_id: string;
  project_binding_id: string;
  /** Explicit retry lineage when this run replaces an earlier run, if any. */
  previous_run_id?: string | null;
  host_surface?: string | null;
  host_thread_id?: string | null;
  host_actor?: TeamActorRef | null;
  host_control_mode?: "managed" | "external_interactive" | string;
  objective?: string | null;
  /** Concrete workspace selected for this attempt; distinct from the centralized store root. */
  execution_root?: string | null;
  status?: TeamRunStatus | string;
  member_run_ids?: string[];
  budget_limit_usd?: number | null;
  created_at?: string;
  updated_at?: string;
  completed_at?: string | null;
}

/** Lifecycle of a {@link MemberRun} (mirrors the harness member-run status). */
export type MemberRunStatus =
  | "starting"
  | "idle"
  | "queued"
  | "running"
  | "waiting"
  | "disconnected"
  | "reviewing"
  | "blocked"
  | "completed"
  | "failed"
  | "stopped";

/** Durable mailbox/participation lifecycle, independent of provider work status. */
export type MemberCoordinationStatus = "active" | "closed" | "retired";

/** Non-secret, immutable-at-start facts about the member's provider workspace. */
export interface MemberWorkspaceSnapshot {
  /** Actual process cwd used to spawn the provider member. */
  cwd: string;
  git_head?: string | null;
  git_branch?: string | null;
  /** Discovered path roots only; instruction file contents are never part of this snapshot. */
  instruction_roots: string[];
  /** Discovered path roots only; skill contents are never part of this snapshot. */
  skill_roots: string[];
}

export type ProviderControlStatus =
  | "not_requested"
  | "requested"
  | "effective"
  | "unsupported"
  | "review_required";

/** One provider-neutral execution setting, separated into intent and receipt. */
export interface ProviderControlValue {
  requested?: string | null;
  effective?: string | null;
  status?: ProviderControlStatus | string;
  note?: string | null;
}

export interface ProviderExecutionControls {
  model: ProviderControlValue;
  reasoning_effort: ProviderControlValue;
  service_tier: ProviderControlValue;
}

/** Store-owned typed authority for a provider-compatibility Blocked state. */
export interface ProviderCompatibilityBlockCause {
  schema_version: 1;
  id: string;
  member_run_id: string;
  provider: string;
  execution_mode: string;
  provider_version: string;
  adapter_contract_version: string;
  boundary: "start_persistent_execution" | "resume_persistent_execution";
  compatibility_status: "review_required" | "incompatible" | "unavailable" | "unknown";
  source: "adapter_compatibility" | "probe_failure";
  probe_error?: string | null;
  caused_at: string;
}

/** One member's participation in a {@link TeamRun}. */
export interface MemberRun {
  id: string;
  team_run_id?: string;
  slot_id?: string | null;
  /** Required canonical AgentMember identity; never inferred from display fields. */
  agent_member_id: string;
  name?: string | null;
  role?: string | null;
  provider?: "codex" | "claude" | "kimi" | string;
  /** Legacy requested-model shortcut; use provider_controls for effective truth. */
  model?: string | null;
  provider_controls?: ProviderExecutionControls | null;
  provider_profile?: ProviderIntegrationProfile | null;
  /**
   * Last observed runtime availability of this member's provider account.
   * Absent or null means nothing was observed; it never means available, and
   * it is independent of provider_profile.compatibility_status.
   */
  provider_capacity?: ProviderCapacitySnapshot | null;
  provider_compatibility_block_cause?: ProviderCompatibilityBlockCause | null;
  coordination_status?: MemberCoordinationStatus | string;
  runtime_generation?: number;
  status?: MemberRunStatus | string;
  native_session?: NativeSessionRef | null;
  /** Optional member-specific Git worktree override of the TeamRun execution root. */
  provider_cwd_hint?: string | null;
  provider_environment_observation?: MemberWorkspaceSnapshot | null;
  owned_paths?: string[];
  started_at?: string;
  last_event_at?: string | null;
  finished_at?: string | null;
}

/**
 * Provider-account capacity as last observed by the Harness capacity probe.
 *
 * Read this honestly: `state: "unknown"` with `evidence_source: "not_exposed"`
 * is the correct, expected answer for providers that expose no quota surface,
 * and an absent snapshot means "not observed" rather than "available". The
 * snapshot is taken once per MemberRun activation and is not refreshed while a
 * member keeps running, so `observed_at` is part of the fact.
 */
export interface ProviderCapacitySnapshot {
  provider: string;
  execution_mode: string;
  account: ProviderAccountRef;
  state: "available" | "limited" | "exhausted" | "unauthorized" | "unknown" | string;
  observed_at: string;
  observed_unix_ms: number;
  reset_at?: string | null;
  evidence_source:
    | "provider_quota_api"
    | "auth_metadata"
    | "execution_canary"
    | "provider_error"
    | "not_exposed"
    | "probe_failed"
    | "none"
    | string;
  confidence: "observed" | "inferred" | "unknown" | string;
  windows?: ProviderCapacityWindow[];
  diagnosis?: string | null;
  runtime_context?: ProviderRuntimeContextFact[];
  detail?: string | null;
}

export interface ProviderAccountRef {
  source: string;
  identifier?: string | null;
  plan?: string | null;
}

/**
 * A provider-reported usage window. `used_percent` is present only when the
 * provider itself reported it; the Workbench never derives or estimates one.
 */
export interface ProviderCapacityWindow {
  label: string;
  limit_id?: string | null;
  used_percent?: number | null;
  window_duration_mins?: number | null;
  resets_at?: string | null;
}

export interface ProviderRuntimeContextFact {
  key: string;
  present: boolean;
  note?: string | null;
}

export interface NativeSessionRef {
  provider: string;
  execution_mode: string;
  native_session_id: string;
  native_locator_kind: string;
  provider_version?: string | null;
  adapter_contract_version: string;
  availability: "available" | "stale" | "missing" | "incompatible" | "unknown" | string;
  supports_resume: boolean;
  last_verified_at?: string | null;
  parent_native_session_id?: string | null;
}

export interface NativeActivityItem {
  kind: "message" | "tool" | string;
  status: "started" | "completed" | "failed" | string;
  title: string;
  summary?: string;
  occurred_at?: string | null;
}

export interface NativeActivityProjection {
  native_session_id: string;
  provider: string;
  execution_mode: string;
  availability: NativeSessionRef["availability"];
  items: NativeActivityItem[];
  truncated: boolean;
}

/** Durable wake-notification the store derives from WorkOperations; the Host
 * acks it through the console (transport intake only, never mutates Work). */
export type HostAttentionKind =
  | "work_review_requested"
  | "work_blocked"
  | "work_accepted"
  | "work_changes_requested"
  | "work_cancelled"
  | "work_delivery_failed"
  | "member_stopped_with_owned_ready_work"
  | "member_failed_with_owned_ready_work";

export type HostAttentionStatus = "actionable" | "claimed" | "delivered" | "acknowledged";

export interface HostAttention {
  id: string;
  team_run_id: string;
  kind: HostAttentionKind | string;
  work_id: string;
  work_version?: number;
  source_event_ref?: string;
  member_run_id?: string | null;
  status: HostAttentionStatus | string;
  attempt?: number;
  claim_id?: string | null;
  claimed_host_surface?: string | null;
  claimed_host_thread_id?: string | null;
  provider_receipt_id?: string | null;
  last_failure_reason?: string | null;
  created_at?: string | null;
  updated_at?: string | null;
}

export interface ProviderIntegrationProfile {
  provider: string;
  execution_mode: string;
  provider_version?: string | null;
  adapter_contract_version?: string | null;
  reviewed_provider_versions?: string[];
  compatibility_status?: "current" | "review_required" | "incompatible" | "unavailable" | "unknown" | string;
  adapter_reviewed_at?: string | null;
  compatibility_note?: string | null;
  /** Who drives the member's rounds. `user_driven` means Harness never starts a
   * provider cycle for a declared external interactive member. */
  execution_driver?: "host_driven" | "provider_driven" | "user_driven" | string;
  interaction_mode: "pause_and_resume" | "end_round_and_follow_up" | "unsupported" | string;
  ordinary_message_boundary?: "in_turn" | "next_round" | "next_round_batched" | "unknown" | string;
  plan_mode?: "native" | "emulated" | "unsupported" | "unknown" | string;
  goal_mode?: "native" | "emulated" | "unsupported" | "unknown" | string;
  tool_event_fidelity: "none" | "summary" | "structured" | string;
  artifact_event_fidelity: "none" | "summary" | "structured" | string;
  supports_cancel: boolean;
  supports_resume: boolean;
  observes_native_subagents: boolean;
  observes_background_tasks: boolean;
  thinking_transient_only: boolean;
}

/**
 * Volatile, display-only member activity delivered over SSE. It is never part
 * of the backend snapshot, ledger history, evidence, messages, or replay.
 */
export interface LiveMemberActivity {
  team_run_id: string;
  member_run_id: string;
  provider: string;
  kind: "thinking" | string;
  preview: string;
  revision: number;
  emitted_at: string;
  expires_at: string;
}

/** Delivery of a {@link TeamMessageProjection} to one recipient. */
export interface ProviderDispatchAttempt {
  member_id?: string;
  policy?: string;
  status?: "queued" | "claimed" | "delivered" | "acknowledged" | "failed" | "expired" | string;
  attempt?: number;
  claim_id?: string | null;
  claimed_by_supervisor_id?: string | null;
  claimed_generation?: number | null;
  claimed_unix_ms?: number | null;
  claim_expires_unix_ms?: number | null;
  provider_receipt_id?: string | null;
  updated_at?: string;
}

export type TeamActorKind = "host" | "member_run" | "agent_member" | "operator" | "service";

export interface TeamActorRef {
  kind: TeamActorKind | string;
  id: string;
  display_name?: string | null;
  authn_source?: string | null;
}

export interface TeamRecipientRef {
  kind: "host" | "member_run" | "agent_member" | string;
  id: string;
}

/** Kind of a {@link TeamMessageProjection} (open enum; rendered as a colored pill). */
export type ProviderDispatchIntent =
  | "message"
  | "control"
  | "provider_interaction_request"
  | "provider_interaction_response";

/**
 * Explicit response intent on a {@link TeamMessageProjection} (ADR 0046 §4).
 * `informational` mail is durable and correlated but never starts a provider
 * round on its own; `response_required` asks the recipient for a semantic
 * reply and wakes an idle provider member.
 */
export type ProviderResponseIntent = "informational" | "response_required";

/**
 * Effective response intent: the explicit field wins; otherwise kind AND
 * sender decide — control always requires a response round,
 * and ordinary message mail requires one unless a peer member sent it
 * (mirrors the Rust `TeamMessageProjection::effective_response_intent` contract).
 */
export function effectiveTeamMessageResponseIntent(
  message: Pick<TeamMessageProjection, "kind" | "response_intent" | "sender" | "sender_runtime_id">,
): ProviderResponseIntent {
  if (message.response_intent === "informational" || message.response_intent === "response_required") {
    return message.response_intent;
  }
  if (message.kind === "control" || message.kind === "provider_interaction_request") {
    return "response_required";
  }
  if (message.kind === "provider_interaction_response") return "informational";
  return sentByPeerMember(message) ? "informational" : "response_required";
}

/**
 * True when a team message was authored by another member rather than by the
 * coordination plane (Host, Operator, Service). Historical rows carry no typed
 * `sender`, so they fall back to the reserved `"host"` `sender_runtime_id`.
 */
function sentByPeerMember(message: Pick<TeamMessageProjection, "sender" | "sender_runtime_id">): boolean {
  const senderKind = message.sender?.kind;
  if (senderKind === "member_run" || senderKind === "agent_member") {
    return true;
  }
  if (senderKind === "host" || senderKind === "operator" || senderKind === "service") {
    return false;
  }
  return message.sender_runtime_id !== "host";
}

/**
 * One message on a team run's handoff chain. `sender_runtime_id` is `"host"` or a
 * member run id; `deliveries` tracks per-recipient ack state (an unacknowledged
 * delivery is a needs-you signal for the operator).
 */
export interface TeamMessageProjection {
  id: string;
  team_run_id?: string;
  /** Optional conversational link. Work remains the responsibility source. */
  work_id?: string | null;
  source_plan_ref?: string | null;
  sender?: TeamActorRef | null;
  sender_runtime_id?: string;
  recipients?: TeamRecipientRef[];
  recipient_runtime_ids?: string[];
  kind?: ProviderDispatchIntent | string;
  body?: string;
  correlation_id?: string | null;
  causation_id?: string | null;
  response_intent?: ProviderResponseIntent | string | null;
  evidence_refs?: string[];
  deliveries?: ProviderDispatchAttempt[];
  created_at?: string;
}

export type AgentSessionStatus = "starting" | "idle" | "running" | "waiting" | "disconnected" | "stopped" | "failed";

export interface AgentIdentity {
  id: string;
  display_name: string;
  organization_status: "active" | "paused" | "retired";
  permission_ceiling: "read_only" | "workspace_write" | "full_access";
  version: number;
  created_at: string;
  updated_at: string;
}

export interface AgentSession {
  id: string;
  agent_identity_id: string;
  node_id: string;
  execution_space_id: string;
  node_daemon_id: string;
  node_daemon_generation: number;
  provider: string;
  provider_profile_ref: string;
  effective_permission_ceiling: "read_only" | "workspace_write" | "full_access";
  status: AgentSessionStatus;
  generation: number;
  version: number;
  created_at: string;
  updated_at: string;
}

export interface TeamMembership {
  id: string;
  team_id: string;
  /** Durable AgentMember identity (legacy payloads may spell it agent_identity_id). */
  agent_member_id?: string;
  agent_identity_id?: string;
  node_id: string;
  role: "host" | "member" | "observer" | string;
  state: "invited" | "active" | "leaving" | "inactive" | string;
  membership_generation?: number;
  revision?: number;
  joined_at: string;
  left_at?: string | null;
}

export interface WorkExecutionBinding {
  id: string;
  work_id: string;
  work_revision: number;
  team_membership_id: string;
  agent_identity_id: string;
  agent_session_id: string;
  agent_session_generation: number;
  status: "active" | "released" | "completed" | "cancelled";
  version: number;
  bound_at: string;
  ended_at?: string | null;
}

export interface CanonicalMessage {
  id: string;
  execution_space_id: string;
  author_node_id: string;
  author_node_daemon_id: string;
  author_node_daemon_generation: number;
  sender_identity_id: string;
  recipients: Array<{kind: "agent_identity" | "team"; id: string}>;
  team_id?: string | null;
  team_run_id?: string | null;
  work_id?: string | null;
  kind: "message" | "reply" | "request_decision" | "provider_interaction_request" | "provider_interaction_response";
  body: string;
  correlation_id: string;
  causation_id?: string | null;
  response_intent: "informational" | "response_required";
  evidence_refs?: string[];
  content_fingerprint: string;
  created_at: string;
}

export interface CanonicalMessageDelivery {
  id: string;
  message_id: string;
  subscription_id: string;
  recipient_identity_id: string;
  target_node_id: string;
  recipient_session_id?: string | null;
  recipient_session_generation?: number | null;
  status: "queued" | "routed" | "claimed" | "provider_received" | "acknowledged" | "failed" | "expired" | "invalidated";
  attempt: number;
  version: number;
  created_at: string;
  updated_at: string;
}

export type WorkPhase = "open" | "active" | "review" | "closed";
export type WorkCondition = "normal" | "blocked" | "on_hold";
export type WorkResolution = "accepted" | "cancelled" | "failed";

export interface Work {
  id: string;
  team_run_id: string;
  /** Durable AgentTeam scope (ADR 0052, §4.1). Absent on legacy rows. */
  team_id?: string | null;
  title: string;
  context_markdown: string;
  completion_criteria_markdown: string;
  phase: WorkPhase | string;
  condition: WorkCondition | string;
  resolution?: WorkResolution | string | null;
  owner_member_id?: string | null;
  active_member_run_id?: string | null;
  claim_mode: "host_assign" | "team_claim" | string;
  eligible_member_ids?: string[];
  prerequisite_work_ids?: string[];
  successor_work_ids?: string[];
  readiness?: {
    state: "ready" | "waiting_prerequisites" | "requires_host_attention" | "not_claimable";
    reason_codes: string[];
    unsatisfied_prerequisite_work_ids: string[];
    failed_or_cancelled_prerequisite_work_ids: string[];
  };
  priority: "low" | "normal" | "high" | "urgent" | string;
  created_by_actor: TeamActorRef;
  result_summary?: string | null;
  blocker_reason?: string | null;
  artifact_refs?: string[];
  check_refs?: string[];
  version: number;
  created_at: string;
  updated_at: string;
  /** Optional deadline (§4.1). Rendered in the Global Work view when present. */
  due_at?: string | null;
}

export function workLifecycleLabel(work?: Work | null): string {
  if (!work) return "unassigned";
  if (work.condition !== "normal") return work.condition;
  if (work.phase === "closed") return work.resolution ?? "closed";
  return work.phase;
}

export function workIsTerminal(work: Work): boolean {
  return work.phase === "closed";
}

export function workIsAccepted(work: Work): boolean {
  return work.phase === "closed" && work.resolution === "accepted";
}

export interface WorkEvent {
  id: string;
  team_run_id: string;
  work_id: string;
  sequence: number;
  kind: string;
  expected_version: number;
  resulting_version: number;
  performed_by_actor: TeamActorRef;
  authority_actor?: TeamActorRef | null;
  causation_ref?: { kind?: string; id?: string } | null;
  idempotency_key: string;
  payload?: unknown;
  created_at: string;
}

export interface WorkDelivery {
  authority: "canonical_trust" | "legacy_compatibility";
  read_only: true;
  execution_space_id?: string | null;
  team_run_id: string;
  work_id: string;
  work_revision: number;
  work_execution_binding_id?: string | null;
  delivery_id: string;
  recipient_agent_member_id?: string | null;
  recipient_member_run_id?: string | null;
  recipient_agent_session_id?: string | null;
  recipient_agent_session_generation?: number | null;
  target_node_id?: string | null;
  status: "queued" | "claimed" | "provider_received" | "failed" | "expired" | "invalidated";
  attempt: number;
  claim_id?: string | null;
  claimed_node_daemon_generation?: number | null;
  provider_receipt_id?: string | null;
  failure_code?: string | null;
  version: number;
  created_at: string;
  updated_at: string;
  integrity_annotations?: string[];
}

export type WorkDelegationState = "active" | "blocked" | "completed" | "failed" | "cancelled";

export interface WorkDelegation {
  id: string;
  source_work_ref: { team_run_id: string; work_id: string };
  source_work_version: number;
  source_owner_member_id: string;
  created_by_member_run_id?: string | null;
  target_agent_team_id: string;
  target_work_ref: { team_run_id: string; work_id: string };
  delegated_by_actor: TeamActorRef;
  state: WorkDelegationState | string;
  resolution_summary?: string | null;
  blocker_reason?: string | null;
  version: number;
  created_at: string;
  updated_at: string;
}

export interface ExecutionNode {
  id: string;
  display_name: string;
  status: "active" | "draining" | "retired" | string;
  created_at: string;
  updated_at: string;
}

export interface NodeProjectRegistration {
  node_id: string;
  execution_space_id: string;
  project_binding_id: string;
  status: "active" | "disabled" | string;
  created_at: string;
  updated_at: string;
}

export interface NodeDaemonLease {
  node_id: string;
  daemon_id: string;
  generation: number;
  instance_id: string;
  status: "active" | "draining" | "released" | "expired" | string;
  acquired_unix_ms: number;
  renewed_unix_ms: number;
  expires_unix_ms: number;
  released_unix_ms?: number | null;
}

export interface TeamSupervisorLease {
  team_run_id: string;
  node_id: string;
  node_daemon_id: string;
  node_daemon_generation: number;
  execution_space_id: string;
  project_binding_id: string;
  supervisor_id: string;
  generation: number;
  owner_process_id: number;
  owner_locator: string;
  status: "active" | "released" | string;
  acquired_unix_ms: number;
  heartbeat_unix_ms: number;
  expires_unix_ms: number;
  released_unix_ms?: number | null;
}

export interface TeamMemberCloseRequest {
  id: string;
  team_run_id: string;
  member_run_id: string;
  requested_by: string;
  reason: string;
  status: "pending" | "applied" | string;
  requested_at: string;
  applied_at?: string | null;
}

/** One recorded action of a member run (tool call, progress note, …). */
export interface MemberAction {
  id: string;
  seq?: number;
  team_run_id?: string;
  member_run_id?: string;
  action_type?: string;
  provider_call_id?: string | null;
  status?: "started" | "progress" | "succeeded" | "failed" | "cancelled" | string;
  provider_status?: string | null;
  semantic_status?: string | null;
  title?: string;
  summary?: string;
  evidence_refs?: string[];
  started_at?: string;
  completed_at?: string | null;
}

/**
 * A delegation spawned from a member run. `mode === "provider_native"` means the
 * provider spawned it on its own and the harness only CAPTURED it; every other
 * mode is orchestrated BY the harness.
 */
export interface DelegationRun {
  id: string;
  team_run_id?: string;
  parent_member_run_id?: string;
  mode?: "provider_native" | "harness_worker" | string;
  provider?: string | null;
  provider_child_thread_id?: string | null;
  objective?: string | null;
  status?: string;
  evidence_ids?: string[];
  created_at?: string;
  updated_at?: string;
}

/** One entry in a team run's event log (created/updated/completed on run entities). */
export interface TeamRunEvent {
  id: string;
  seq?: number;
  team_run_id?: string;
  source_kind?: "host" | "member" | "delegation" | "operator" | "service" | string;
  member_run_id?: string | null;
  delegation_run_id?: string | null;
  entity_type?: string;
  entity_id?: string;
  operation?: "created" | "updated" | "completed" | string;
  summary?: string;
  occurred_at?: string;
}

/**
 * Server build/data provenance (`GET /v1/meta`). Lets the dashboard prove
 * which build served it, which coordination store it read, and how far that
 * store's operation log has advanced — the second occurrence of "panel shows
 * something other than Store truth" (issue #307) was a stale frontend build
 * with no way to detect itself; this is the cross-check.
 */
export interface HarnessMeta {
  /** The commit the *server* binary was built from ("unknown" if undeterminable). */
  git_rev: string;
  /** When the server binary was compiled, `unix-ms:<millis>`, or null if undeterminable. */
  built_at: string | null;
  /** Absolute path to the coordination store this exact response read. */
  store_root: string;
  /** Monotonic cursor over the store's WorkOperation log; only ever grows. */
  latest_op_seq: number;
  /** harness-cli's own crate version. */
  server_version: string;
  build_sha: string;
  node_id: string | null;
  daemon_generation: number | null;
  protocol_version: "agentfirm-member-trust/1";
  schema_version: "agentfirm.role_views.v1";
  action_manifest_version: "agentfirm.role_actions.v1";
  capability_auth: "x-agentfirm-token";
}

export interface DashboardSnapshot {
  generated_at?: string;
  company_os?: CompanyOsSnapshotProjection;
  teams?: AgentTeam[];
  /** Optional direct projection for forward compatibility; current server
   * authority is `company_os.agent_members`. */
  agent_members?: AgentMember[];
  members?: ProviderLaunchProfile[];
  messages?: Message[];
  events?: ProviderDispatchEvent[];
  evidence?: Evidence[];
  provider_child_threads?: ProviderChildThread[];
  /**
   * Transient, client-only member previews keyed by member_run_id. New SSE
   * frames replace the prior preview; refresh/reconnect starts empty.
   */
  live_member_activity?: Record<string, LiveMemberActivity>;
  /** Native durable Mission rows. */
  missions?: Mission[];
  /** ADR 0051 pre-cutover rows, exposed only in an isolated historical view. */
  legacy_waves?: LegacyWave[];
  /** Append-only Mission Log rows (ADR 0051): the Host's versioned judgment,
   * replacing Wave as the write path. Every row is a permanent entry, not a
   * latest-wins projection. */
  mission_log?: MissionLogEntry[];
  /** Agent Team runs (team-console): host-orchestrated member groups. */
  team_runs?: TeamRun[];
  member_runs?: MemberRun[];
  team_messages?: TeamMessageProjection[];
  /** Development batch Wave 4C canonical runtime/message fabric. Legacy `team_messages` is read-only history. */
  agent_identities?: AgentIdentity[];
  agent_sessions?: AgentSession[];
  team_memberships?: TeamMembership[];
  work_execution_bindings?: WorkExecutionBinding[];
  canonical_messages?: CanonicalMessage[];
  canonical_message_deliveries?: CanonicalMessageDelivery[];
  works?: Work[];
  work_events?: WorkEvent[];
  work_deliveries?: WorkDelivery[];
  work_delegations?: WorkDelegation[];
  execution_nodes?: ExecutionNode[];
  node_project_registrations?: NodeProjectRegistration[];
  node_daemon_leases?: NodeDaemonLease[];
  team_supervisor_leases?: TeamSupervisorLease[];
  team_member_close_requests?: TeamMemberCloseRequest[];
  member_actions?: MemberAction[];
  delegation_runs?: DelegationRun[];
  team_run_events?: TeamRunEvent[];
}

export type DashboardAction = (path: string, body?: unknown) => Promise<void>;
