export type TaskStatus =
  | "planned"
  | "assigned"
  | "running"
  | "blocked"
  | "review"
  | "done"
  | "archived"
  | "superseded";
/**
 * Goal lifecycle. Product columns are active/blocked/review/done; `complete`
 * (legacy alias folded into `done`) and `archived` stay valid for old rows but
 * are not shown as columns. See ADR 0019.
 */
export type GoalStatus = "active" | "blocked" | "review" | "done" | "complete" | "archived";
/**
 * Markdown-first Goal lifecycle stage (distinct from the legacy `GoalStatus`
 * kanban). Phases: exploration (exploring→explored), work (working→done),
 * acceptance (verifying→verified).
 */
export type GoalStage =
  | "draft"
  | "exploring"
  | "explored"
  | "working"
  | "done"
  | "verifying"
  | "verified";
/** One multi-agent / multi-round exploration note feeding `design_md`. */
export interface Exploration {
  author: string;
  round?: number;
  notes_md: string;
  created_at: string;
}

/**
 * Status of one agent-planned phase (mirrors harness-core `GoalPhaseStatus`).
 * Phases run sequentially; a phase must reach `passed` (its verdict) before the
 * next begins.
 */
export type GoalPhaseStatus =
  | "not_started"
  | "in_progress"
  | "passed"
  | "failed"
  | "blocked";

export type GoalPhaseKind = "execution" | "remediation" | "building";

export type GoalPhaseExecutionMode = "task_graph" | "workflow";

/**
 * Kind of a declared artifact (goal-phase-artifacts; mirrors harness-core
 * `ArtifactKind`, serde snake_case). `code` is the default and matches today's
 * implicit behavior (a code diff). The multi-word kinds are the bar — they read
 * `design_doc`, `test_report`, `migration_doc`, `registered_doc` on the wire.
 */
export type ArtifactKind =
  | "design_doc"
  | "adr"
  | "code"
  | "test_report"
  | "migration_doc"
  | "registered_doc"
  | "screenshot"
  | "other";

/**
 * One declared deliverable of a phase or task (goal-phase-artifacts; mirrors
 * harness-core `ArtifactSpec`). Makes artifacts first-class and declarative so
 * the verdict gate can VERIFY a phase produced what it promised. `required`
 * defaults to TRUE on the wire; an empty `outputs[]` reproduces today's behavior
 * (the legacy `design_md` is the implicit `design_doc`, `acceptance` the gate).
 */
export interface ArtifactSpec {
  /** Stable handle for this artifact within its phase/task. */
  id: string;
  /** What kind of artifact this is (gate/render hint). Defaults to `code`. */
  kind?: ArtifactKind;
  /**
   * Where the artifact lands (repo-relative path; glob ok). When present and
   * `required`, the gate asserts it exists & is non-empty in the worktree diff.
   */
  path?: string | null;
  /** Why this artifact exists — the human/agent-readable intent. */
  purpose: string;
  /** Whether the gate must enforce this artifact. Defaults to TRUE. */
  required?: boolean;
  /** Optional per-artifact acceptance criterion (a finer gate than presence). */
  acceptance?: string | null;
}

/**
 * One agent-planned phase of a goal (goal-planning-model; mirrors harness-core
 * `GoalPhase`). It owns the tasks whose `phase_id == this.id`; `acceptance` is
 * the verdict gate before the next phase.
 */
export interface GoalPhase {
  id: string;
  name: string;
  intent: string;
  status: GoalPhaseStatus;
  /** Markdown gate condition (the verdict this phase must pass). */
  acceptance?: string | null;
  verdict_decision_id?: string | null;
  created_at: string;
  started_at?: string | null;
  ended_at?: string | null;
  /**
   * Artifacts this phase declares it will produce (goal-phase-artifacts). Empty
   * reproduces today's behavior; non-empty makes the verdict gate enforce each
   * `required` artifact's presence.
   */
  outputs?: ArtifactSpec[];
  /**
   * Artifacts a PRIOR phase must have landed for this phase to run
   * (goal-phase-refinements). Reuses {@link ArtifactSpec} (`path`/`purpose`/
   * `required` are what matter); before running, each REQUIRED input's `path`
   * must exist or the phase fails fast. Empty reproduces today's behavior.
   */
  inputs?: ArtifactSpec[];
  /**
   * Per-phase retry budget override (goal-phase-refinements). When set, this
   * phase's replan/rerun budget is this value instead of the global default;
   * `null`/absent falls back to the global. Additive + back-compat.
   */
  retry?: number | null;
  /**
   * The commit that landed this phase's writable work onto the goal's branch
   * (goal-phase-landing). Set by `run-phases` on a passing phase or by
   * `goal reconcile-phase` for out-of-band work; absent for read-only / legacy.
   */
  landed_commit?: string | null;
  kind?: GoalPhaseKind;
  builtin?: string | null;
  execution_mode?: GoalPhaseExecutionMode;
  workflow_ref?: string | null;
}

