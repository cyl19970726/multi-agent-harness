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

function addValue(values: Set<string>, value?: string | null) {
  if (value) values.add(value);
}
