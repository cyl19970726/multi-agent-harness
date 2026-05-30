import type { SelectionState } from "../app/selection";
import type {
  AgentMember,
  AgentTeam,
  DashboardSnapshot,
  Decision,
  Evidence,
  Goal,
  Message,
  Proposal,
  ProviderChildThread,
  ProviderSession,
  Task,
  TaskStatus,
  WorkflowWarning,
} from "../types";
import { deriveWarnings } from "./warnings";

export interface TimelineItem {
  id: string;
  kind: "message" | "event" | "session" | "evidence" | "proposal" | "decision" | "warning";
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
  warnings: WorkflowWarning[];
  selectedMemberMessages: Message[];
  selectedMemberTimeline: TimelineItem[];
  activity: TimelineItem[];
  decisionQueue: TimelineItem[];
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
    warnings,
    selectedMemberMessages,
    selectedMemberTimeline: selectedMember
      ? buildMemberTimeline(snapshot, selectedMember, warnings)
      : [],
    activity: buildActivity(snapshot, warnings),
    decisionQueue: buildDecisionQueue(snapshot, warnings),
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

function isGoalStatus(goal: Goal, ...statuses: string[]): boolean {
  return statuses.includes((goal.status ?? "active").toLowerCase());
}

function groupMembersByRole(members: AgentMember[]): RoleGroup[] {
  const map = new Map<string, AgentMember[]>();
  for (const member of members) {
    const role = member.role || "Member";
    map.set(role, [...(map.get(role) ?? []), member]);
  }
  return [...map.entries()].map(([role, groupMembers]) => ({ role, members: groupMembers }));
}

function buildLanes(tasks: Task[]): Lane[] {
  return laneOrder.map((status) => ({
    id: status,
    title: labelStatus(status),
    tasks: tasks.filter((task) => task.status === status),
  }));
}

function buildMemberTimeline(snapshot: DashboardSnapshot, member: AgentMember, warnings: WorkflowWarning[]): TimelineItem[] {
  const messages = (snapshot.messages ?? [])
    .filter((message) => message.from_agent_id === member.id || message.to_agent_id === member.id)
    .map((message) => ({
      id: message.id,
      kind: "message" as const,
      title: message.kind === "task" ? "Task assignment" : message.kind === "report" ? "Member report" : "Message",
      meta: `${message.delivery_status} · ${message.created_at ? formatTime(message.created_at) : "no time"}`,
      body: message.content,
      objectRef: message.task_id ?? undefined,
      createdAt: message.created_at ?? undefined,
    }));

  const sessions = (snapshot.provider_sessions ?? [])
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

  const events = (snapshot.events ?? [])
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

  const memberWarnings = warnings
    .filter((warning) => warning.memberId === member.id)
    .map((warning) => ({
      id: warning.id,
      kind: "warning" as const,
      title: warning.kind,
      meta: warning.severity,
      body: warning.summary,
      severity: warning.severity,
      objectRef: warning.taskId,
    }));

  return sortTimelineDesc([...messages, ...sessions, ...events, ...memberWarnings]).slice(0, 12);
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