/** Where a {@link Knowledge} entry came from (mirrors `KnowledgeSource`). */
export type KnowledgeSource = "exploration" | "task" | "decision" | "evidence";

/**
 * One append-only learning in a goal's knowledge ledger (mirrors harness-core
 * `Knowledge`) — the durable truth `design_md` is synthesized from. Carries
 * provenance (`phase_id`/`task_id`/author) so "which task changed the plan,
 * when, by whom" is always answerable.
 */
export interface Knowledge {
  id: string;
  goal_id: string;
  phase_id?: string | null;
  task_id?: string | null;
  author: string;
  timestamp: string;
  notes_md: string;
  tags?: string[];
  source?: KnowledgeSource;
  superseded_by_knowledge_id?: string | null;
  created_at: string;
}
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
export type ProviderSessionStatus = "queued" | "running" | "succeeded" | "failed" | "canceled" | "stale";

export interface Goal {
  id: string;
  title?: string;
  owner_agent_id?: string;
  status?: string;
  priority?: string;
  created_at?: string;
  updated_at?: string;
  vision_id?: string | null;
  goal_design_id?: string | null;
  closed_by_decision_id?: string | null;
  git_metadata?: GitMetadata | null;
  // Markdown-first lifecycle (additive).
  stage?: GoalStage;
  description_md?: string | null;
  design_md?: string | null;
  acceptance_md?: string | null;
  explorations?: Exploration[];
  skill_refs?: string[];
  stage_changed_at?: string | null;
  /**
   * Agent-planned, SEQUENTIAL phases (goal-planning-model). Empty/absent for
   * legacy goals, which still render via the legacy stage bar. When non-empty,
   * the dashboard renders the phases timeline + per-phase task DAG instead.
   */
  phases?: GoalPhase[];
  /** Append-only knowledge ledger (the truth `design_md` is synthesized from). */
  knowledge?: Knowledge[];
  /** When `design_md` was last (re)synthesized from `knowledge[]`. */
  design_synthesis_at?: string | null;
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
  /** Full per-task design (goal-planning-model): a grounded slice of the goal's design. */
  design_md?: string | null;
  /** The {@link GoalPhase} id this task belongs to; `null` for legacy/unplaced tasks. */
  phase_id?: string | null;
  /** When `status === "superseded"`, the `Knowledge.id` whose finding killed it. */
  superseded_by_knowledge_id?: string | null;
  /** The `WorkflowStep`s that executed this task (reverse link). */
  workflow_step_ids?: string[];
  /**
   * Artifacts this task declares it will produce (goal-phase-artifacts). Empty
   * reproduces today's behavior (the implicit `design_md` doc + acceptance gate).
   */
  outputs?: ArtifactSpec[];
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

/**
 * Canonical, provider-agnostic turn-event vocabulary (mirrors harness-core
 * `HarnessTurnEventKind`, snake_case wire spelling). The dashboard renders off
 * `kind` so a new provider needs no new frontend branch — the backend
 * `ProviderAdapter::normalize_turn_event` maps its raw events onto these.
 */
export type HarnessTurnEventKind =
  | "turn_started"
  | "turn_completed"
  | "message_delta"
  | "message"
  | "tool_call"
  | "tool_result"
  | "reasoning"
  | "usage"
  | "error"
  | "provider_meta"
  | "unknown";

/** A normalized tool invocation (`tool_call` kind). */
export interface HarnessToolCall {
  id?: string;
  name: string;
  args: unknown;
}

/** A normalized tool result (`tool_result` kind). */
export interface HarnessToolResult {
  tool_call_id?: string;
  name?: string;
  content: string;
  is_error: boolean;
}

/** Normalized token usage (`usage`/`turn_completed` kinds). */
export interface HarnessTokenUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  cached_input_tokens?: number;
  reasoning_output_tokens?: number;
}

/**
 * One normalized turn event, from `GET /v1/provider-sessions/{id}/normalized-events`
 * (and the historical `/v1/sessions/{id}/normalized-events`) or the live
 * `provider_turn_event_normalized` SSE frame. `raw_provider_event` always
 * retains the original provider JSON so a "show raw" view loses nothing; `seq`
 * is a harness-assigned monotonic per-session counter used to merge/dedupe the
 * live stream against a fetched snapshot.
 */
