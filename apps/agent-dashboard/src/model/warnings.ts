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

  // Advisory: the unbound-Lead gap. Nothing in the schema binds
  // AgentTeam.owner_agent_id to a member with role==="lead"; surface the
  // divergence so the doctrinal Lead and the team owner are visibly reconciled
  // (or visibly not). Frontend-derived, non-gating, low/medium severity.
  const memberById = new Map((snapshot.members ?? []).map((member) => [member.id, member]));
  for (const team of snapshot.teams ?? []) {
    const ownerId = team.owner_agent_id;
    if (!ownerId) continue;
    const owner = memberById.get(ownerId);
    const ownerRole = owner?.role?.toLowerCase();
    // Owner resolves to a known member whose role is not "lead": medium (a real
    // mismatch). Owner does not resolve to any member at all: low (stale/partial
    // snapshot, not a doctrine violation we can assert).
    if (owner && ownerRole !== "lead") {
      warnings.push({
        id: `lead_owner_role_mismatch:${team.id}`,
        kind: "lead_owner_role_mismatch",
        severity: "medium",
        memberId: ownerId,
        summary: `Team "${team.name ?? team.id}" owner ${owner.name ?? ownerId} has role "${owner.role ?? "unset"}", not "lead" — owner_agent_id and the Lead role are unbound.`,
      });
    } else if (!owner) {
      warnings.push({
        id: `lead_owner_role_mismatch:${team.id}`,
        kind: "lead_owner_role_mismatch",
        severity: "low",
        memberId: ownerId,
        summary: `Team "${team.name ?? team.id}" owner_agent_id ${ownerId} does not resolve to a member in this snapshot.`,
      });
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

  const tasksByGoal = new Map<string, Task[]>();
  for (const task of snapshot.tasks ?? []) {
    if (!task.goal_id) continue;
    const list = tasksByGoal.get(task.goal_id) ?? [];
    list.push(task);
    tasksByGoal.set(task.goal_id, list);
  }
  const goalById = new Map((snapshot.goals ?? []).map((goal) => [goal.id, goal]));

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

    // Closeout gate (§3.7): a goal whose task graph is finished must not be
    // closed without a closeout Decision + GoalEvaluation (or a valid waiver).
    const goal = goalById.get(learning.goal_id);
    const goalTasks = tasksByGoal.get(learning.goal_id) ?? [];
    const terminalStates = new Set(["done", "archived"]);
    const graphComplete =
      goalTasks.length > 0 && goalTasks.every((task) => terminalStates.has(task.status));
    const isClosed = goal?.status === "complete" || goal?.status === "archived";
    if ((graphComplete || isClosed) && learning.may_close === false) {
      warnings.push({
        id: `goal_close_without_evaluation:${learning.goal_id}`,
        kind: "goal_close_without_evaluation",
        // A goal that is already closed without the gate is a hard violation.
        severity: isClosed ? "high" : "medium",
        goalId: learning.goal_id,
        summary: isClosed
          ? `Goal is closed without satisfying the closeout gate: ${(learning.closeout_blockers ?? ["missing closeout decision + GoalEvaluation"]).join("; ")}`
          : `Goal task graph is complete but cannot close yet: ${(learning.closeout_blockers ?? ["missing closeout decision + GoalEvaluation"]).join("; ")}`,
      });
    }

    // Waiver hygiene: any waiver decision lacking a follow-up task and evidence.
    for (const waiver of learning.closeout_waivers ?? []) {
      const hasFollowUp = Boolean(waiver.follow_up_task_id);
      const hasEvidence = (waiver.evidence_ids?.length ?? 0) > 0;
      if (!hasFollowUp || !hasEvidence) {
        warnings.push({
          id: `waiver_without_follow_up:${waiver.id}`,
          kind: "waiver_without_follow_up",
          severity: "high",
          goalId: learning.goal_id,
          taskId: waiver.task_id ?? undefined,
          evidenceId: waiver.evidence_ids?.[0],
          summary: `Waiver decision "${waiver.id}" is invalid: ${[
            hasFollowUp ? null : "missing follow_up_task_id",
            hasEvidence ? null : "missing evidence",
          ]
            .filter(Boolean)
            .join(" and ")}.`,
        });
      }
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
