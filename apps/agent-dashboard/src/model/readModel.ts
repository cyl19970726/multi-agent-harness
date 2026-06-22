import type { SelectionState } from "../app/selection";
import type {
  AgentMember,
  AgentStats,
  AgentTeam,
  DashboardSnapshot,
  Decision,
  Evidence,
  Gap,
  GitMetadata,
  Goal,
  GoalCase,
  GoalDesign,
  GoalEvaluation,
  Message,
  Proposal,
  ProviderChildThread,
  ProviderSession,
  Review,
  SenderKind,
  Task,
  TaskStatus,
  Vision,
  WorkflowDef,
  WorkflowRun,
  GoalOrchestrationRun,
  WorkflowStep,
  WorkflowWarning,
} from "../types";
import { deriveWarnings } from "./warnings";

export interface TimelineItem {
  id: string;
  kind: "message" | "event" | "session" | "evidence" | "proposal" | "decision" | "review" | "warning";
  title: string;
  meta: string;
  body?: string;
  severity?: WorkflowWarning["severity"];
  objectRef?: string;
  createdAt?: string;
  /**
   * Structured re-skin metadata. The merged-timeline data layer is unchanged —
   * these optional fields just expose, in typed form, what was previously baked
   * into the `meta`/`title` strings so the chat-app view can render bubbles and
   * action cards without re-parsing text. Nothing here is a new data source.
   */
  /** Message direction relative to the selected member ("in" = inbound/agent). */
  direction?: "in" | "out";
  /** Raw message delivery_status, for the bubble's delivery chip. */
  deliveryStatus?: string;
  /** The other party in a message (from for inbound, to for outbound). */
  counterpartyId?: string;
  /** Message author id (raw from_agent_id), so a bubble can show who spoke. */
  fromAgentId?: string;
  /** Identity class of a message author ("operator" renders distinctly). */
  senderKind?: SenderKind;
  /**
   * Provider session that produced this message (from message.delivery), so an
   * agent reply bubble can drill into the RAW provider turn (claude/codex
   * stream events) on demand.
   */
  providerSessionId?: string;
  /** Provider-neutral provider label for a session row. */
  provider?: string;
  /** Raw provider-session status for a session block header/badge. */
  sessionStatus?: string;
  /** Session window bounds (used to nest rows and show duration). */
  startedAt?: string;
  endedAt?: string;
  /** Session thread/turn identifiers, surfaced on the session block header. */
  threadId?: string;
  turnId?: string;
  /** Review verdict (open enum), for the review action card. */
  verdict?: string;
  /** AgentEvent event_type, for the event action card icon/label. */
  eventType?: string;
  /** Optional count + noun a row can render ("Ran 3 commands" / "Edited 2 files"). */
  count?: number;
  countNoun?: string;
}

/**
 * One session block in the chat-app stream: a provider session and the timeline
 * rows whose time falls inside its window. `session` is undefined for the
 * default group that collects session-less rows (standalone operator messages).
 */
export interface MemberSessionGroup {
  id: string;
  session?: ProviderSession;
  items: TimelineItem[];
}

export interface RoleGroup {
  role: string;
  members: AgentMember[];
}

/**
 * The Lead's doctrinal loop, surfaced from existing canonical objects when the
 * selected member is the Lead. No new schema — each lane is a filtered view:
 *  - design goals: GoalDesign owned by this agent (via owning Goal.owner_agent_id)
 *  - assignments: outbox Message(kind="task") authored by this Lead
 *  - decisions authored: Decision rows for tasks the Lead owns
 *  - evaluations: GoalEvaluation owned by this agent (evaluator/owner)
 *  - team composition: member_ids of the teams this Lead owns
 */
export interface LeadResponsibilities {
  goalDesigns: GoalDesign[];
  assignments: Message[];
  decisions: Decision[];
  evaluations: GoalEvaluation[];
  teamMemberIds: string[];
}

export interface Lane {
  id: string;
  title: string;
  tasks: Task[];
}

export interface WorkbenchModel {
  snapshot: DashboardSnapshot;
  /** Snapshot freshness anchor (snapshot.generated_at); used by the TopBar chip. */
  generatedAt?: string;
  selectedGoal?: Goal;
  selectedTeam?: AgentTeam;
  selectedMember?: AgentMember;
  selectedTask?: Task;
  goals: Goal[];
  activeGoals: Goal[];
  completeGoals: Goal[];
  blockedGoals: Goal[];
  proposedGoals: Goal[];
  members: AgentMember[];
  roleGroups: RoleGroup[];
  tasks: Task[];
  goalTasks: Task[];
  lanes: Lane[];
  /** Derived dependency graph over ALL tasks (ready/waiting/edges). */
  taskGraph: TaskGraph;
  messages: Message[];
  evidence: Evidence[];
  proposals: Proposal[];
  decisions: Decision[];
  reviews: Review[];
  /** Reviews scoped to the selected task (task_id match). */
  reviewsForTask: Review[];
  /** Reviews scoped to the selected goal (goal_id match, or via the goal's tasks). */
  reviewsForGoal: Review[];
  /** All gaps, sorted by severity (p0→p2) then unresolved-first. */
  gaps: Gap[];
  /** Gaps grouped by goal_id (null/undefined goal collapses to ""). */
  gapsByGoal: Map<string, Gap[]>;
  /** Gaps grouped by severity (p0/p1/p2/other). */
  gapsBySeverity: Map<string, Gap[]>;
  /** GoalDesign objects grouped by goal_id (dual-read: top-level + goal_learning_status). */
  goalDesignByGoal: Map<string, GoalDesign[]>;
  /** GoalEvaluation objects grouped by goal_id. */
  goalEvaluationByGoal: Map<string, GoalEvaluation[]>;
  /** All GoalCase objects (graduated teaching artifacts). */
  goalCases: GoalCase[];
  /** All Vision objects. */
  visions: Vision[];
  /** GoalDesign objects for the selected goal. */
  goalDesignsForGoal: GoalDesign[];
  /** GoalEvaluation objects for the selected goal. */
  goalEvaluationsForGoal: GoalEvaluation[];
  /** GoalCase objects sourced from the selected goal. */
  goalCasesForGoal: GoalCase[];
  /** The Vision linked to the selected goal via Goal.vision_id, if any. */
  visionForGoal?: Vision;
  warnings: WorkflowWarning[];
  selectedMemberMessages: Message[];
  /** Messages addressed TO the selected member (to_agent_id match). */
  inboxMessages: Message[];
  /** Messages authored BY the selected member (from_agent_id match). */
  outboxMessages: Message[];
  selectedMemberTimeline: TimelineItem[];
  /** Reviews authored by the selected member (reviewer_agent_id match). */
  reviewsByMember: Review[];
  /**
   * The id of the team Lead for the selected member's team. Authoritative
   * source is `team.owner_agent_id`; this is frontend-derived, not a schema
   * field, so it can be used to render a Lead chip without inventing data.
   */
  leadMemberId?: string;
  /**
   * The selected member's Lead responsibilities, derived entirely from existing
   * canonical objects (no schema invention). Only populated when the selected
   * member IS the Lead (`selectedMemberIsLead`); otherwise empty.
   */
  leadResponsibilities: LeadResponsibilities;
  /** True when the selected member is the Lead (role==="lead" OR team owner). */
  selectedMemberIsLead: boolean;
  activity: TimelineItem[];
  decisionQueue: TimelineItem[];
  /**
   * The subset of the decision queue awaiting THIS team's Lead — proposals and
   * review/decision warnings on tasks not owned by the Lead, keyed to the team
   * `owner_agent_id`. Makes the anti-drift invariant (a worker cannot ratify
   * their own proposal as a global decision) visible.
   */
  leadDecisionQueue: TimelineItem[];
  sessionsByMember: ProviderSession[];
  childThreadsByMember: ProviderChildThread[];
  /** Per-agent activity stats keyed by member id (computeAgentStats over each
   * member's provider sessions). Powers the list sparkline/runs + detail perf. */
  statsByMember: Record<string, AgentStats>;
  /** Registered workflow catalog (from GET /v1/workflows, fetched in App). */
  workflowDefs: WorkflowDef[];
  /** Every workflow run, Running pinned first then terminal newest-first. */
  workflowRuns: WorkflowRun[];
  /** Steps grouped by run id (a run's `step_ids` order is applied by helpers). */
  workflowStepsByRun: Map<string, WorkflowStep[]>;
  /** The run addressed by `selection.workflowRunId`, if any. */
  selectedWorkflowRun?: WorkflowRun;
  /** The selected run's steps, ordered by `run.step_ids`. */
  selectedWorkflowSteps: WorkflowStep[];
  /** The goal↔run orchestration checkpoints (Stage 0), newest-first. */
  goalOrchestrationRuns: GoalOrchestrationRun[];
}

