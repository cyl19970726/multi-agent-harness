import type { DashboardSnapshot, Proposal, Task, WorkflowWarning } from "./types";

export function deriveWarnings(snapshot: Required<DashboardSnapshot>): WorkflowWarning[] {
  const warnings: WorkflowWarning[] = [];
  const taskById = new Map(snapshot.tasks.map((task) => [task.id, task]));
  const memberById = new Map(snapshot.members.map((member) => [member.id, member]));
  const evidenceIds = new Set(snapshot.evidence.map((item) => item.id));
  const add = (warning: Omit<WorkflowWarning, "id">) => {
    warnings.push({ id: `warning-${warnings.length}-${warning.kind}`, ...warning });
  };

  snapshot.tasks.forEach((task) => {
    if (task.assignee_agent_id) {
      const assignments = snapshot.messages.filter(
        (message) =>
          message.kind === "task" &&
          message.task_id === task.id &&
          message.to_agent_id === task.assignee_agent_id,
      );
      if (!assignments.length) {
        add({
          kind: "fake_assignment_risk",
          severity: "high",
          goalId: task.goal_id ?? undefined,
          taskId: task.id,
          memberId: task.assignee_agent_id,
          summary: "Task has an assignee but no task assignment message.",
        });
      } else if (!assignments.some((message) => ["delivered", "acknowledged"].includes(message.delivery_status))) {
        add({
          kind: "assignment_not_delivered",
          severity: "medium",
          goalId: task.goal_id ?? undefined,
          taskId: task.id,
          memberId: task.assignee_agent_id,
          summary: "Assignment message exists but has not been delivered.",
        });
      }
    }

    const hasReport = snapshot.messages.some((message) => message.kind === "report" && message.task_id === task.id);
    if ((task.status === "review" || task.status === "done") && !hasReport) {
      add({
        kind: "missing_report",
        severity: "medium",
        goalId: task.goal_id ?? undefined,
        taskId: task.id,
        memberId: task.assignee_agent_id ?? undefined,
        summary: "Task is in review or done without a report message.",
      });
    }
  });

  snapshot.messages.forEach((message) => {
    const task = message.task_id ? taskById.get(message.task_id) : undefined;
    const target = message.to_agent_id ? memberById.get(message.to_agent_id) : undefined;
    if (
      target != null &&
      ["closing", "closed", "retired"].includes(target.status ?? "") &&
      ["queued", "acknowledged"].includes(message.delivery_status)
    ) {
      add({
        kind: "closed_member_pending_delivery",
        severity: "high",
        goalId: task?.goal_id ?? undefined,
        taskId: message.task_id ?? undefined,
        memberId: target.id,
        summary: "Message is pending for a member that cannot receive normal delivery.",
      });
    }
    if (message.delivery_status === "failed") {
      add({
        kind: "failed_delivery",
        severity: "high",
        goalId: task?.goal_id ?? undefined,
        taskId: message.task_id ?? undefined,
        memberId: message.to_agent_id ?? undefined,
        summary: message.delivery?.last_error || "Message delivery failed.",
      });
    }
    if (message.kind === "task" && message.delivery_status === "queued") {
      add({
        kind: "queued_task_message",
        severity: "medium",
        goalId: task?.goal_id ?? undefined,
        taskId: message.task_id ?? undefined,
        memberId: message.to_agent_id ?? undefined,
        summary: "Task message is still queued.",
      });
    }
    if (message.delivery_status === "acknowledged" && message.delivery?.provider_session_id) {
      add({
        kind: "claimed_delivery_pending",
        severity: "medium",
        goalId: task?.goal_id ?? undefined,
        taskId: message.task_id ?? undefined,
        memberId: message.to_agent_id ?? undefined,
        sessionId: message.delivery.provider_session_id,
        summary: "Message has been claimed by a provider session and is waiting for terminal reconciliation.",
      });
    }
  });

  snapshot.provider_sessions.forEach((session) => {
    const task = session.task_id ? taskById.get(session.task_id) : undefined;
    if (session.status === "queued" || session.status === "running") {
      add({
        kind: "provider_session_blocks_delivery",
        severity: "medium",
        goalId: task?.goal_id ?? undefined,
        taskId: session.task_id ?? undefined,
        memberId: session.agent_member_id,
        sessionId: session.id,
        summary: `Provider session ${session.status}; later normal messages should remain queued.`,
      });
    }
    if (session.status === "failed" || session.status === "canceled") {
      add({
        kind: "failed_provider_session",
        severity: "high",
        goalId: task?.goal_id ?? undefined,
        taskId: session.task_id ?? undefined,
        memberId: session.agent_member_id,
        sessionId: session.id,
        summary: `Provider session ${session.status}.`,
      });
    }
    if (session.status === "stale" && session.terminal_source !== "failed") {
      add({
        kind: "unresolved_provider_session",
        severity: "high",
        goalId: task?.goal_id ?? undefined,
        taskId: session.task_id ?? undefined,
        memberId: session.agent_member_id,
        sessionId: session.id,
        summary: "Provider turn was accepted but no terminal event has reconciled it yet.",
      });
    }
    if (session.status === "succeeded" && session.task_id && session.agent_member_id) {
      const hasReport = snapshot.messages.some(
        (message) =>
          message.kind === "report" &&
          message.task_id === session.task_id &&
          message.from_agent_id === session.agent_member_id,
      );
      if (!hasReport) {
        add({
          kind: "provider_only_claim",
          severity: "medium",
          goalId: task?.goal_id ?? undefined,
          taskId: session.task_id,
          memberId: session.agent_member_id,
          sessionId: session.id,
          summary: "Provider session exists but no report message was recorded.",
        });
      }
    }
  });

  snapshot.proposals.forEach((proposal) => {
    const task = taskById.get(proposal.task_id);
    warnMissingEvidence(warnings, proposal, task, evidenceIds);
    if (task) warnOwnedPathViolations(warnings, proposal, task);
  });

  snapshot.decisions.forEach((decision) => {
    const task = taskById.get(decision.task_id);
    if (!(decision.evidence_ids ?? []).length) {
      add({
        kind: "decision_missing_evidence",
        severity: "medium",
        goalId: task?.goal_id ?? undefined,
        taskId: decision.task_id,
        decisionId: decision.id,
        summary: "Decision has no evidence ids.",
      });
    }
  });

  snapshot.goal_learning_status.forEach((status) => {
    (status.warnings ?? []).forEach((warning) => {
      add({
        kind: "goal_learning_gap",
        severity: "medium",
        goalId: status.goal_id,
        summary: `Goal ${status.goal_id}: ${warning}`,
      });
    });
  });

  return warnings;
}

