import type { DashboardSnapshot, Task, WorkflowWarning } from "../types";

const deliveredStates = new Set(["delivered", "acknowledged"]);

export function deriveWarnings(snapshot: DashboardSnapshot): WorkflowWarning[] {
  const messages = snapshot.messages ?? [];
  const evidence = snapshot.evidence ?? [];
  const proposals = snapshot.proposals ?? [];
  const decisions = snapshot.decisions ?? [];
  const sessions = snapshot.provider_sessions ?? [];

  const warnings: WorkflowWarning[] = [];

  for (const task of snapshot.tasks ?? []) {
    const taskMessages = messages.filter((message) => message.task_id === task.id);
    const assignment = taskMessages.find((message) => message.kind === "task");
    const report = taskMessages.find((message) => message.kind === "report");
    const taskEvidence = evidence.filter((item) => item.task_id === task.id);
    const taskProposal = proposals.find((proposal) => proposal.task_id === task.id);
    const taskDecision = decisions.find((decision) => decision.task_id === task.id);

    if (task.assignee_agent_id && task.status !== "planned" && !assignment) {
      warnings.push(warning("fake_assignment_risk", "high", task, "Task has an assignee but no assignment message."));
    }

    if (assignment && !deliveredStates.has(assignment.delivery_status)) {
      warnings.push(warning("assignment_not_delivered", "medium", task, "Assignment message has not reached delivered or acknowledged."));
    }

    if ((task.status === "review" || task.status === "done") && !report) {
      warnings.push(warning("missing_report", "high", task, "Task is in review/done without a report message."));
    }

    if (task.status === "review" && !taskEvidence.length) {
      warnings.push(warning("missing_evidence", "high", task, "Task is in review without linked evidence."));
    }

    if (taskProposal && !(taskProposal.evidence_ids?.length ?? 0)) {
      warnings.push(warning("proposal_missing_evidence", "medium", task, "Proposal has no evidence refs."));
    }

    if ((task.status === "done" || task.status === "review") && !taskDecision) {
      warnings.push(warning("decision_missing", "high", task, "Reviewable task lacks a Leader decision."));
    }
  }

  for (const session of sessions) {
    if (session.status === "failed" || session.status === "canceled") {
      warnings.push({
        id: `failed_provider_session:${session.id}`,
        kind: "failed_provider_session",
        severity: "medium",
        sessionId: session.id,
        memberId: session.agent_member_id,
        taskId: session.task_id ?? undefined,
        summary: "Provider session failed or was canceled.",
      });
    }
  }

  for (const learning of snapshot.goal_learning_status ?? []) {
    for (const item of learning.warnings ?? []) {
      warnings.push({
        id: `goal_learning_gap:${learning.goal_id}:${item}`,
        kind: "goal_learning_gap",
        severity: learning.ok ? "low" : "medium",
        goalId: learning.goal_id,
        summary: item,
      });
    }
  }

  return warnings;
}

function warning(kind: string, severity: WorkflowWarning["severity"], task: Task, summary: string): WorkflowWarning {
  return {
    id: `${kind}:${task.id}`,
    kind,
    severity,
    goalId: task.goal_id ?? undefined,
    taskId: task.id,
    memberId: task.assignee_agent_id ?? undefined,
    summary,
  };
}