const laneOrder: TaskStatus[] = ["planned", "assigned", "running", "blocked", "review", "done", "archived"];

export function buildWorkbenchModel(
  snapshot: DashboardSnapshot,
  selection: SelectionState,
  workflowDefs: WorkflowDef[] = [],
): WorkbenchModel {
  const goals = snapshot.goals ?? [];
  const teams = snapshot.teams ?? [];
  const members = snapshot.members ?? [];
  const tasks = snapshot.tasks ?? [];
  const messages = snapshot.messages ?? [];
  const evidence = snapshot.evidence ?? [];
  const proposals = snapshot.proposals ?? [];
  const decisions = snapshot.decisions ?? [];
  const reviews = snapshot.reviews ?? [];
  const warnings = deriveWarnings(snapshot);

  const selectedGoal = goals.find((goal) => goal.id === selection.goalId) ?? goals.find((goal) => goal.status === "active") ?? goals[0];
  const selectedTeam =
    teams.find((team) => team.id === selection.teamId) ??
    teams.find((team) => (team.member_ids ?? []).some((id) => members.some((member) => member.id === id))) ??
    teams[0];
  const selectedMember =
    members.find((member) => member.id === selection.memberId) ??
    members.find((member) => member.id === selectedTeam?.owner_agent_id) ??
    members[0];
  const goalTasks = selectedGoal ? tasks.filter((task) => task.goal_id === selectedGoal.id) : tasks;
  const selectedTask =
    tasks.find((task) => task.id === selection.taskId) ??
    goalTasks.find((task) => task.id === selectedMember?.current_task_id) ??
    goalTasks[0] ??
    tasks[0];

  const selectedMemberMessages = selectedMember
    ? messages.filter((message) => message.from_agent_id === selectedMember.id || message.to_agent_id === selectedMember.id)
    : [];
  const inboxMessages = selectedMember
    ? messages.filter((message) => message.to_agent_id === selectedMember.id)
    : [];
  const outboxMessages = selectedMember
    ? messages.filter((message) => message.from_agent_id === selectedMember.id)
    : [];
  const reviewsByMember = selectedMember
    ? reviews.filter((review) => review.reviewer_agent_id === selectedMember.id)
    : [];
  // The team Lead is authoritative as the team's owner_agent_id. We resolve the
  // owning team from the selected member's team_ids first, falling back to the
  // selected team. This stays frontend-derived (no schema field) per the design.
  const leadMemberId =
    (selectedMember
      ? teams.find((team) => (selectedMember.team_ids ?? []).includes(team.id))?.owner_agent_id
      : undefined) ?? selectedTeam?.owner_agent_id;

  // The selected member is the Lead when it owns the resolved team OR carries
  // the doctrinal role==="lead". Both are frontend-derived, no schema field.
  const selectedMemberIsLead = Boolean(
    selectedMember &&
      (selectedMember.id === leadMemberId || selectedMember.role?.toLowerCase() === "lead"),
  );

  const reviewsForTask = selectedTask
    ? reviews.filter((review) => review.task_id === selectedTask.id)
    : [];
  const goalTaskIds = new Set(goalTasks.map((task) => task.id));
  const reviewsForGoal = selectedGoal
    ? reviews.filter(
        (review) =>
          review.goal_id === selectedGoal.id ||
          (review.task_id != null && goalTaskIds.has(review.task_id)),
      )
    : [];

  const gaps = sortGaps(snapshot.gaps ?? []);
  const gapsByGoal = groupBy(gaps, (gap) => gap.goal_id ?? "");
  const gapsBySeverity = groupBy(gaps, (gap) => gap.severity ?? "other");

  // Dual-read: union the top-level snapshot arrays with the graduated objects
  // surfaced inside goal_learning_status (deduped by id), so the GoalDocument
  // renders whichever path the backend wrote.
  const learningStatus = snapshot.goal_learning_status ?? [];
  const goalDesigns = dedupeById(
    [
      ...(snapshot.goal_designs ?? []),
      ...learningStatus.flatMap((status) => status.goal_design_objects ?? []),
    ],
    (design) => design.id,
  );
  const goalEvaluations = dedupeById(
    [
      ...(snapshot.goal_evaluations ?? []),
      ...learningStatus.flatMap((status) => status.goal_evaluation_objects ?? []),
    ],
    (evaluation) => evaluation.id,
  );
  const goalCases = dedupeById(
    [
      ...(snapshot.goal_cases ?? []),
      ...learningStatus.flatMap((status) => status.goal_case_objects ?? []),
    ],
    (goalCase) => goalCase.case_id,
  );
  const visions = snapshot.visions ?? [];
  const goalDesignByGoal = groupBy(goalDesigns, (design) => design.goal_id ?? "");
  const goalEvaluationByGoal = groupBy(goalEvaluations, (evaluation) => evaluation.goal_id ?? "");
  const goalDesignsForGoal = selectedGoal ? (goalDesignByGoal.get(selectedGoal.id) ?? []) : [];
  const goalEvaluationsForGoal = selectedGoal ? (goalEvaluationByGoal.get(selectedGoal.id) ?? []) : [];
  const goalCasesForGoal = selectedGoal
    ? goalCases.filter((goalCase) => goalCase.source_goal_id === selectedGoal.id)
    : [];
  const visionForGoal =
    selectedGoal?.vision_id != null
      ? visions.find((vision) => vision.id === selectedGoal.vision_id)
      : undefined;

  const leadResponsibilities = buildLeadResponsibilities(
    selectedMemberIsLead ? selectedMember : undefined,
    { goals, teams, tasks, goalDesigns, goalEvaluations, outboxMessages, decisions },
  );

  const leadDecisionQueue = buildLeadDecisionQueue(snapshot, warnings, leadMemberId);

  // Per-agent stats: group sessions by member once, then computeAgentStats each.
  const nowMs = Date.now();
  const sessionsByMemberId = new Map<string, ProviderSession[]>();
  for (const session of snapshot.provider_sessions ?? []) {
    const owner = session.agent_member_id;
    if (!owner) continue;
    const list = sessionsByMemberId.get(owner) ?? [];
    list.push(session);
    sessionsByMemberId.set(owner, list);
  }
  const statsByMember: Record<string, AgentStats> = {};
  for (const member of members) {
    statsByMember[member.id] = computeAgentStats(sessionsByMemberId.get(member.id) ?? [], nowMs);
  }
  // Workflows: Running runs pinned on top (they pulse), then terminal runs
  // newest-first by created_at (§2 ordering).
  const allWorkflowRuns = snapshot.workflow_runs ?? [];
  const allWorkflowSteps = snapshot.workflow_steps ?? [];
  const workflowRuns = sortWorkflowRuns(allWorkflowRuns);
  const workflowStepsByRun = groupBy(allWorkflowSteps, (step) => step.run_id);
  const selectedWorkflowRun = selection.workflowRunId
    ? allWorkflowRuns.find((run) => run.id === selection.workflowRunId)
    : undefined;
  const selectedWorkflowSteps = selectedWorkflowRun
    ? orderStepsByRun(selectedWorkflowRun, workflowStepsByRun.get(selectedWorkflowRun.id) ?? [])
    : [];
  // Stage 0: goal↔run orchestration checkpoints, newest-first by updated_at.
  const goalOrchestrationRuns = [...(snapshot.goal_orchestration_runs ?? [])].sort(
    (a, b) => (b.updated_at ?? "").localeCompare(a.updated_at ?? ""),
  );

  return {
    snapshot,
    generatedAt: snapshot.generated_at,
    selectedGoal,
    selectedTeam,
    selectedMember,
    selectedTask,
    goals,
    activeGoals: goals.filter((goal) => isGoalStatus(goal, "active")),
    completeGoals: goals.filter((goal) => isGoalStatus(goal, "complete", "done")),
    blockedGoals: goals.filter((goal) => isGoalStatus(goal, "blocked")),
    proposedGoals: goals.filter((goal) => isGoalStatus(goal, "proposed", "planned")),
    members,
    roleGroups: groupMembersByRole(members),
    tasks,
    goalTasks,
    lanes: buildLanes(goalTasks, snapshot.kanban),
    taskGraph: taskGraph(tasks),
    messages,
    evidence,
    proposals,
    decisions,
    reviews,
    reviewsForTask,
    reviewsForGoal,
    gaps,
    gapsByGoal,
    gapsBySeverity,
    goalDesignByGoal,
    goalEvaluationByGoal,
    goalCases,
    visions,
    goalDesignsForGoal,
    goalEvaluationsForGoal,
    goalCasesForGoal,
    visionForGoal,
    warnings,
    selectedMemberMessages,
    inboxMessages,
    outboxMessages,
    reviewsByMember,
    leadMemberId,
    leadResponsibilities,
    selectedMemberIsLead,
    selectedMemberTimeline: selectedMember
      ? buildMemberTimeline(snapshot, selectedMember, warnings)
      : [],
    activity: buildActivity(snapshot, warnings),
    decisionQueue: buildDecisionQueue(snapshot, warnings),
    leadDecisionQueue,
    sessionsByMember: selectedMember
      ? (snapshot.provider_sessions ?? []).filter((session) => session.agent_member_id === selectedMember.id)
      : [],
    childThreadsByMember: selectedMember
      ? (snapshot.provider_child_threads ?? []).filter((thread) => thread.agent_member_id === selectedMember.id)
      : [],
    statsByMember,
    workflowDefs,
    workflowRuns,
    workflowStepsByRun,
    goalOrchestrationRuns,
    selectedWorkflowRun,
    selectedWorkflowSteps,
  };
}

