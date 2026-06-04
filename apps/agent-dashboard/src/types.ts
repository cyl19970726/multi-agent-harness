export type TaskStatus = "planned" | "assigned" | "running" | "blocked" | "review" | "done" | "archived";
/**
 * Goal lifecycle. Product columns are active/blocked/review/done; `complete`
 * (legacy alias folded into `done`) and `archived` stay valid for old rows but
 * are not shown as columns. See ADR 0019.
 */
export type GoalStatus = "active" | "blocked" | "review" | "done" | "complete" | "archived";
export type DeliveryStatus = "queued" | "delivered" | "acknowledged" | "failed";

/**
 * Shared git/worktree context (ADR 0019). Additive-optional; on Goal and Task.
 * The read model prefers these over the Task flat git fields where both exist.
 */
export interface GitMetadata {
  repo?: string | null;
  worktree_path?: string | null;
  branch?: string | null;
  base_branch?: string | null;
  pr_ref?: string | null;
  commit?: string | null;
  owned_paths?: string[];
}
export type MessageKind = "message" | "task" | "report";
export type SenderKind = "agent" | "operator" | "system";
export type ProviderSessionStatus = "queued" | "running" | "succeeded" | "failed" | "canceled" | "stale";

export interface Goal {
  id: string;
  title?: string;
  objective?: string;
  owner_agent_id?: string;
  status?: string;
  priority?: string;
  success_criteria?: string[];
  created_at?: string;
  updated_at?: string;
  vision_id?: string | null;
  goal_design_id?: string | null;
  closed_by_decision_id?: string | null;
  git_metadata?: GitMetadata | null;
}

export interface Task {
  id: string;
  goal_id?: string | null;
  parent_task_id?: string | null;
  title?: string;
  objective?: string;
  owner_agent_id?: string;
  assignee_agent_id?: string | null;
  reviewer_agent_id?: string | null;
  status: TaskStatus;
  /** Full task write-up (markdown). `objective` stays the one-line summary. */
  description?: string | null;
  depends_on_task_ids?: string[];
  workspace_ref?: string | null;
  branch_ref?: string | null;
  pr_ref?: string | null;
  owned_paths?: string[];
  acceptance_criteria?: string[];
  created_at?: string;
  updated_at?: string;
  phase?: string | null;
  scope_refs?: string[];
  requires_human_approval?: boolean;
  verdict_decision_id?: string | null;
  git_metadata?: GitMetadata | null;
}

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
  provider_thread_id?: string | null;
  provider_agent_path?: string | null;
  provider_agent_nickname?: string | null;
  provider_agent_role?: string | null;
  current_task_id?: string | null;
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

/** Per-agent activity stats derived client-side from provider_sessions
 * (computeAgentStats). No backend aggregate; powers the list sparkline/run
 * count and the detail Tasks-tab performance summary. */
export interface AgentStats {
  runCount30d: number;
  runsTotal: number;
  succeeded: number;
  failed: number;
  successRate: number | null;
  avgDurationMs: number | null;
  activity7d: number[];
  lastActiveMs: number | null;
  runningCount: number;
  liveSessionId: string | null;
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
  task_id?: string | null;
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
  provider_session_id?: string | null;
  provider_request_id?: string | null;
  provider_thread_id?: string | null;
  provider_turn_id?: string | null;
  terminal_source?: string | null;
  delivered_at?: string | null;
  last_error?: string | null;
}

export interface ProviderSession {
  id: string;
  provider?: string;
  agent_member_id?: string;
  task_id?: string | null;
  workspace_ref?: string | null;
  provider_thread_id?: string | null;
  provider_turn_id?: string | null;
  terminal_source?: string | null;
  status?: ProviderSessionStatus | string;
  command?: string;
  args?: string[];
  prompt_ref?: string | null;
  prompt_summary?: string | null;
  provider_session_ref?: string | null;
  exit_code?: number | null;
  stdout_ref?: string | null;
  stderr_ref?: string | null;
  jsonl_ref?: string | null;
  transcript_ref?: string | null;
  last_message_ref?: string | null;
  started_at?: string;
  ended_at?: string | null;
  evidence_ids?: string[];
}

export interface AgentEvent {
  id: string;
  agent_member_id?: string;
  provider_runtime_id?: string | null;
  task_id?: string | null;
  event_type?: string;
  summary?: string;
  payload_ref?: string | null;
  created_at?: string;
}

