import type { DashboardSnapshot, Task, WorkflowWarning } from "../types";

const deliveredStates = new Set(["delivered", "acknowledged"]);

export function deriveWarnings(snapshot: DashboardSnapshot): WorkflowWarning[] {
  const messages = snapshot.messages ?? [];
  const evidence = snapshot.evidence ?? [];
  const proposals = snapshot.proposals ?? [];
  const decisions = snapshot.decisions ?? [];
  const sessions = snapshot.provider_sessions ?? [];
  const reviews = snapshot.reviews ?? [];
  const gaps = snapshot.gaps ?? [];

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

  const failingVerdicts = new Set(["fail", "blocked", "needs_changes"]);
  for (const review of reviews) {
    const verdict = (review.verdict ?? "").toLowerCase();
    const hasBlockers = (review.blockers?.length ?? 0) > 0;
    if (failingVerdicts.has(verdict) || hasBlockers) {
      warnings.push({
        // "decision" in the kind routes this into the decision queue area.
        id: `review_needs_decision:${review.id}`,
        kind: "review_needs_decision",
        severity: verdict === "fail" || verdict === "blocked" ? "high" : "medium",
        goalId: review.goal_id ?? undefined,
        taskId: review.task_id ?? undefined,
        evidenceId: review.evidence_ids?.[0],
        summary: `Review verdict "${review.verdict ?? "unknown"}" needs a Leader decision: ${review.summary ?? review.id}`,
      });
    }
  }

  const unresolvedGapStatuses = new Set(["open", "in_progress", "blocked"]);
  for (const gap of gaps) {
    const severity = (gap.severity ?? "").toLowerCase();
    const status = (gap.status ?? "open").toLowerCase();
    if (!unresolvedGapStatuses.has(status)) continue;
    // A P0 gap that is still open is the highest-priority repair signal.
    if (severity === "p0" && status === "open") {
      warnings.push({
        id: `gap_p0_open:${gap.id}`,
        kind: "gap_p0_open",
        severity: "high",
        goalId: gap.goal_id ?? undefined,
        taskId: gap.task_id ?? undefined,
        evidenceId: gap.evidence_ids?.[0],
        summary: `P0 gap (${gap.category ?? "uncategorized"}) is still open: ${gap.summary ?? gap.id}`,
      });
      continue;
    }
    // Other unresolved gaps surface at a severity mapped from p0/p1/p2.
    warnings.push({
      id: `gap_unresolved:${gap.id}`,
      kind: "gap_unresolved",
      severity: severity === "p0" ? "high" : severity === "p1" ? "medium" : "low",
      goalId: gap.goal_id ?? undefined,
      taskId: gap.task_id ?? undefined,
      evidenceId: gap.evidence_ids?.[0],
      summary: `Unresolved ${gap.severity ?? "gap"} (${gap.category ?? "uncategorized"}, ${status}): ${gap.summary ?? gap.id}`,
    });
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