/**
 * Order a run's steps by its `step_ids` (the authoritative start order). Steps
 * whose id is absent from `step_ids` (e.g. a freshly-streamed row not yet
 * reflected in the run) are appended in their original order.
 */
export function orderStepsByRun(run: WorkflowRun, steps: WorkflowStep[]): WorkflowStep[] {
  const byId = new Map(steps.map((step) => [step.id, step]));
  const ordered: WorkflowStep[] = [];
  for (const id of run.step_ids ?? []) {
    const step = byId.get(id);
    if (step) {
      ordered.push(step);
      byId.delete(id);
    }
  }
  for (const step of steps) {
    if (byId.has(step.id)) ordered.push(step);
  }
  return ordered;
}

/**
 * Run ordering for the index list: Running runs first (they pulse and are the
 * live focus), then terminal/other runs newest-first by `created_at`.
 */
function sortWorkflowRuns(runs: WorkflowRun[]): WorkflowRun[] {
  return runs.slice().sort((a, b) => {
    const aRunning = (a.status ?? "").toLowerCase() === "running";
    const bRunning = (b.status ?? "").toLowerCase() === "running";
    if (aRunning !== bRunning) return aRunning ? -1 : 1;
    // created_at is "unix-ms:<n>", which Date.parse cannot read (→ NaN → every
    // run tied at 0, leaving append order). parseTs strips the prefix so the
    // list actually sorts most-recent-launch first.
    const aMs = parseTs(a.created_at) || 0;
    const bMs = parseTs(b.created_at) || 0;
    return bMs - aMs;
  });
}

export function memberName(members: AgentMember[], id?: string | null): string {
  if (!id) return "Unassigned";
  return members.find((member) => member.id === id)?.name ?? id;
}

export function taskTitle(tasks: Task[], id?: string | null): string {
  if (!id) return "No task";
  return tasks.find((task) => task.id === id)?.title ?? id;
}

export function objectShortId(id?: string | null): string {
  if (!id) return "none";
  return id.length > 22 ? `${id.slice(0, 18)}...` : id;
}

export function countBySeverity(warnings: WorkflowWarning[]) {
  return {
    high: warnings.filter((warning) => warning.severity === "high").length,
    medium: warnings.filter((warning) => warning.severity === "medium").length,
    low: warnings.filter((warning) => warning.severity === "low").length,
  };
}

/** Resolved gap statuses (fixed/wontfix) sink below unresolved ones in the ledger. */
const resolvedGapStatuses = new Set(["fixed", "wontfix"]);

export function gapIsResolved(gap: Gap): boolean {
  return resolvedGapStatuses.has((gap.status ?? "open").toLowerCase());
}

