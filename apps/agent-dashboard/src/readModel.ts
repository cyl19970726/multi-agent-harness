import type {
  AgentMember,
  AgentTeam,
  AutonomousProposal,
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
} from "./types";

const taskStatuses: TaskStatus[] = ["planned", "assigned", "running", "blocked", "review", "done", "archived"];

export const emptySnapshot: Required<DashboardSnapshot> = {
  generated_at: "sample",
  goals: [],
  teams: [],
  members: [],
  kanban: {
    planned: [],
    assigned: [],
    running: [],
    blocked: [],
    review: [],
    done: [],
    archived: [],
  },
  tasks: [],
  messages: [],
  events: [],
  proposals: [],
  autonomous_proposals: [],
  evidence: [],
  decisions: [],
  provider_sessions: [],
  provider_child_threads: [],
  goal_learning_status: [],
};

export function normalizeSnapshot(snapshot: DashboardSnapshot | null): Required<DashboardSnapshot> {
  return { ...emptySnapshot, ...(snapshot ?? {}) };
}

export function byId<T extends { id: string }>(items: T[]): Map<string, T> {
  return new Map(items.map((item) => [item.id, item]));
}

export function activeGoal(snapshot: Required<DashboardSnapshot>, selectedGoalId?: string): Goal | undefined {
  return snapshot.goals.find((goal) => goal.id === selectedGoalId) ?? snapshot.goals[0];
}

export function tasksForGoal(snapshot: Required<DashboardSnapshot>, goalId?: string): Task[] {
  if (!goalId) return snapshot.tasks;
  return snapshot.tasks.filter((task) => task.goal_id === goalId);
}

export function taskColumnsForTasks(tasks: Task[]): Array<{ status: TaskStatus; tasks: Task[] }> {
  return taskStatuses.map((status) => ({
    status,
    tasks: tasks.filter((task) => task.status === status),
  }));
}

export function membersForTasks(snapshot: Required<DashboardSnapshot>, tasks: Task[]): AgentMember[] {
  if (!tasks.length) return [];
  const taskIds = new Set(tasks.map((task) => task.id));
  const memberIds = new Set<string>();
  tasks.forEach((task) => {
    addValue(memberIds, task.owner_agent_id);
    addValue(memberIds, task.assignee_agent_id);
    addValue(memberIds, task.reviewer_agent_id);
  });
  snapshot.messages
    .filter((message) => message.task_id && taskIds.has(message.task_id))
    .forEach((message) => {
      addValue(memberIds, message.from_agent_id);
      addValue(memberIds, message.to_agent_id);
    });
  snapshot.provider_sessions
    .filter((session) => session.task_id && taskIds.has(session.task_id))
    .forEach((session) => addValue(memberIds, session.agent_member_id));
  snapshot.provider_child_threads
    .filter((thread) => thread.task_id && taskIds.has(thread.task_id))
    .forEach((thread) => addValue(memberIds, thread.agent_member_id));
  return snapshot.members.filter(
    (member) => memberIds.has(member.id) || (member.current_task_id != null && taskIds.has(member.current_task_id)),
  );
}

export function teamsForMembers(teams: AgentTeam[], members: AgentMember[]): AgentTeam[] {
  const memberIds = new Set(members.map((member) => member.id));
  return teams.filter(
    (team) =>
      (team.status == null || team.status === "active") &&
      ((team.member_ids ?? []).some((id) => memberIds.has(id)) ||
        members.some((member) => (member.team_ids ?? []).includes(team.id)) ||
        (team.owner_agent_id != null && memberIds.has(team.owner_agent_id))),
  );
}

export function teamMembers(team: AgentTeam, members: AgentMember[]): AgentMember[] {
  const explicit = new Set(team.member_ids ?? []);
  return members.filter((member) => explicit.has(member.id) || (member.team_ids ?? []).includes(team.id));
}

export function messagesForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Message[] {
  if (!taskId) return [];
  return snapshot.messages.filter((message) => message.task_id === taskId);
}

