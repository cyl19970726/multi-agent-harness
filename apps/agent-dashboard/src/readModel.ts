import type {
  AgentMember,
  AgentTeam,
  DashboardSnapshot,
  Decision,
  Goal,
  Message,
  Proposal,
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

export function taskColumns(snapshot: Required<DashboardSnapshot>): Array<{ status: TaskStatus; tasks: Task[] }> {
  const tasks = byId(snapshot.tasks);
  return taskStatuses.map((status) => ({
    status,
    tasks: (snapshot.kanban[status] ?? []).map((id) => tasks.get(id)).filter(Boolean) as Task[],
  }));
}

export function teamMembers(team: AgentTeam, members: AgentMember[]): AgentMember[] {
  const explicit = new Set(team.member_ids ?? []);
  return members.filter((member) => explicit.has(member.id) || (member.team_ids ?? []).includes(team.id));
}

export function activeGoal(snapshot: Required<DashboardSnapshot>, selectedGoalId?: string): Goal | undefined {
  return snapshot.goals.find((goal) => goal.id === selectedGoalId) ?? snapshot.goals[0];
}

export function tasksForGoal(snapshot: Required<DashboardSnapshot>, goalId?: string): Task[] {
  if (!goalId) return snapshot.tasks;
  return snapshot.tasks.filter((task) => task.goal_id === goalId);
}

export function messagesForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Message[] {
  if (!taskId) return [];
  return snapshot.messages.filter((message) => message.task_id === taskId);
}

export function messagesForMember(snapshot: Required<DashboardSnapshot>, memberId?: string): Message[] {
  if (!memberId) return [];
  return snapshot.messages.filter((message) => message.to_agent_id === memberId || message.from_agent_id === memberId);
}

export function sessionsForMember(snapshot: Required<DashboardSnapshot>, memberId?: string): ProviderSession[] {
  if (!memberId) return [];
  return snapshot.provider_sessions.filter((session) => session.agent_member_id === memberId);
}

export function proposalsForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Proposal[] {
  if (!taskId) return [];
  return snapshot.proposals.filter((proposal) => proposal.task_id === taskId);
}

export function decisionsForTask(snapshot: Required<DashboardSnapshot>, taskId?: string): Decision[] {
  if (!taskId) return [];
  return snapshot.decisions.filter((decision) => decision.task_id === taskId);
}

export function deriveWarnings(snapshot: Required<DashboardSnapshot>): WorkflowWarning[] {
  const warnings: WorkflowWarning[] = [];
  const add = (kind: string, severity: WorkflowWarning["severity"], summary: string, taskId?: string, memberId?: string) => {
    warnings.push({ id: `warning-${warnings.length}-${kind}`, kind, severity, summary, taskId, memberId });
  };

  snapshot.tasks.forEach((task) => {
    if (task.assignee_agent_id) {
      const assignments = snapshot.messages.filter(
        (message) =>
          message.kind === "task" &&
          message.task_id === task.id &&
          message.to_agent_id === task.assignee_agent_id,
      );
      if (assignments.length === 0) {
        add("fake_assignment_risk", "high", "Task has an assignee but no task assignment message.", task.id, task.assignee_agent_id);
      } else if (!assignments.some((message) => ["delivered", "acknowledged"].includes(message.delivery_status))) {
        add("assignment_not_delivered", "medium", "Assignment message exists but has not been delivered.", task.id, task.assignee_agent_id);
      }
    }

    if ((task.status === "review" || task.status === "done") && !snapshot.messages.some((message) => message.kind === "report" && message.task_id === task.id)) {
      add("missing_report", "medium", "Task is in review or done without a report message.", task.id, task.assignee_agent_id ?? undefined);
    }
  });

  snapshot.messages.forEach((message) => {
    if (message.delivery_status === "failed") {
      add("failed_delivery", "high", "Message delivery failed.", message.task_id ?? undefined, message.to_agent_id ?? undefined);
    }
    if (message.kind === "task" && message.delivery_status === "queued") {
      add("queued_task_message", "medium", "Task message is still queued.", message.task_id ?? undefined, message.to_agent_id ?? undefined);
    }
  });

  snapshot.provider_sessions.forEach((session) => {
    if (session.status === "failed") {
      add("failed_provider_session", "high", "Provider session failed.", session.task_id ?? undefined, session.agent_member_id);
    }
    if (session.status === "succeeded" && session.task_id && session.agent_member_id) {
      const hasReport = snapshot.messages.some(
        (message) =>
          message.kind === "report" &&
          message.task_id === session.task_id &&
          message.from_agent_id === session.agent_member_id,
      );
      if (!hasReport) {
        add("provider_only_claim", "medium", "Provider session exists but no report message was recorded.", session.task_id, session.agent_member_id);
      }
    }
  });

  snapshot.proposals.forEach((proposal) => {
    if ((proposal.status === "submitted" || proposal.status === "accepted") && (proposal.evidence_ids ?? []).length === 0) {
      add("proposal_missing_evidence", "high", "Submitted or accepted proposal has no evidence ids.", proposal.task_id, proposal.agent_member_id);
    }
  });

  snapshot.decisions.forEach((decision) => {
    if ((decision.evidence_ids ?? []).length === 0) {
      add("decision_missing_evidence", "medium", "Decision has no evidence ids.", decision.task_id);
    }
  });

  snapshot.goal_learning_status.forEach((status) => {
    (status.warnings ?? []).forEach((warning) => {
      add("goal_learning_gap", "medium", `Goal ${status.goal_id}: ${warning}`);
    });
  });

  return warnings;
}