/** Severity sort rank: p0 first, then p1, p2, then unknown. */
export function gapSeverityRank(severity?: string | null): number {
  switch ((severity ?? "").toLowerCase()) {
    case "p0":
      return 0;
    case "p1":
      return 1;
    case "p2":
      return 2;
    default:
      return 3;
  }
}

/** Sort gaps by severity (p0→p2), unresolved-first, then most-recently-updated. */
function sortGaps(gaps: Gap[]): Gap[] {
  return [...gaps].sort((a, b) => {
    const severityDelta = gapSeverityRank(a.severity) - gapSeverityRank(b.severity);
    if (severityDelta !== 0) return severityDelta;
    const resolvedDelta = Number(gapIsResolved(a)) - Number(gapIsResolved(b));
    if (resolvedDelta !== 0) return resolvedDelta;
    return (b.updated_at ?? "").localeCompare(a.updated_at ?? "");
  });
}

/** Keep the first occurrence per id (top-level snapshot wins over learning-status). */
function dedupeById<T>(items: T[], key: (item: T) => string): T[] {
  const seen = new Set<string>();
  const result: T[] = [];
  for (const item of items) {
    const k = key(item);
    if (seen.has(k)) continue;
    seen.add(k);
    result.push(item);
  }
  return result;
}

function groupBy<T>(items: T[], key: (item: T) => string): Map<string, T[]> {
  const map = new Map<string, T[]>();
  for (const item of items) {
    const k = key(item);
    map.set(k, [...(map.get(k) ?? []), item]);
  }
  return map;
}

function isGoalStatus(goal: Goal, ...statuses: string[]): boolean {
  return statuses.includes((goal.status ?? "active").toLowerCase());
}

/**
 * Product column for a Goal: the legacy `complete` status folds into `done`
 * (ADR 0019). `archived` is returned as-is so callers can hide it.
 */
export function displayGoalStatus(goal: Goal): string {
  const status = (goal.status ?? "active").toLowerCase();
  return status === "complete" ? "done" : status;
}

export interface TaskGraphEdge {
  /** The dependency (prerequisite) task id. */
  from: string;
  /** The dependent task id (waits for `from`). */
  to: string;
}

export interface TaskGraph {
  nodes: Task[];
  /** dependency -> dependent edges, derived from depends_on_task_ids. */
  edges: TaskGraphEdge[];
  /** Tasks whose every dependency is `done` AND that are still planned/assigned. */
  ready: Set<string>;
  /** taskId -> unfinished dependency ids (the reason it is waiting). */
  waiting: Map<string, string[]>;
}

/** Self-statuses that can become "ready" once dependencies clear. */
const READY_SELF_STATUSES: TaskStatus[] = ["planned", "assigned"];

/**
 * Derive the task graph from `depends_on_task_ids` alone (ADR 0019 / 0009): one
 * stored edge, everything else derived. `ready` = all dependencies `done` and
 * self planned/assigned; `waiting` = at least one unfinished dependency. Note
 * `waiting` (derived) is distinct from the stored `status==="blocked"`: a
 * `planned` task with an unfinished dependency is waiting, not blocked. An
 * unknown dependency id counts as unfinished (we cannot prove it is done).
 */
export function taskGraph(tasks: Task[]): TaskGraph {
  const byId = new Map(tasks.map((task) => [task.id, task]));
  const edges: TaskGraphEdge[] = [];
  const ready = new Set<string>();
  const waiting = new Map<string, string[]>();
  for (const task of tasks) {
    const unfinished: string[] = [];
    for (const depId of task.depends_on_task_ids ?? []) {
      edges.push({ from: depId, to: task.id });
      const dep = byId.get(depId);
      if (!dep || dep.status !== "done") unfinished.push(depId);
    }
    if (unfinished.length > 0) {
      waiting.set(task.id, unfinished);
    } else if (READY_SELF_STATUSES.includes(task.status)) {
      ready.add(task.id);
    }
  }
  return { nodes: tasks, edges, ready, waiting };
}

/** Tasks that depend on `taskId` (reverse edges = what this task blocks). */
export function tasksBlockedBy(taskId: string, tasks: Task[]): Task[] {
  return tasks.filter((task) => (task.depends_on_task_ids ?? []).includes(taskId));
}

/**
 * One greedy group inside a DAG layer: tasks with pairwise-disjoint
 * `owned_paths`. A group with >1 task is a `parallel([...])` (they ran
 * concurrently); a singleton ran on its own. Mirrors the compiler grouping.
 */
export interface PhaseDagGroup {
  /** True when the group holds more than one task (rendered as parallel). */
  parallel: boolean;
  tasks: Task[];
}

/** One layer of a phase's task DAG: groups that ran before the next layer. */
export interface PhaseDagLayer {
  layer: number;
  groups: PhaseDagGroup[];
}

/**
 * Layer a phase's LIVE tasks into the same shape the Starlark compiler emits, so
 * the dashboard renders the execution plan visually:
 *  - longest dependency-path layering over IN-PHASE `depends_on_task_ids` (a dep
 *    pointing outside the phase is ignored, mirroring the compiler);
 *  - within a layer, a greedy disjoint-`owned_paths` partition — a >1 group is a
 *    parallel row, a singleton is serial; groups run after one another.
 * Superseded tasks are excluded (they render separately, struck through).
 * Tasks are id-sorted so the layout is deterministic, matching the compiler.
 */
export function phaseTaskDag(phaseId: string, tasks: Task[]): PhaseDagLayer[] {
  const live = tasks
    .filter((task) => task.phase_id === phaseId && task.status !== "superseded")
    .slice()
    .sort((a, b) => a.id.localeCompare(b.id));
  if (live.length === 0) return [];

  const inPhase = new Set(live.map((task) => task.id));
  const byId = new Map(live.map((task) => [task.id, task]));
  const layerOf = new Map<string, number>(live.map((task) => [task.id, 0]));

  // Iterative longest-path relaxation; cap passes so a cycle cannot loop forever
  // (it just settles at the cap, which is fine for a read-only view).
  const cap = live.length + 1;
  for (let pass = 0; pass < cap; pass += 1) {
    let changed = false;
    for (const task of live) {
      let want = 0;
      for (const dep of task.depends_on_task_ids ?? []) {
        if (inPhase.has(dep)) want = Math.max(want, (layerOf.get(dep) ?? 0) + 1);
      }
      if (want !== layerOf.get(task.id)) {
        layerOf.set(task.id, want);
        changed = true;
      }
    }
    if (!changed) break;
  }

  const maxLayer = Math.max(0, ...live.map((task) => layerOf.get(task.id) ?? 0));
  const layers: PhaseDagLayer[] = [];
  for (let l = 0; l <= maxLayer; l += 1) {
    const layerTasks = live.filter((task) => (layerOf.get(task.id) ?? 0) === l);
    const groups: { paths: Set<string>; tasks: Task[] }[] = [];
    for (const task of layerTasks) {
      const paths = new Set(byId.get(task.id)?.owned_paths ?? []);
      let placed = false;
      for (const group of groups) {
        const disjoint = [...paths].every((p) => !group.paths.has(p));
        if (disjoint) {
          for (const p of paths) group.paths.add(p);
          group.tasks.push(task);
          placed = true;
          break;
        }
      }
      if (!placed) groups.push({ paths, tasks: [task] });
    }
    if (groups.length > 0) {
      layers.push({
        layer: l,
        groups: groups.map((group) => ({ parallel: group.tasks.length > 1, tasks: group.tasks })),
      });
    }
  }
  return layers;
}