export function messagesForMember(snapshot: Required<DashboardSnapshot>, memberId?: string): Message[] {
  if (!memberId) return [];
  return snapshot.messages.filter((message) => message.to_agent_id === memberId || message.from_agent_id === memberId);
}

export function inboxForMember(snapshot: Required<DashboardSnapshot>, memberId?: string): Message[] {
  if (!memberId) return [];
  return snapshot.messages.filter((message) => message.to_agent_id === memberId);
}

export function outboxForMember(snapshot: Required<DashboardSnapshot>, memberId?: string): Message[] {
  if (!memberId) return [];
  return snapshot.messages.filter((message) => message.from_agent_id === memberId);
}

export function sessionsForMember(snapshot: Required<DashboardSnapshot>, memberId?: string): ProviderSession[] {
  if (!memberId) return [];
  return snapshot.provider_sessions.filter((session) => session.agent_member_id === memberId);
}

export function childThreadsForMember(snapshot: Required<DashboardSnapshot>, memberId?: string): ProviderChildThread[] {
  if (!memberId) return [];
  return snapshot.provider_child_threads.filter((thread) => thread.agent_member_id === memberId);
}

export function sessionsForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): ProviderSession[] {
  if (!taskId) return [];
  return snapshot.provider_sessions.filter((session) => session.task_id === taskId);
}

export function proposalsForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Proposal[] {
  if (!taskId) return [];
  return snapshot.proposals.filter((proposal) => proposal.task_id === taskId);
}

export function autonomousProposalsForGoal(
  snapshot: Required<DashboardSnapshot>,
  goalId?: string,
  tasks: Task[] = tasksForGoal(snapshot, goalId),
): AutonomousProposal[] {
  if (!goalId) return snapshot.autonomous_proposals;
  const taskIds = new Set(tasks.map((task) => task.id));
  return snapshot.autonomous_proposals.filter(
    (proposal) =>
      proposal.goal_id === goalId ||
      (proposal.task_id != null && taskIds.has(proposal.task_id)) ||
      (proposal.follow_up_goal_ids ?? []).includes(goalId),
  );
}

export function decisionsForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Decision[] {
  if (!taskId) return [];
  return snapshot.decisions.filter((decision) => decision.task_id === taskId);
}

export function evidenceForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Evidence[] {
  if (!taskId) return [];
  const ids = new Set<string>();
  const addIds = (values?: string[]) => values?.forEach((id) => ids.add(id));
  messagesForTask(snapshot, taskId).forEach((message) => addIds(message.evidence_ids));
  proposalsForTask(snapshot, taskId).forEach((proposal) => addIds(proposal.evidence_ids));
  decisionsForTask(snapshot, taskId).forEach((decision) => addIds(decision.evidence_ids));
  sessionsForTask(snapshot, taskId).forEach((session) => addIds(session.evidence_ids));
  return snapshot.evidence.filter((item) => item.task_id === taskId || ids.has(item.id));
}

export function assignmentProofForTask(snapshot: Required<DashboardSnapshot>, task?: Task): Message[] {
  if (!task?.id || !task.assignee_agent_id) return [];
  return messagesForTask(snapshot, task.id).filter(
    (message) => message.kind === "task" && message.to_agent_id === task.assignee_agent_id,
  );
}

export function reportsForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Message[] {
  return messagesForTask(snapshot, taskId).filter((message) => message.kind === "report");
}

export function reviewEvidenceForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Evidence[] {
  return evidenceForTask(snapshot, taskId).filter((item) =>
    ["critic_findings", "check_passed", "check_failed", "goal_evaluation"].includes(item.source_type ?? ""),
  );
}

export function warningsForScope(
  warnings: WorkflowWarning[],
  goalId: string | undefined,
  tasks: Task[],
  members: AgentMember[],
): WorkflowWarning[] {
  if (!goalId) return warnings;
  const taskIds = new Set(tasks.map((task) => task.id));
  const memberIds = new Set(members.map((member) => member.id));
  return warnings.filter(
    (warning) =>
      warning.goalId === goalId ||
      (warning.taskId != null && taskIds.has(warning.taskId)) ||
      (warning.memberId != null && memberIds.has(warning.memberId)),
  );
}

