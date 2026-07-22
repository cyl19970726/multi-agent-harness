export type DeliveryStatus = "queued" | "delivered" | "acknowledged" | "failed";
/**
 * One project in the multi-project control plane (goal-multi-project P6). The
 * backend `GET /v1/projects` enumerates the registry + on-disk stores + the
 * reserved `_global` project; each entry mirrors the Rust `ProjectContext` plus
 * an `is_current` marker. `kind` is `"global"` only for the reserved `~/` store.
 */
export interface Project {
  id: string;
  project_root: string;
  store_root: string;
  kind: "repo" | "global";
  is_git_repo: boolean;
  is_current: boolean;
}

export type MessageKind = "message" | "task" | "report";
export type SenderKind = "agent" | "operator" | "system";
export type ProviderExecutionStatus = "queued" | "running" | "succeeded" | "failed" | "canceled" | "stale";

/**
 * The backend's four-layer runtime health snapshot (serialized
 * `AgentRuntimeHealth`). A `null`/missing probe means "unknown" — it must NOT
 * be rendered as healthy/green; treat it as amber.
 */
export interface RuntimeHealth {
  process_alive?: boolean;
  socket_exists?: boolean;
  protocol_probe?: string | null;
  delivery_probe?: string | null;
  checked_at?: string | null;
}

export interface AgentMember {
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
  provider_config?: AgentProviderConfig | null;
  created_at?: string | null;
  last_seen_at?: string | null;
  queued_count?: number;
  inbox_count?: number;
  team_ids?: string[];
  provider_child_thread_count?: number;
}

/** Provider launch/runtime config carried on an AgentMember (mirrors the Rust
 * AgentProviderConfig). All optional; the Config tab renders what is set and
 * shows "Not configured" otherwise. */
export interface AgentProviderConfig {
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
  owner_agent_id?: string;
  status?: "active" | "closed" | "archived";
  member_ids?: string[];
}