/**
 * Phase-scoped kanban lanes (goal-task-board-model): the Kanban counterpart to
 * {@link phaseTaskDag}. Filters to this phase's LIVE tasks (`phase_id===phaseId`
 * && status!=="superseded") and buckets them by their own `status` into the same
 * `laneOrder`/`labelStatus` lanes the global board uses — mirroring
 * `buildLanesLocal`, but scoped to one phase. Returns every lane (empty lanes
 * included) so the board layout is stable; a nonexistent phase yields all-empty
 * lanes. Reuses the {@link Lane} interface.
 */
export function phaseKanban(phaseId: string, tasks: Task[]): Lane[] {
  const live = tasks.filter(
    (task) => task.phase_id === phaseId && task.status !== "superseded",
  );
  return laneOrder.map((status) => ({
    id: status,
    title: labelStatus(status),
    tasks: live.filter((task) => task.status === status),
  }));
}

/**
 * The "(no phase)" set for a goal (goal-task-board-model): tasks that belong to
 * the goal (`goal_id===goalId`) but carry no `phase_id` — the goal-scoped tasks
 * a phase-driven goal would otherwise hide. Superseded tasks are excluded so the
 * section mirrors the live phase views. Keeps phaseless-but-goaled work visible.
 */
export function phaselessGoalTasks(goalId: string, tasks: Task[]): Task[] {
  return tasks.filter(
    (task) =>
      task.goal_id === goalId &&
      (task.phase_id == null || task.phase_id === "") &&
      task.status !== "superseded",
  );
}

/**
 * Effective git context for a Task: prefer `git_metadata`, fall back to the flat
 * fields retained for back-compat (ADR 0019).
 */
export function taskGitMetadata(task: Task): GitMetadata {
  const meta = task.git_metadata ?? {};
  return {
    repo: meta.repo ?? null,
    worktree_path: meta.worktree_path ?? task.workspace_ref ?? null,
    branch: meta.branch ?? task.branch_ref ?? null,
    base_branch: meta.base_branch ?? null,
    pr_ref: meta.pr_ref ?? task.pr_ref ?? null,
    commit: meta.commit ?? null,
    owned_paths: meta.owned_paths ?? task.owned_paths ?? [],
  };
}

/**
 * Lead-band ordering. The Lead group renders first so the team's authority is
 * visible at the top of the rail/picker, then the standard collaboration roles
 * (critic → worker → observer), with anything else trailing. This is a stable
 * sort: members keep their snapshot order within a role, and roles not in the
 * canonical list keep their first-seen order after the known ones.
 */
const roleSortRank: Record<string, number> = {
  lead: 0,
  critic: 1,
  worker: 2,
  observer: 3,
};

export function roleGroupRank(role: string): number {
  const rank = roleSortRank[role.toLowerCase()];
  return rank ?? 4;
}

function groupMembersByRole(members: AgentMember[]): RoleGroup[] {
  const map = new Map<string, AgentMember[]>();
  for (const member of members) {
    const role = member.role || "Member";
    map.set(role, [...(map.get(role) ?? []), member]);
  }
  const groups = [...map.entries()].map(([role, groupMembers]) => ({ role, members: groupMembers }));
  // Stable sort: known roles by canonical rank (lead first), unknown roles keep
  // their first-seen order after the known ones (rank ties preserve index).
  return groups
    .map((group, index) => ({ group, index }))
    .sort((a, b) => {
      const rankDelta = roleGroupRank(a.group.role) - roleGroupRank(b.group.role);
      return rankDelta !== 0 ? rankDelta : a.index - b.index;
    })
    .map((entry) => entry.group);
}

/**
 * Lanes for the kanban. The backend emits a `kanban` map (status → task-id[])
 * which is the owner-decided source of truth for lane membership and ordering;
 * we resolve those ids against the goal-scoped task set. When the map is
 * empty/absent (e.g. the offline fixture, or a backend that does not emit it)
 * we fall back to the local build that buckets tasks by their own `status`.
 */
function buildLanes(tasks: Task[], kanban?: Record<TaskStatus, string[]>): Lane[] {
  if (kanban && Object.values(kanban).some((ids) => ids.length > 0)) {
    return buildLanesFromKanban(tasks, kanban);
  }
  return buildLanesLocal(tasks);
}

/** Backend-emitted kanban as source of truth: order/membership comes from the map. */
function buildLanesFromKanban(tasks: Task[], kanban: Record<TaskStatus, string[]>): Lane[] {
  const byId = new Map(tasks.map((task) => [task.id, task]));
  return laneOrder.map((status) => ({
    id: status,
    title: labelStatus(status),
    tasks: (kanban[status] ?? [])
      .map((id) => byId.get(id))
      .filter((task): task is Task => task != null),
  }));
}

/** Local fallback: bucket the goal-scoped tasks by their own status. */
function buildLanesLocal(tasks: Task[]): Lane[] {
  return laneOrder.map((status) => ({
    id: status,
    title: labelStatus(status),
    tasks: tasks.filter((task) => task.status === status),
  }));
}

/**
 * The member's single merged chronological stream. Per
 * docs/dashboard/pages/agent-member-workbench.md it unifies task assignment,
 * reports, sessions, events, evidence, delivery state, proposals, and the
 * reviews this member authored, so a reviewer can trace assignment BEFORE
 * report/evidence in one place.
 *
 * Sort is ascending-by-time then displayed in that order, with a per-kind tie
 * rank so a task assignment provably precedes its report/evidence when they
 * share a timestamp. Warnings carry a synthetic timestamp (NOW) so urgent items
 * surface at the head instead of sinking to the bottom. No hard cap — the
 * surface scrolls.
 */