function warnMissingEvidence(
  warnings: WorkflowWarning[],
  proposal: Proposal,
  task: Task | undefined,
  evidenceIds: Set<string>,
) {
  const missing = (proposal.evidence_ids ?? []).filter((id) => !evidenceIds.has(id));
  if ((proposal.status === "submitted" || proposal.status === "accepted") && !(proposal.evidence_ids ?? []).length) {
    warnings.push({
      id: `warning-${warnings.length}-proposal_missing_evidence`,
      kind: "proposal_missing_evidence",
      severity: "high",
      goalId: task?.goal_id ?? undefined,
      taskId: proposal.task_id,
      memberId: proposal.agent_member_id,
      proposalId: proposal.id,
      summary: "Submitted or accepted proposal has no evidence ids.",
    });
  }
  missing.forEach((evidenceId) => {
    warnings.push({
      id: `warning-${warnings.length}-proposal_bad_evidence_ref`,
      kind: "proposal_bad_evidence_ref",
      severity: "high",
      goalId: task?.goal_id ?? undefined,
      taskId: proposal.task_id,
      memberId: proposal.agent_member_id,
      proposalId: proposal.id,
      evidenceId,
      summary: `Proposal references missing evidence ${evidenceId}.`,
    });
  });
}

function warnOwnedPathViolations(warnings: WorkflowWarning[], proposal: Proposal, task: Task) {
  const ownedPaths = task.owned_paths ?? [];
  if (!ownedPaths.length) return;
  const violations = (proposal.changed_paths ?? []).filter((path) => !ownedPaths.some((owned) => path.startsWith(owned)));
  if (violations.length) {
    warnings.push({
      id: `warning-${warnings.length}-owned_path_violation`,
      kind: "owned_path_violation",
      severity: "medium",
      goalId: task.goal_id ?? undefined,
      taskId: task.id,
      memberId: proposal.agent_member_id,
      proposalId: proposal.id,
      summary: `Proposal changed paths outside task owned_paths: ${violations.slice(0, 3).join(", ")}`,
    });
  }
}