export interface Message {
  id: string;
  from_agent_id?: string;
  to_agent_id?: string | null;
  channel?: string | null;
  kind: MessageKind;
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

export interface AgentEvent {
  id: string;
  agent_member_id?: string;
  provider_runtime_id?: string | null;
  event_type?: string;
  summary?: string;
  payload_ref?: string | null;
  created_at?: string;
}

export interface ProviderChildThread {
  id: string;
  provider?: string;
  agent_member_id?: string;
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

/** Durable intent container for one or more ordered Waves. */
export interface Mission {
  id: string;
  title: string;
  objective: string;
  desired_outcome?: string | null;
  status?: MissionStatus | string;
  wave_ids?: string[];
  outcome_summary?: string | null;
  completed_by?: string | null;
  created_at?: string;
  updated_at?: string;
  completed_at?: string | null;
}

/** Executor selected by a Wave; each retains its own runtime semantics. */
export type WaveExecutorKind = "agent_team" | "dynamic_workflow" | "host";

/** Lifecycle of a Wave, independent from its lightweight acceptance gate. */
export type WaveStatus =
  | "planned"
  | "running"
  | "waiting"
  | "completed"
  | "blocked"
  | "failed"
  | "cancelled";

/** Lightweight Wave gate state. */
export type WaveGateStatus = "pending" | "accepted" | "revise" | "blocked";

/** One ordered, lightweight unit of a Mission. */
export interface Wave {
  id: string;
  mission_id: string;
  index: number;
  title: string;
  objective: string;
  exit_criteria?: string | null;
  status?: WaveStatus | string;
  executor_kind: WaveExecutorKind | string;
  executor_run_ids?: string[];
  accepted_run_id?: string | null;
  plan_note?: string | null;
  outcome_summary?: string | null;
  artifact_refs?: string[];
  gate_status?: WaveGateStatus | string;
  gate_note?: string | null;
  accepted_by?: string | null;
  accepted_at?: string | null;
  created_at?: string;
  updated_at?: string;
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
 * One Agent Team run: a host-orchestrated group of member runs working one
 * objective in waves. Wire shape is snake_case; timestamps are "unix-ms:<ms>"
 * strings like the rest of the snapshot.
 */
export interface TeamRun {
  id: string;
  definition_id?: string | null;
  /** Native Mission context when this is an executor attempt. */
  mission_id?: string | null;
  /** Native Wave context when this is an executor attempt. */
  wave_id?: string | null;
  /** Retry lineage: the previous attempt of this same native Wave, if any. */
  previous_run_id?: string | null;
  host_surface?: string | null;
  host_thread_id?: string | null;
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
  | "reviewing"
  | "blocked"
  | "completed"
  | "failed"
  | "stopped";

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

/** One member's participation in a {@link TeamRun}. */
export interface MemberRun {
  id: string;
  team_run_id?: string;
  slot_id?: string | null;
  name?: string | null;
  role?: string | null;
  provider?: "codex" | "claude" | "kimi" | string;
  model?: string | null;
  provider_profile?: ProviderIntegrationProfile | null;
  status?: MemberRunStatus | string;
  native_session?: NativeSessionRef | null;
  /** Optional member-specific Git worktree override of the TeamRun execution root. */
  worktree_ref?: string | null;
  workspace_snapshot?: MemberWorkspaceSnapshot | null;
  owned_paths?: string[];
  started_at?: string;
  last_event_at?: string | null;
  finished_at?: string | null;
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

export interface ProviderIntegrationProfile {
  provider: string;
  execution_mode: string;
  provider_version?: string | null;
  adapter_contract_version?: string | null;
  reviewed_provider_versions?: string[];
  compatibility_status?: "current" | "review_required" | "incompatible" | "unavailable" | "unknown" | string;
  adapter_reviewed_at?: string | null;
  compatibility_note?: string | null;
  interaction_mode: "pause_and_resume" | "end_round_and_follow_up" | "unsupported" | string;
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

/** Delivery of a {@link TeamMessage} to one recipient. */
export interface TeamMessageDelivery {
  member_id?: string;
  policy?: string;
  status?: "queued" | "delivered" | "acknowledged" | "failed" | "expired" | string;
  attempt?: number;
  updated_at?: string;
}

/** Kind of a {@link TeamMessage} (open enum; rendered as a colored pill). */
export type TeamMessageKind =
  | "assignment"
  | "question"
  | "answer"
  | "progress"
  | "blocker"
  | "handoff"
  | "review_request"
  | "review_result"
  | "control"
  | "broadcast";

/**
 * One message on a team run's handoff chain. `from_member_id` is `"host"` or a
 * member run id; `deliveries` tracks per-recipient ack state (an unacknowledged
 * delivery is a needs-you signal for the operator).
 */
export interface TeamMessage {
  id: string;
  team_run_id?: string;
  from_member_id?: string;
  to_member_ids?: string[];
  kind?: TeamMessageKind | string;
  body?: string;
  correlation_id?: string | null;
  causation_id?: string | null;
  evidence_refs?: string[];
  deliveries?: TeamMessageDelivery[];
  created_at?: string;
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

export interface PendingInteractionOption {
  id: string;
  label: string;
  intent?: string | null;
}

export interface PendingInteraction {
  id: string;
  team_run_id: string;
  member_run_id: string;
  provider: string;
  provider_request_id: string;
  method: string;
  kind: "question" | "tool_approval" | "plan_review" | "unknown" | string;
  route: "lead" | "human" | "policy" | string;
  status: "pending" | "answered" | "approved" | "denied" | "dismissed" | "unsupported" | "cancelled" | string;
  title: string;
  prompt: string;
  options: PendingInteractionOption[];
  tool_call_id?: string | null;
  response_option_id?: string | null;
  response_text?: string | null;
  created_at: string;
  resolved_at?: string | null;
  resolved_by?: string | null;
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
  mode?: "provider_native" | "harness_worker" | "dynamic_workflow" | string;
  provider?: string | null;
  provider_child_thread_id?: string | null;
  workflow_run_id?: string | null;
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
  source_kind?: "host" | "member" | "delegation" | string;
  member_run_id?: string | null;
  delegation_run_id?: string | null;
  entity_type?: string;
  entity_id?: string;
  operation?: "created" | "updated" | "completed" | string;
  summary?: string;
  occurred_at?: string;
}

export interface DashboardSnapshot {
  generated_at?: string;
  teams?: AgentTeam[];
  members?: AgentMember[];
  messages?: Message[];
  events?: AgentEvent[];
  evidence?: Evidence[];
  provider_child_threads?: ProviderChildThread[];
  /**
   * Transient, client-only member previews keyed by member_run_id. New SSE
   * frames replace the prior preview; refresh/reconnect starts empty.
   */
  live_member_activity?: Record<string, LiveMemberActivity>;
  workflow_runs?: WorkflowRun[];
  workflow_steps?: WorkflowStep[];
  /** Native durable Mission rows. */
  missions?: Mission[];
  /** Native ordered Wave rows. */
  waves?: Wave[];
  /** Agent Team runs (team-console): host-orchestrated member groups. */
  team_runs?: TeamRun[];
  member_runs?: MemberRun[];
  team_messages?: TeamMessage[];
  member_actions?: MemberAction[];
  pending_interactions?: PendingInteraction[];
  delegation_runs?: DelegationRun[];
  team_run_events?: TeamRunEvent[];
}

/**
 * A registered (built-in) workflow's run-independent metadata, from
 * `GET /v1/workflows`. The catalog is fetched separately from the snapshot.
 */
export interface WorkflowDef {
  name: string;
  summary: string;
}

/** Lifecycle of a {@link WorkflowRun} (mirrors harness-core `WorkflowRunStatus`). */
export type WorkflowRunStatus =
  | "pending"
  | "running"
  | "paused"
  | "completed"
  | "failed";

/** Status of a single {@link WorkflowStep} (mirrors harness-core `WorkflowStepStatus`). */
export type WorkflowStepStatus =
  | "queued"
  | "running"
  | "completed"
  | "failed"
  | "cached";

export type WorkflowTerminalReason =
  | "canceled_by_operator"
  | "driver_exited"
  | "orphan_reaped"
  | "leaf_timeout"
  | "idle_timeout"
  | "provider_failed"
  | "verdict_failed"
  | "completed";

/**
 * One run of a built-in (registered) workflow. Mirrors harness-core
 * `WorkflowRun` (lib.rs:1261-1273) verbatim, snake_case. `step_ids` orders the
 * steps in the sequence they were started.
 */
export interface WorkflowRun {
  id: string;
  workflow_name: string;
  status: WorkflowRunStatus | string;
  step_ids: string[];
  created_at: string;
  ended_at?: string | null;
  summary?: string | null;
  /** JSON parameterization the dynamic `run-script` program was authored with. */
  args?: unknown;
  /** How many agent steps this run spawned (the per-run agent count). */
  agents_spawned?: number;
  /** Collected structured output of the run (one entry per step). */
  final_output?: unknown;
  /**
   * Who initiated this run — an agent member id (a Codex / Claude member) or
   * "operator" for a human-triggered CLI run. `undefined` for legacy rows that
   * predate the field.
   */
  initiated_by?: string | null;
  /**
   * The mandatory `design_intent` a Starlark program declares via its
   * `workflow(name, design_intent)` header — the WHY behind the run's shape.
   * `undefined` for registry runs / legacy rows.
   */
  design_intent?: string | null;
  /**
   * The authored source the dynamic `run-script` path was run with — the raw
   * Starlark program text snapshotted as `{ lang: "starlark", script }`, the
   * small durable audit record of the run shape. `undefined` for registry runs /
   * legacy rows.
   */
  spec?: unknown;
  /**
   * True when this run was a `--dry-run` validation (mock driver, no provider
   * spawned, no tokens). Surfaced as a "dry-run" badge so a validation run is
   * never mistaken for a real one. `undefined`/false for live and legacy rows.
   */
  dry_run?: boolean;
  terminal_reason?: WorkflowTerminalReason | string | null;
  partial_output_available?: boolean;
}

/**
 * Normalized token usage for one worker turn (mirrors the Rust `TokenUsage`).
 * `total` is `input + output`; provider subset counters (codex cached /
 * reasoning, claude cache_*) are NOT re-added.
 */
export interface WorkflowStepTokens {
  input: number;
  output: number;
  total: number;
}

/**
 * Structured failure carried on a step's {@link WorkflowStepResult} when it did
 * NOT succeed. `reason` is the classified bucket; `detail` is human-facing
 * context (typically the worker's stderr).
 */
export interface WorkflowStepFailure {
  failed: boolean;
  /** Classified failure bucket. */
  reason: "timeout" | "exit" | "spawn" | "delivery" | string;
  detail: string;
}

/**
 * Structured result payload carried on a {@link WorkflowStep}. Mirrors the
 * harness-workflow `step_result_json` shape PLUS the observability fields the
 * runtime captures onto each step (see `build_step_details` in harness-cli). The
 * step's actor is a PROVIDER that ran in a NEW one-shot ephemeral worker
 * (codex/claude), not a pre-existing member; `isolation` is set when the node
 * opted into a throwaway git worktree.
 */
export interface WorkflowStepResult {
  phase?: string;
  label?: string;
  /** The provider that ran this step ("codex" | "claude" | "kimi"). */
  provider?: string;
  /** Per-node isolation mode this step ran under, if any ("worktree"). */
  isolation?: string | null;
  ok?: boolean;
  native_session?: NativeSessionRef | null;
  output_summary?: string;
  /** The model the worker actually ran (the requested override), if any. */
  model?: string | null;
  /** Process exit code; null when the worker was killed on timeout / signal. */
  exit_code?: number | null;
  /** Wall-clock duration of the worker process, in milliseconds. */
  duration_ms?: number;
  /** Normalized token usage parsed from the worker's terminal event, if present. */
  tokens?: WorkflowStepTokens | null;
  /** The provider's exact billed cost in USD for the turn, when reported (claude
   * `total_cost_usd`). Absent for codex, which emits only token usage. */
  cost_usd?: number | null;
  /** Present only when the step failed; describes why. */
  failure?: WorkflowStepFailure | null;
  /**
   * The FULL worktree diff text for an `isolation: "worktree"` step, capped to
   * 20k chars. Absent for shared-cwd steps. See {@link worktree_diff_truncated}.
   */
  worktree_diff?: string;
  /** True when {@link worktree_diff} was truncated at the cap. */
  worktree_diff_truncated?: boolean;
  structured?: unknown;
  schema_attempt_count?: number;
  selected_json_index?: number | null;
  schema_candidate_count?: number;
  empty_field_count?: number;
  schema_strict?: boolean;
}

/**
 * One agent step inside a {@link WorkflowRun}. Mirrors harness-core
 * `WorkflowStep` (lib.rs:1279-1292) verbatim, snake_case. There is NO
 * `member_id`; the step actor is a PROVIDER carried in `result.provider`, and
 * provider-native activity resolves via `native_session`.
 */
export interface WorkflowStep {
  id: string;
  run_id: string;
  phase: string;
  label: string;
  native_session?: NativeSessionRef | null;
  status: WorkflowStepStatus | string;
  output_summary?: string | null;
  /** Structured result for this step, beyond the human-facing summary. */
  result?: WorkflowStepResult | null;
  started_at: string;
  ended_at?: string | null;
  terminal_reason?: WorkflowTerminalReason | string | null;
  partial?: boolean;
}

export type DashboardAction = (path: string, body?: unknown) => Promise<void>;