function buildMemberTimeline(snapshot: DashboardSnapshot, member: AgentMember, warnings: WorkflowWarning[]): TimelineItem[] {
  const messages: TimelineItem[] = (snapshot.messages ?? [])
    .filter((message) => message.from_agent_id === member.id || message.to_agent_id === member.id)
    .map((message) => {
      const direction = message.to_agent_id === member.id ? "in" : "out";
      const title =
        message.kind === "task"
          ? "Task assignment"
          : message.kind === "report"
            ? "Member report"
            : direction === "in"
              ? "Inbox message"
              : "Outbox message";
      return {
        id: message.id,
        kind: "message" as const,
        title,
        meta: `${direction === "in" ? "inbox" : "outbox"} · ${message.delivery_status} · ${message.created_at ? formatTime(message.created_at) : "no time"}`,
        body: message.content,
        objectRef: message.task_id ?? undefined,
        createdAt: message.created_at ?? undefined,
        direction,
        deliveryStatus: message.delivery_status,
        counterpartyId: (direction === "in" ? message.from_agent_id : message.to_agent_id) ?? undefined,
        fromAgentId: message.from_agent_id,
        senderKind: message.sender_kind,
        providerSessionId: message.delivery?.provider_session_id ?? undefined,
      };
    });

  // Delivery state of this member's messages (terminal_source / errors) is part
  // of the timeline per the spec; surface it as its own row when present.
  const deliveries: TimelineItem[] = (snapshot.messages ?? [])
    .filter((message) => message.from_agent_id === member.id || message.to_agent_id === member.id)
    .filter((message) => message.delivery != null && (message.delivery.delivered_at || message.delivery.last_error))
    .map((message) => {
      const delivery = message.delivery!;
      const failed = Boolean(delivery.last_error);
      return {
        id: `delivery-${message.id}`,
        kind: "event" as const,
        title: failed ? "Delivery failed" : "Delivery completed",
        meta: delivery.terminal_source
          ? `${delivery.terminal_source}${delivery.delivered_at ? ` · ${formatTime(delivery.delivered_at)}` : ""}`
          : (delivery.delivered_at ? formatTime(delivery.delivered_at) : "delivery"),
        body: delivery.last_error ?? delivery.provider_turn_id ?? message.content,
        severity: failed ? ("high" as const) : undefined,
        objectRef: message.task_id ?? undefined,
        createdAt: delivery.delivered_at ?? message.created_at ?? undefined,
      };
    });

  const sessions: TimelineItem[] = (snapshot.provider_sessions ?? [])
    .filter((session) => session.agent_member_id === member.id)
    .map((session) => ({
      id: session.id,
      kind: "session" as const,
      title: `Provider session ${session.status ?? "unknown"}`,
      meta: session.provider ?? "provider",
      body: session.prompt_summary ?? session.command,
      objectRef: session.task_id ?? undefined,
      createdAt: session.started_at ?? undefined,
      provider: session.provider ?? undefined,
      sessionStatus: session.status ?? undefined,
      startedAt: session.started_at ?? undefined,
      endedAt: session.ended_at ?? undefined,
      threadId: session.provider_thread_id ?? undefined,
      turnId: session.provider_turn_id ?? undefined,
    }));

  const events: TimelineItem[] = (snapshot.events ?? [])
    .filter((event) => event.agent_member_id === member.id)
    .map((event) => {
      const counted = extractEventCount(event.event_type, event.summary);
      return {
        id: event.id,
        kind: "event" as const,
        title: event.event_type ?? "event",
        meta: event.created_at ? formatTime(event.created_at) : "event",
        body: event.summary,
        objectRef: event.task_id ?? undefined,
        createdAt: event.created_at ?? undefined,
        eventType: event.event_type ?? undefined,
        count: counted?.count,
        countNoun: counted?.noun,
      };
    });

  // Evidence linked to this member's tasks. Member rows carry no member id, so
  // we scope by the tasks the member touched (current task + tasks referenced
  // by their messages).
  const memberTaskIds = new Set<string>(
    [
      member.current_task_id,
      ...(snapshot.messages ?? [])
        .filter((message) => message.from_agent_id === member.id || message.to_agent_id === member.id)
        .map((message) => message.task_id),
    ].filter((id): id is string => Boolean(id)),
  );
  const evidence: TimelineItem[] = (snapshot.evidence ?? [])
    .filter((item) => item.task_id != null && memberTaskIds.has(item.task_id))
    .map((item) => ({
      id: item.id,
      kind: "evidence" as const,
      title: `Evidence: ${item.source_type ?? item.evidence_kind ?? "artifact"}`,
      meta: item.source_ref ?? "evidence",
      body: item.summary,
      objectRef: item.task_id ?? undefined,
    }));

  const proposals: TimelineItem[] = (snapshot.proposals ?? [])
    .filter((proposal) => proposal.agent_member_id === member.id)
    .map((proposal) => ({
      id: proposal.id,
      kind: "proposal" as const,
      title: proposal.title ?? "Proposal",
      meta: proposal.status ?? "draft",
      body: proposal.summary,
      objectRef: proposal.task_id,
      count: proposal.changed_paths?.length || undefined,
      countNoun: proposal.changed_paths?.length ? "file" : undefined,
    }));

  const reviews: TimelineItem[] = (snapshot.reviews ?? [])
    .filter((review) => review.reviewer_agent_id === member.id)
    .map((review) => ({
      id: review.id,
      kind: "review" as const,
      title: `Review: ${review.verdict ?? review.review_kind ?? "review"}`,
      meta: `${review.review_kind ?? "review"}${review.created_at ? ` · ${formatTime(review.created_at)}` : ""}`,
      body: review.summary,
      objectRef: review.task_id ?? undefined,
      createdAt: review.created_at ?? undefined,
      verdict: review.verdict ?? undefined,
    }));

  // Warnings get a synthetic NOW timestamp so they no longer sink below dated
  // rows; severity still drives their tone.
  const syntheticNow = new Date().toISOString();
  const memberWarnings: TimelineItem[] = warnings
    .filter((warning) => warning.memberId === member.id)
    .map((warning) => ({
      id: warning.id,
      kind: "warning" as const,
      title: warning.kind,
      meta: warning.severity,
      body: warning.summary,
      severity: warning.severity,
      objectRef: warning.taskId,
      createdAt: syntheticNow,
    }));

  return sortTimelineChronological([
    ...messages,
    ...deliveries,
    ...sessions,
    ...events,
    ...evidence,
    ...proposals,
    ...reviews,
    ...memberWarnings,
  ]);
}

/**
 * Per-kind tie-break rank used when two items share (or lack) a timestamp, so
 * an assignment always renders before the report/evidence it produced.
 */
const timelineKindRank: Record<TimelineItem["kind"], number> = {
  message: 0,
  session: 1,
  event: 2,
  proposal: 3,
  evidence: 4,
  review: 5,
  decision: 6,
  warning: 7,
};

/**
 * Sort ascending by time (oldest first) so the displayed order proves causality
 * (assignment → report → evidence). Items without a timestamp keep a stable
 * tail ordered by kind rank. Equal timestamps tie-break by kind rank.
 */
function sortTimelineChronological(items: TimelineItem[]): TimelineItem[] {
  return [...items].sort((a, b) => {
    const ta = a.createdAt ? Date.parse(a.createdAt) : NaN;
    const tb = b.createdAt ? Date.parse(b.createdAt) : NaN;
    const aHas = !Number.isNaN(ta);
    const bHas = !Number.isNaN(tb);
    if (aHas && bHas && ta !== tb) return ta - tb;
    if (aHas && !bHas) return -1;
    if (!aHas && bHas) return 1;
    return timelineKindRank[a.kind] - timelineKindRank[b.kind];
  });
}

/**
 * Best-effort "N <noun>" extraction from an AgentEvent's type/summary, so an
 * event card can show "Ran 3 commands" / "Edited 2 files" when the count is
 * already present in the text. Provider-neutral: it reads whatever the neutral
 * AgentEvent already carries and never fabricates a count.
 */