export interface VisionContext {
  id: string;
  title: string;
  objective: string;
  selectedGoal?: Goal;
  totalGoals: number;
  completedGoals: number;
  incompleteGoals: number;
  distanceLabel: string;
  readModelGap?: string;
}

export interface GoalCollection {
  proposed: Goal[];
  active: Goal[];
  blocked: Goal[];
  completed: Goal[];
  archived: Goal[];
}

export interface TimelineItem {
  id: string;
  kind: string;
  title: string;
  detail?: string;
  status?: string;
  taskId?: string | null;
  createdAt?: string;
}

export interface TeamWorkspaceModel {
  team?: AgentTeam;
  members: AgentMember[];
  roleGroups: Array<{ role: string; members: AgentMember[] }>;
  tasks: Task[];
  messages: Message[];
  activity: TimelineItem[];
}

export interface DocumentModel {
  goal?: Goal;
  task?: Task;
  tasks: Task[];
  assignmentMessages: Message[];
  reportMessages: Message[];
  evidence: Evidence[];
  proposals: Proposal[];
  decisions: Decision[];
}

export interface GraphKanbanModel {
  nodes: Array<{ id: string; label: string; kind: "goal" | "task"; status?: string }>;
  edges: Array<{ from: string; to: string; label: string }>;
  columns: Array<{ status: TaskStatus; tasks: Task[] }>;
}

export interface DocsContextItem {
  path: string;
  owner: string;
  status: string;
  reason: string;
}

export function activeVisionContext(snapshot: Required<DashboardSnapshot>, selectedGoalId?: string): VisionContext {
  const goal = activeGoal(snapshot, selectedGoalId);
  const completed = snapshot.goals.filter((item) => isCompletedGoal(item)).length;
  const incomplete = Math.max(snapshot.goals.length - completed, 0);
  const status = snapshot.goal_learning_status.find((item) => item.goal_id === goal?.id);
  const warnings = status?.warnings?.length ?? 0;
  const distanceLabel = warnings
    ? `${warnings} learning gaps before vision progress is provable`
    : incomplete
      ? `${incomplete} incomplete goals remain`
      : "all recorded goals are complete";
  return {
    id: "vision-self-hosting",
    title: "Self-hosting Multi-Agent Harness",
    objective: "Turn high-level goals into durable team, task graph, evidence, review, decision, and next-goal loops.",
    selectedGoal: goal,
    totalGoals: snapshot.goals.length,
    completedGoals: completed,
    incompleteGoals: incomplete,
    distanceLabel,
    readModelGap: "Snapshot has no first-class Vision object yet; Workbench derives this context from goals and learning status.",
  };
}

export function goalCollection(snapshot: Required<DashboardSnapshot>): GoalCollection {
  const collection: GoalCollection = {
    proposed: [],
    active: [],
    blocked: [],
    completed: [],
    archived: [],
  };
  snapshot.goals.forEach((goal) => {
    const status = (goal.status ?? "active").toLowerCase();
    if (isCompletedGoal(goal)) collection.completed.push(goal);
    else if (status.includes("block")) collection.blocked.push(goal);
    else if (status.includes("archive") || status.includes("reject") || status.includes("kill")) collection.archived.push(goal);
    else if (status.includes("plan") || status.includes("propos")) collection.proposed.push(goal);
    else collection.active.push(goal);
  });
  return collection;
}

