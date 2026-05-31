import type { SelectionState } from "../app/selection";
import type {
  AgentMember,
  AgentTeam,
  DashboardSnapshot,
  Decision,
  Evidence,
  Gap,
  Goal,
  GoalCase,
  GoalDesign,
  GoalEvaluation,
  Message,
  Proposal,
  ProviderChildThread,
  ProviderSession,
  Review,
  Task,
  TaskStatus,
  Vision,
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
  docs: RelatedDoc[];
  sessionsByMember: ProviderSession[];
  childThreadsByMember: ProviderChildThread[];
}

export interface RelatedDoc {
  path: string;
  title: string;
  reason: string;
  lifecycle: string;
}

const laneOrder: TaskStatus[] = ["planned", "assigned", "running", "blocked", "review", "done", "archived"];

const docCatalog: RelatedDoc[] = [
  {
    path: "docs/dashboard/frontend-design.md",
    title: "Frontend design index",
    reason: "Reading order, page map, and implementation readiness.",
    lifecycle: "volatile",
  },
  {
    path: "docs/dashboard/frontend-architecture.md",
    title: "Frontend architecture",
    reason: "Accepted stack, component boundary, and no-shadcn decision.",
    lifecycle: "volatile",
  },
  {
    path: "docs/dashboard/pages/team-workspace.md",
    title: "Team workspace page contract",
    reason: "Default collaboration-space layout and first viewport.",
    lifecycle: "volatile",
  },
  {
    path: "docs/dashboard/pages/agent-member-workbench.md",
    title: "AgentMember workbench page contract",
    reason: "Durable member, inbox/outbox, timeline, and runtime layout.",
    lifecycle: "volatile",
  },
  {
    path: "docs/dashboard/acceptance.md",
    title: "Screenshot acceptance",
    reason: "PM/User subagent browser acceptance and screenshot matrix.",
    lifecycle: "volatile",
  },
];

export function buildWorkbenchModel(snapshot: DashboardSnapshot, selection: SelectionState): WorkbenchModel {
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

  return {
    snapshot,
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
    lanes: buildLanes(goalTasks),
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
    docs: docCatalog,
    sessionsByMember: selectedMember
      ? (snapshot.provider_sessions ?? []).filter((session) => session.agent_member_id === selectedMember.id)
      : [],
    childThreadsByMember: selectedMember
      ? (snapshot.provider_child_threads ?? []).filter((thread) => thread.agent_member_id === selectedMember.id)
      : [],
  };
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

function buildLanes(tasks: Task[]): Lane[] {
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
    }));

  const events: TimelineItem[] = (snapshot.events ?? [])
    .filter((event) => event.agent_member_id === member.id)
    .map((event) => ({
      id: event.id,
      kind: "event" as const,
      title: event.event_type ?? "event",
      meta: event.created_at ? formatTime(event.created_at) : "event",
      body: event.summary,
      objectRef: event.task_id ?? undefined,
      createdAt: event.created_at ?? undefined,
    }));

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