function extractEventCount(
  eventType?: string,
  summary?: string,
): { count: number; noun: string } | undefined {
  const haystack = `${eventType ?? ""} ${summary ?? ""}`;
  const match = haystack.match(
    /(\d+)\s+(commands?|files?|edits?|tools?|calls?|tests?|diffs?|changes?)/i,
  );
  if (!match) return undefined;
  const count = Number(match[1]);
  if (!Number.isFinite(count)) return undefined;
  return { count, noun: match[2].toLowerCase().replace(/s$/, "") };
}

/**
 * Regroup the (already merged + sorted) member timeline into provider-session
 * blocks for the chat-app view. This is a PURE re-projection of the existing
 * timeline — no new data source, no re-sort beyond preserving the input order.
 *
 * A row nests under a session when (a) it IS that session's own row, or (b) its
 * timestamp falls inside the session window [started_at, ended_at] (open-ended
 * sessions extend to the next session's start, else to +∞). Rows that match no
 * window collect into a single default group at the head, time-ordered like the
 * input. Assignment-before-report is preserved because we keep input order
 * within every group (the input is already chronological with the kind
 * tie-break applied in sortTimelineChronological).
 */
export function groupMemberTimelineBySession(
  timeline: TimelineItem[],
  sessions: ProviderSession[],
): MemberSessionGroup[] {
  // Order sessions by start time so window boundaries are well-defined.
  const ordered = [...sessions].sort((a, b) =>
    (a.started_at ?? "").localeCompare(b.started_at ?? ""),
  );
  const windows = ordered.map((session, index) => {
    const start = session.started_at ? Date.parse(session.started_at) : NaN;
    const explicitEnd = session.ended_at ? Date.parse(session.ended_at) : NaN;
    const nextStart = ordered[index + 1]?.started_at
      ? Date.parse(ordered[index + 1].started_at!)
      : NaN;
    const end = !Number.isNaN(explicitEnd)
      ? explicitEnd
      : !Number.isNaN(nextStart)
        ? nextStart
        : Number.POSITIVE_INFINITY;
    return { session, start, end };
  });

  const groups = new Map<string, TimelineItem[]>();
  for (const window of windows) groups.set(window.session.id, []);
  const defaultItems: TimelineItem[] = [];

  for (const item of timeline) {
    // A session's own row always nests under itself.
    if (item.kind === "session") {
      const own = groups.get(item.id);
      if (own) {
        own.push(item);
        continue;
      }
    }
    const ts = item.createdAt ? Date.parse(item.createdAt) : NaN;
    let placed = false;
    if (!Number.isNaN(ts)) {
      // Last (most recent) matching window wins so a row co-owned by adjacent
      // windows lands in the session it actually started under.
      for (let i = windows.length - 1; i >= 0; i -= 1) {
        const window = windows[i];
        if (Number.isNaN(window.start)) continue;
        if (ts >= window.start && ts <= window.end) {
          groups.get(window.session.id)!.push(item);
          placed = true;
          break;
        }
      }
    }
    if (!placed) defaultItems.push(item);
  }

  const result: MemberSessionGroup[] = [];
  if (defaultItems.length) {
    result.push({ id: "__standalone__", items: defaultItems });
  }
  for (const window of windows) {
    result.push({
      id: window.session.id,
      session: window.session,
      items: groups.get(window.session.id) ?? [],
    });
  }
  return result;
}

/** Parse a harness timestamp ("unix-ms:<ms>" or ISO) to epoch ms, or NaN.
 * Shared so list/detail/stats/duration all agree on the format. */
export function parseTs(value?: string | null): number {
  if (!value) return NaN;
  return value.startsWith("unix-ms:") ? Number(value.slice("unix-ms:".length)) : Date.parse(value);
}

/**
 * Per-agent activity stats derived from that member's provider sessions — no
 * backend aggregate. Powers the Agents-list sparkline/run-count and the detail
 * Tasks-tab performance summary. O(n) over the member's own sessions.
 */
export function computeAgentStats(sessions: ProviderSession[], nowMs: number): AgentStats {
  const DAY = 86_400_000;
  const activity7d = [0, 0, 0, 0, 0, 0, 0];
  let runCount30d = 0;
  let succeeded = 0;
  let failed = 0;
  let durSum = 0;
  let durN = 0;
  let lastActiveMs: number | null = null;
  let runningCount = 0;
  let liveSessionId: string | null = null;
  let liveStart = -Infinity;
  for (const s of sessions) {
    const start = parseTs(s.started_at);
    if (!Number.isNaN(start)) {
      if (lastActiveMs === null || start > lastActiveMs) lastActiveMs = start;
      const ageDays = Math.floor((nowMs - start) / DAY);
      if (ageDays >= 0 && ageDays < 30) runCount30d += 1;
      if (ageDays >= 0 && ageDays < 7) {
        // bucket 6 = today, 0 = 6 days ago (oldest→newest)
        activity7d[6 - ageDays] += 1;
      }
    }
    const status = s.status ?? "";
    if (status === "succeeded") succeeded += 1;
    else if (status === "failed") failed += 1;
    if (status === "running" || status === "queued") {
      if (status === "running") runningCount += 1;
      if (!Number.isNaN(start) && start > liveStart) {
        liveStart = start;
        liveSessionId = s.id;
      }
    }
    const end = parseTs(s.ended_at);
    if (!Number.isNaN(start) && !Number.isNaN(end) && end >= start) {
      durSum += end - start;
      durN += 1;
    }
  }
  const terminal = succeeded + failed;
  return {
    runCount30d,
    runsTotal: sessions.length,
    succeeded,
    failed,
    successRate: terminal ? succeeded / terminal : null,
    avgDurationMs: durN ? Math.round(durSum / durN) : null,
    activity7d,
    lastActiveMs,
    runningCount,
    liveSessionId,
  };
}

/** Human duration between two timestamps ("unix-ms:<ms>" or ISO): "1m54s", "2h3m", "45s". */
export function formatDuration(
  start?: string | null,
  end?: string | null,
): string | undefined {
  if (!start) return undefined;
  const startMs = parseTs(start);
  if (Number.isNaN(startMs)) return undefined;
  const endMs = end ? parseTs(end) : Date.now();
  if (Number.isNaN(endMs)) return undefined;
  const totalS = Math.max(0, Math.round((endMs - startMs) / 1000));
  if (totalS < 60) return `${totalS}s`;
  const m = Math.floor(totalS / 60);
  const s = totalS % 60;
  if (m < 60) return s ? `${m}m${s}s` : `${m}m`;
  const h = Math.floor(m / 60);
  const remM = m % 60;
  return remM ? `${h}h${remM}m` : `${h}h`;
}