export function teamWorkspace(
  snapshot: Required<DashboardSnapshot>,
  teamId?: string,
  goalId?: string,
): TeamWorkspaceModel {
  const goalTasks = tasksForGoal(snapshot, goalId);
  const goalMembers = membersForTasks(snapshot, goalTasks);
  const team =
    snapshot.teams.find((item) => item.id === teamId) ??
    teamsForMembers(snapshot.teams, goalMembers)[0] ??
    snapshot.teams[0];
  const members = team ? teamMembers(team, snapshot.members) : goalMembers.length ? goalMembers : snapshot.members;
  const memberIds = new Set(members.map((member) => member.id));
  const tasks = goalTasks.length
    ? goalTasks
    : snapshot.tasks.filter((task) =>
        [task.owner_agent_id, task.assignee_agent_id, task.reviewer_agent_id].some((id) => id != null && memberIds.has(id)),
      );
  const taskIds = new Set(tasks.map((task) => task.id));
  const messages = snapshot.messages.filter(
    (message) =>
      (message.task_id != null && taskIds.has(message.task_id)) ||
      (message.from_agent_id != null && memberIds.has(message.from_agent_id)) ||
      (message.to_agent_id != null && memberIds.has(message.to_agent_id)),
  );
  const activity = messages
    .slice(-10)
    .reverse()
    .map((message) => ({
      id: message.id,
      kind: message.kind,
      title: `${message.kind} ${message.delivery_status}`,
      detail: message.content || message.task_id || message.channel || undefined,
      status: message.delivery_status,
      taskId: message.task_id,
      createdAt: message.created_at,
    }));
  return { team, members, roleGroups: roleGroupsForMembers(members), tasks, messages, activity };
}

export function memberWorkbench(snapshot: Required<DashboardSnapshot>, memberId?: string): AgentMember | undefined {
  return memberId ? snapshot.members.find((member) => member.id === memberId) : snapshot.members[0];
}

export function memberTimeline(snapshot: Required<DashboardSnapshot>, memberId?: string): TimelineItem[] {
  if (!memberId) return [];
  const timeline: TimelineItem[] = [];
  messagesForMember(snapshot, memberId).forEach((message) => {
    timeline.push({
      id: message.id,
      kind: `message:${message.kind}`,
      title: `${message.kind} ${message.delivery_status}`,
      detail: message.content || message.channel || undefined,
      status: message.delivery_status,
      taskId: message.task_id,
      createdAt: message.created_at,
    });
  });
  sessionsForMember(snapshot, memberId).forEach((session) => {
    timeline.push({
      id: session.id,
      kind: "provider_session",
      title: `${session.provider || "provider"} ${session.status || "session"}`,
      detail: session.prompt_summary || session.provider_thread_id || session.command || undefined,
      status: session.status,
      taskId: session.task_id,
      createdAt: session.started_at,
    });
  });
  snapshot.events
    .filter((event) => event.agent_member_id === memberId)
    .forEach((event) => {
      timeline.push({
        id: event.id,
        kind: event.event_type || "event",
        title: event.summary || event.event_type || "event",
        detail: event.payload_ref || undefined,
        taskId: event.task_id,
        createdAt: event.created_at,
      });
    });
  return timeline.sort((a, b) => timestampValue(b.createdAt) - timestampValue(a.createdAt)).slice(0, 18);
}

export function goalDocument(snapshot: Required<DashboardSnapshot>, goalId?: string): DocumentModel {
  const goal = activeGoal(snapshot, goalId);
  const tasks = tasksForGoal(snapshot, goal?.id);
  const taskIds = new Set(tasks.map((task) => task.id));
  return {
    goal,
    tasks,
    assignmentMessages: snapshot.messages.filter((message) => message.kind === "task" && message.task_id != null && taskIds.has(message.task_id)),
    reportMessages: snapshot.messages.filter((message) => message.kind === "report" && message.task_id != null && taskIds.has(message.task_id)),
    evidence: snapshot.evidence.filter((item) => item.task_id != null && taskIds.has(item.task_id)),
    proposals: snapshot.proposals.filter((proposal) => taskIds.has(proposal.task_id)),
    decisions: snapshot.decisions.filter((decision) => taskIds.has(decision.task_id)),
  };
}

export function taskDocument(snapshot: Required<DashboardSnapshot>, taskId?: string): DocumentModel {
  const task = snapshot.tasks.find((item) => item.id === taskId) ?? snapshot.tasks[0];
  return {
    task,
    goal: activeGoal(snapshot, task?.goal_id ?? undefined),
    tasks: task ? [task] : [],
    assignmentMessages: assignmentProofForTask(snapshot, task),
    reportMessages: reportsForTask(snapshot, task?.id),
    evidence: evidenceForTask(snapshot, task?.id),
    proposals: proposalsForTask(snapshot, task?.id),
    decisions: decisionsForTask(snapshot, task?.id),
  };
}