export interface Proposal {
  id: string;
  task_id: string;
  agent_member_id?: string;
  title?: string;
  summary?: string;
  status?: string;
  changed_paths?: string[];
  evidence_ids?: string[];
}

export interface ProviderChildThread {
  id: string;
  provider?: string;
  agent_member_id?: string;
  provider_runtime_id?: string | null;
  task_id?: string | null;
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
  task_id?: string | null;
  source_type?: string;
  source_ref?: string;
  summary?: string;
  evidence_kind?: string | null;
  goal_id?: string | null;
}

export interface Decision {
  id: string;
  task_id: string;
  decision?: string;
  rationale?: string;
  evidence_ids?: string[];
  decision_kind?: string | null;
  goal_id?: string | null;
  is_waiver?: boolean;
  follow_up_task_id?: string | null;
}

export interface Review {
  id: string;
  task_id?: string | null;
  goal_id?: string | null;
  reviewer_agent_id?: string;
  review_kind?: string;
  /** Open enum: pass/fail/blocked/needs_changes, or an adapter-supplied value. */
  verdict?: string;
  summary?: string;
  blockers?: string[];
  residual_risk?: string | null;
  missing_validation?: string[];
  evidence_ids?: string[];
  created_at?: string;
}

/**
 * Gap ledger entry (absorbs the bug ledger: a Bug is a Gap with category="bug").
 * `category` is an open enum (free string); `severity`/`status` are closed,
 * harness-owned sets matching the Rust hard enums.
 */
export interface Gap {
  id: string;
  goal_id?: string | null;
  task_id?: string | null;
  /** Open enum: ux/data/observability/parity/tooling/workflow/docs/bug/other, or adapter-supplied. */
  category?: string;
  severity?: "p0" | "p1" | "p2" | string;
  status?: "open" | "in_progress" | "fixed" | "blocked" | "deferred" | "wontfix" | string;
  summary?: string;
  evidence_ids?: string[];
  next_step?: string | null;
  owner_agent_id?: string | null;
  repro_ref?: string | null;
  closing_test_ref?: string | null;
  created_at?: string;
  updated_at?: string;
}

/**
 * Executable thesis for a Goal (the generic subset of the strategy-creation
 * checklist). Graduates from `Evidence(source_type=goal_design)`; both
 * representations coexist (dual-read by goal_id, no backfill).
 */
export interface GoalDesign {
  id: string;
  goal_id: string;
  scenario_summary?: string;
  non_goals?: string[];
  risk_and_permission_boundaries?: string;
  required_infra?: string[];
  agent_team?: string | null;
  task_graph?: string[];
  evidence_plan?: string[];
  acceptance_gates?: string[];
  created_at?: string;
}

/** Retrospective for a Goal: what worked / failed, reusable patterns, follow-ups. */
export interface GoalEvaluation {
  id: string;
  goal_id: string;
  evaluator_agent_id?: string;
  /** Open enum: success/partial/failed/blocked, or an adapter-supplied value. */
  outcome?: string;
  what_worked?: string;
  what_failed?: string;
  missing_infra?: string[];
  missing_evidence?: string[];
  team_design_feedback?: string;
  task_graph_feedback?: string;
  dashboard_feedback?: string;
  reusable_patterns?: string[];
  anti_patterns?: string[];
  follow_up_task_ids?: string[];
  proposed_goal_ids?: string[];
  created_at?: string;
}

/** Reusable teaching artifact distilled from a completed Goal. */
export interface GoalCase {
  case_id: string;
  source_goal_id: string;
  scenario_type?: string;
  project_adapter?: string | null;
  goal_design_ref?: string | null;
  evaluation_ref?: string | null;
  reusable_patterns?: string[];
  anti_patterns?: string[];
  follow_up_refs?: string[];
  tags?: string[];
  created_at?: string;
}