function buildActivity(snapshot: DashboardSnapshot, warnings: WorkflowWarning[]): TimelineItem[] {
  const messages = (snapshot.messages ?? []).map((message) => ({
    id: message.id,
    kind: "message" as const,
    title: message.kind === "task" ? "Task assigned" : message.kind === "report" ? "Report received" : "Message",
    meta: `${message.delivery_status} · ${message.created_at ? formatTime(message.created_at) : "no time"}`,
    body: message.content,
    objectRef: message.task_id ?? undefined,
    createdAt: message.created_at ?? undefined,
  }));

  const proposals = (snapshot.proposals ?? []).map((proposal) => ({
    id: proposal.id,
    kind: "proposal" as const,
    title: proposal.title ?? "Proposal",
    meta: proposal.status ?? "draft",
    body: proposal.summary,
    objectRef: proposal.task_id,
  }));

  const decisions = (snapshot.decisions ?? []).map((decision) => ({
    id: decision.id,
    kind: "decision" as const,
    title: `Decision: ${decision.decision ?? "pending"}`,
    meta: decision.task_id,
    body: decision.rationale,
    objectRef: decision.task_id,
  }));

  const warningRows = warnings.slice(0, 5).map((warning) => ({
    id: warning.id,
    kind: "warning" as const,
    title: warning.kind,
    meta: warning.severity,
    body: warning.summary,
    severity: warning.severity,
    objectRef: warning.taskId ?? warning.goalId ?? warning.memberId,
  }));

  return sortTimelineDesc([...warningRows, ...messages, ...proposals, ...decisions]).slice(0, 14);
}

/**
 * The Lead's doctrinal loop, assembled from existing canonical objects. Nothing
 * here is a new schema field — every lane is a filtered projection of what the
 * snapshot already carries, attributed to the Lead via Goal/Task ownership and
 * message authorship.
 */
function buildLeadResponsibilities(
  lead: AgentMember | undefined,
  data: {
    goals: Goal[];
    teams: AgentTeam[];
    tasks: Task[];
    goalDesigns: GoalDesign[];
    goalEvaluations: GoalEvaluation[];
    outboxMessages: Message[];
    decisions: Decision[];
  },
): LeadResponsibilities {
  if (!lead) {
    return { goalDesigns: [], assignments: [], decisions: [], evaluations: [], teamMemberIds: [] };
  }

  // Goals this Lead owns; their designs/evaluations are the Lead's design+eval lanes.
  const ownedGoalIds = new Set(
    data.goals.filter((goal) => goal.owner_agent_id === lead.id).map((goal) => goal.id),
  );
  // Tasks this Lead owns; decisions on them are the Lead's close-out decisions.
  const ownedTaskIds = new Set(
    data.tasks.filter((task) => task.owner_agent_id === lead.id).map((task) => task.id),
  );

  const goalDesigns = data.goalDesigns.filter(
    (design) => design.goal_id != null && ownedGoalIds.has(design.goal_id),
  );

  // Assignment truth is the outbox Message(kind="task"), not assignee_agent_id.
  const assignments = data.outboxMessages.filter((message) => message.kind === "task");

  const decisions = data.decisions.filter(
    (decision) =>
      ownedTaskIds.has(decision.task_id) ||
      (decision.goal_id != null && ownedGoalIds.has(decision.goal_id)),
  );

  // Evaluations attributed to this Lead either by evaluator id or owned goal.
  const evaluations = data.goalEvaluations.filter(
    (evaluation) =>
      evaluation.evaluator_agent_id === lead.id || ownedGoalIds.has(evaluation.goal_id),
  );

  // Team composition the Lead shapes: union of member_ids across teams it owns.
  const teamMemberIds = [
    ...new Set(
      data.teams
        .filter((team) => team.owner_agent_id === lead.id)
        .flatMap((team) => team.member_ids ?? []),
    ),
  ];

  return { goalDesigns, assignments, decisions, evaluations, teamMemberIds };
}

/**
 * The "Awaiting Lead decision" partition: proposals and review/decision
 * warnings on work the Lead does not own personally, surfaced as pending Lead
 * decisions. Keyed to `leadMemberId` (team `owner_agent_id`). If there is no
 * Lead, this is empty (no false attribution).
 */
function buildLeadDecisionQueue(
  snapshot: DashboardSnapshot,
  warnings: WorkflowWarning[],
  leadMemberId?: string,
): TimelineItem[] {
  if (!leadMemberId) return [];
  const tasksById = new Map((snapshot.tasks ?? []).map((task) => [task.id, task]));

  // A proposal awaits the Lead when its author is NOT the Lead (the anti-drift
  // invariant: a worker cannot ratify their own proposal as a global decision).
  const proposals = (snapshot.proposals ?? [])
    .filter((proposal) => proposal.agent_member_id !== leadMemberId)
    .map((proposal) => ({
      id: `lead-${proposal.id}`,
      kind: "proposal" as const,
      title: proposal.title ?? "Proposal",
      meta: proposal.status ?? "pending",
      body: proposal.summary,
      objectRef: proposal.task_id,
    }));

  // Review/decision warnings on tasks the Lead does not personally own surface
  // as pending Lead decisions.
  const warningItems = warnings
    .filter(
      (warning) => warning.kind.includes("decision") || warning.kind === "review_needs_decision",
    )
    .filter((warning) => {
      const task = warning.taskId ? tasksById.get(warning.taskId) : undefined;
      return !task || task.owner_agent_id !== leadMemberId;
    })
    .map((warning) => ({
      id: `lead-${warning.id}`,
      kind: "warning" as const,
      title: warning.kind,
      meta: warning.severity,
      body: warning.summary,
      severity: warning.severity,
      objectRef: warning.taskId,
    }));

  return sortTimelineDesc([...warningItems, ...proposals]);
}

function buildDecisionQueue(snapshot: DashboardSnapshot, warnings: WorkflowWarning[]): TimelineItem[] {
  const proposals = (snapshot.proposals ?? []).map((proposal) => ({
    id: proposal.id,
    kind: "proposal" as const,
    title: proposal.title ?? "Proposal",
    meta: proposal.status ?? "pending",
    body: proposal.summary,
    objectRef: proposal.task_id,
  }));
  const warningItems = warnings
    .filter((warning) => warning.kind.includes("decision") || warning.kind.includes("evidence") || warning.kind.includes("report"))
    .map((warning) => ({
      id: warning.id,
      kind: "warning" as const,
      title: warning.kind,
      meta: warning.severity,
      body: warning.summary,
      severity: warning.severity,
      objectRef: warning.taskId,
    }));

  return sortTimelineDesc([...warningItems, ...proposals]).slice(0, 10);
}

/** Newest-first sort by created_at; items without a timestamp keep their relative order at the end. */
function sortTimelineDesc(items: TimelineItem[]): TimelineItem[] {
  return [...items].sort((a, b) => {
    const ta = a.createdAt ? Date.parse(a.createdAt) : NaN;
    const tb = b.createdAt ? Date.parse(b.createdAt) : NaN;
    const aHas = !Number.isNaN(ta);
    const bHas = !Number.isNaN(tb);
    if (aHas && bHas) return tb - ta;
    if (aHas) return -1;
    if (bHas) return 1;
    return 0;
  });
}

function labelStatus(status: string): string {
  const labels: Record<string, string> = {
    planned: "Planned",
    assigned: "Assigned",
    running: "Running",
    blocked: "Blocked",
    review: "Review",
    done: "Done",
    archived: "Archived",
  };
  return labels[status] ?? status;
}

function formatTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}