export function graphKanbanModel(snapshot: Required<DashboardSnapshot>, goalId?: string): GraphKanbanModel {
  const goal = activeGoal(snapshot, goalId);
  const tasks = tasksForGoal(snapshot, goal?.id);
  const nodes: GraphKanbanModel["nodes"] = [];
  const edges: GraphKanbanModel["edges"] = [];
  if (goal) nodes.push({ id: goal.id, label: goal.title || goal.id, kind: "goal", status: goal.status });
  tasks.forEach((task) => {
    nodes.push({ id: task.id, label: task.title || task.id, kind: "task", status: task.status });
    if (goal) edges.push({ from: goal.id, to: task.id, label: "owns" });
    (task.depends_on_task_ids ?? []).forEach((dependency) => edges.push({ from: dependency, to: task.id, label: "blocks" }));
  });
  return { nodes, edges, columns: taskColumnsForTasks(tasks) };
}

export function decisionQueue(snapshot: Required<DashboardSnapshot>, goalId?: string): Decision[] {
  if (!goalId) return snapshot.decisions.slice(-8).reverse();
  const taskIds = new Set(tasksForGoal(snapshot, goalId).map((task) => task.id));
  return snapshot.decisions.filter((decision) => taskIds.has(decision.task_id)).slice(-8).reverse();
}

export function docsContext(_snapshot: Required<DashboardSnapshot>, objectRef?: string): DocsContextItem[] {
  const suffix = objectRef ? `Related to ${objectRef}` : "Workbench context";
  return [
    { path: "docs/prd.md", owner: "Product", status: "canonical", reason: `Vision and product purpose. ${suffix}.` },
    { path: "docs/concept-model.md", owner: "Architecture", status: "canonical", reason: "Canonical object relationships and anti-drift invariants." },
    { path: "docs/dashboard/frontend-design.md", owner: "Frontend", status: "canonical", reason: "Accepted Workbench page map and interaction design." },
    { path: "docs/dashboard/hard-layout-specs/agent-workbench-shell-v1.md", owner: "Frontend", status: "active spec", reason: "Current implementation shell contract." },
    { path: "docs/goal-learning-loop.md", owner: "Workflow", status: "canonical", reason: "Goal evaluation, follow-up, and next-goal loop." },
  ];
}

export function warningsByObject(warnings: WorkflowWarning[]): Map<string, WorkflowWarning[]> {
  const grouped = new Map<string, WorkflowWarning[]>();
  warnings.forEach((warning) => {
    [warning.goalId, warning.taskId, warning.memberId, warning.proposalId, warning.decisionId, warning.sessionId]
      .filter((id): id is string => Boolean(id))
      .forEach((id) => {
        const next = grouped.get(id) ?? [];
        next.push(warning);
        grouped.set(id, next);
      });
  });
  return grouped;
}

function roleGroupsForMembers(members: AgentMember[]): Array<{ role: string; members: AgentMember[] }> {
  const groups = new Map<string, AgentMember[]>();
  members.forEach((member) => {
    const role = member.role || member.provider_agent_role || "Member";
    const next = groups.get(role) ?? [];
    next.push(member);
    groups.set(role, next);
  });
  return Array.from(groups.entries()).map(([role, groupMembers]) => ({ role, members: groupMembers }));
}

function isCompletedGoal(goal: Goal): boolean {
  const status = (goal.status ?? "").toLowerCase();
  return ["done", "complete", "completed", "accepted", "closed"].some((value) => status.includes(value));
}

function timestampValue(value?: string): number {
  if (!value) return 0;
  const unix = value.match(/^unix-ms:(\d+)$/);
  if (unix) return Number(unix[1]);
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function addValue(values: Set<string>, value?: string | null) {
  if (value) values.add(value);
}