/** A durable product vision a Goal can be scheduled against (Goal.vision_id). */
export interface Vision {
  id: string;
  summary?: string;
  source_refs?: string[];
  created_at?: string;
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

export interface AutonomousProposal {
  id: string;
  kind?: string;
  source_type?: string;
  source_ref?: string;
  summary?: string;
  task_id?: string | null;
  goal_id?: string | null;
  created_at?: string;
  message_id?: string | null;
  from_agent_id?: string | null;
  to_agent_id?: string | null;
  linked_evidence_ids?: string[];
  disposition?: "pending" | "accepted" | "rejected" | "deferred" | "request_evidence" | "decided" | string;
  decision_id?: string | null;
  decision_rationale?: string | null;
  follow_up_task_ids?: string[];
  follow_up_goal_ids?: string[];
}

export interface GoalLearningStatus {
  goal_id: string;
  ok?: boolean;
  warnings?: string[];
  task_ids?: string[];
  /** Legacy representation: learning artifacts carried as Evidence rows. */
  goal_design?: unknown[];
  goal_evaluation?: unknown[];
  goal_cases?: { source_ref?: string; id?: string }[];
  /** Graduated representation: first-class learning objects (dual-read union). */
  goal_design_objects?: GoalDesign[];
  goal_evaluation_objects?: GoalEvaluation[];
  goal_case_objects?: GoalCase[];
  follow_up_tasks?: Task[];
  member_reports?: unknown[];
  decisions?: unknown[];
  /** Closeout-gate readiness (§3.7): surfaced so the UI can render the gate. */
  closeout_decisions?: Decision[];
  closeout_waivers?: Decision[];
  has_closeout_decision?: boolean;
  has_evaluation?: boolean;
  has_closeout_waiver?: boolean;
  may_close?: boolean;
  closeout_blockers?: string[];
  event_order?: Record<string, unknown>;
}

export interface DashboardSnapshot {
  generated_at?: string;
  goals?: Goal[];
  teams?: AgentTeam[];
  members?: AgentMember[];
  kanban?: Record<TaskStatus, string[]>;
  tasks?: Task[];
  messages?: Message[];
  events?: AgentEvent[];
  proposals?: Proposal[];
  autonomous_proposals?: AutonomousProposal[];
  evidence?: Evidence[];
  decisions?: Decision[];
  reviews?: Review[];
  gaps?: Gap[];
  goal_designs?: GoalDesign[];
  goal_evaluations?: GoalEvaluation[];
  goal_cases?: GoalCase[];
  visions?: Vision[];
  provider_sessions?: ProviderSession[];
  provider_child_threads?: ProviderChildThread[];
  goal_learning_status?: GoalLearningStatus[];
  /**
   * Transient, client-only: raw provider turn events pushed live via SSE
   * (provider_turn_event), keyed by session id. Never sent by the backend
   * snapshot; accumulated by applyFrame so the agent TUI streams sub-second.
   */
  live_turn_events?: Record<string, Record<string, unknown>[]>;
  workflow_runs?: WorkflowRun[];
  workflow_steps?: WorkflowStep[];
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
   * Retention policy for the heavy per-node provider turn-event trace:
   * "durable" (default) persists the trace so a completed run can be drilled
   * into; "live" streams it over SSE during execution but does not retain it.
   * Live streaming is independent of this and always happens.
   */
  trace_retention?: "durable" | "live" | string;
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
  /** The provider that ran this step ("codex" | "claude"). */
  provider?: string;
  /** Per-node isolation mode this step ran under, if any ("worktree"). */
  isolation?: string | null;
  ok?: boolean;
  provider_session_id?: string | null;
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
}

/**
 * One agent step inside a {@link WorkflowRun}. Mirrors harness-core
 * `WorkflowStep` (lib.rs:1279-1292) verbatim, snake_case. There is NO
 * `member_id`; the step actor is a PROVIDER carried in `result.provider`, and
 * the live turn drill-in resolves via `provider_session_id`.
 */
export interface WorkflowStep {
  id: string;
  run_id: string;
  phase: string;
  label: string;
  provider_session_id?: string | null;
  status: WorkflowStepStatus | string;
  output_summary?: string | null;
  /** Structured result for this step, beyond the human-facing summary. */
  result?: WorkflowStepResult | null;
  started_at: string;
  ended_at?: string | null;
}

export interface WorkflowWarning {
  id: string;
  kind: string;
  severity: "high" | "medium" | "low";
  goalId?: string;
  taskId?: string;
  memberId?: string;
  proposalId?: string;
  decisionId?: string;
  sessionId?: string;
  evidenceId?: string;
  summary: string;
}

export type DashboardAction = (path: string, body?: unknown) => Promise<void>;