export interface HarnessTurnEvent {
  session_id: string;
  provider: string;
  seq: number;
  ts: string;
  provider_thread_id?: string;
  provider_turn_id?: string;
  provider_item_id?: string;
  kind: HarnessTurnEventKind;
  role?: string;
  text?: string;
  delta?: string;
  tool_call?: HarnessToolCall;
  tool_result?: HarnessToolResult;
  usage?: HarnessTokenUsage;
  model?: string;
  duration_ms?: number;
  cost_usd?: number;
  status?: string;
  error?: string;
  raw_provider_event: unknown;
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
  created_at?: string;
  updated_at?: string;
  completed_at?: string | null;
}

/** Whether a Mission view is native product state or a read-only Goal bridge. */
export type MissionProjectionSource = "native" | "goal_compatibility" | string;

/**
 * A Mission read model during the non-destructive Goal migration. Compatibility
 * projections retain legacy phase provenance and do not pretend those phases
 * are Waves.
 */
export interface MissionProjection {
  mission: Mission;
  source: MissionProjectionSource;
  source_id: string;
  legacy_goal_phase_ids?: string[];
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
  status?: TeamRunStatus | string;
  wave_index?: number;
  member_run_ids?: string[];
  task_ids?: string[];
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

/** One member's participation in a {@link TeamRun}. */
export interface MemberRun {
  id: string;
  team_run_id?: string;
  slot_id?: string | null;
  name?: string | null;
  role?: string | null;
  provider?: "codex" | "claude" | "kimi" | string;
  model?: string | null;
  status?: MemberRunStatus | string;
  provider_session_id?: string | null;
  acp_session_id?: string | null;
  current_task_id?: string | null;
  worktree_ref?: string | null;
  owned_paths?: string[];
  started_at?: string;
  last_event_at?: string | null;
  finished_at?: string | null;
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
  task_id?: string | null;
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
  task_id?: string | null;
  action_type?: string;
  status?: "started" | "progress" | "succeeded" | "failed" | "cancelled" | string;
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
  parent_task_id?: string | null;
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
  /**
   * Transient, client-only: NORMALIZED turn events pushed live via SSE
   * (provider_turn_event_normalized, Stage B), keyed by session id and merged
   * by `seq` (latest-wins) so the canonical TUI streams sub-second and aligns
   * with the /normalized-events read endpoint. Never sent by the backend snapshot.
   */
  live_normalized_events?: Record<string, HarnessTurnEvent[]>;
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
  /** Native and read-only Goal compatibility Mission views. */
  mission_projections?: MissionProjection[];
  /**
   * The goal↔run orchestration checkpoints (Stage 0): each `goal run-phases`
   * execution and its per-phase `workflow_run_id` links — the BACK link to the
   * forward `goal_id`/`phase_id` now stamped on each {@link WorkflowRun}.
   */
  goal_orchestration_runs?: GoalOrchestrationRun[];
  /** Agent Team runs (team-console): host-orchestrated member groups. */
  team_runs?: TeamRun[];
  member_runs?: MemberRun[];
  team_messages?: TeamMessage[];
  member_actions?: MemberAction[];
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
  /**
   * True when this run was a `--dry-run` validation (mock driver, no provider
   * spawned, no tokens). Surfaced as a "dry-run" badge so a validation run is
   * never mistaken for a real one. `undefined`/false for live and legacy rows.
   */
  dry_run?: boolean;
  /**
   * The goal this run belongs to, when spawned by the goal orchestrator
   * (`goal plan` / `goal run-phases`). Together with `phase_id` this is the
   * FORWARD goal↔run link (Stage 0). `undefined` for standalone `run-script` /
   * registry runs and legacy rows.
   */
  goal_id?: string | null;
  /** The goal phase this run executed (`phase-*`) or revised (`revise-*`). */
  phase_id?: string | null;
}

/**
 * One phase's outcome inside a {@link GoalOrchestrationRun} — which compiled
 * workflow ran it and whether its verdict passed. Mirrors harness-core
 * `OrchestrationPhaseRun`.
 */
export interface OrchestrationPhaseRun {
  phase_id: string;
  workflow_run_id?: string | null;
  compiled_path?: string | null;
  passed: boolean;
  started_at: string;
  ended_at?: string | null;
  landed_commit?: string | null;
}

/**
 * The durable checkpoint for `harness goal run-phases`: it sequences a goal's
 * phases, gating each on its verdict, and records each phase run. Mirrors
 * harness-core `GoalOrchestrationRun`.
 */
export interface GoalOrchestrationRun {
  id: string;
  goal_id: string;
  status?: string;
  phase_runs?: OrchestrationPhaseRun[];
  created_at: string;
  updated_at: string;
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
