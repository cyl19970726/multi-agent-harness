import type { StatusTone } from "./atoms";

/** Map a Task.status to a status tone. */
export function taskTone(status?: string | null): StatusTone {
  switch (status) {
    case "running":
      return "running";
    case "done":
      return "good";
    case "blocked":
      return "bad";
    case "review":
      return "warn";
    case "assigned":
      return "info";
    default:
      return "idle";
  }
}

/** Map a member runtime/status string to a status tone. */
export function memberTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running":
      return "running";
    case "blocked":
    case "stale":
    case "failed":
      return "bad";
    case "idle":
      return "idle";
    case "":
      return "idle";
    default:
      return "info";
  }
}

/** Map a goal status to a status tone. */
export function goalTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "active":
      return "running";
    case "complete":
    case "done":
      return "good";
    case "blocked":
      return "bad";
    case "proposed":
    case "planned":
      return "decision";
    default:
      return "idle";
  }
}

/** Map a GoalPhase.status to a status tone (goal-planning-model). */
export function phaseStatusTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "in_progress":
      return "running";
    case "passed":
      return "good";
    case "failed":
      return "bad";
    case "blocked":
      return "bad";
    case "not_started":
      return "idle";
    default:
      return "idle";
  }
}

/** Map a Knowledge.source to a status tone (goal-planning-model). */
export function knowledgeSourceTone(source?: string | null): StatusTone {
  switch ((source ?? "").toLowerCase()) {
    case "task":
      return "running";
    case "decision":
      return "decision";
    case "evidence":
      return "good";
    case "exploration":
      return "info";
    default:
      return "info";
  }
}

/** Map an ArtifactKind to a status tone (goal-phase-artifacts). */
export function artifactKindTone(kind?: string | null): StatusTone {
  switch ((kind ?? "code").toLowerCase()) {
    case "code":
      return "running";
    case "test_report":
      return "good";
    case "design_doc":
    case "adr":
    case "migration_doc":
      return "info";
    case "registered_doc":
      return "decision";
    case "screenshot":
      return "warn";
    default:
      return "idle";
  }
}

/** Map a Review.verdict (open enum) to a status tone. */
export function reviewVerdictTone(verdict?: string | null): StatusTone {
  switch ((verdict ?? "").toLowerCase()) {
    case "pass":
      return "good";
    case "fail":
    case "blocked":
      return "bad";
    case "needs_changes":
      return "warn";
    default:
      return "info";
  }
}

/** Map a warning severity to a status tone. */
export function severityTone(severity?: "high" | "medium" | "low"): StatusTone {
  return severity === "high" ? "bad" : severity === "medium" ? "warn" : "info";
}

/** Map a Gap.severity (p0/p1/p2) to a status tone. */
export function gapSeverityTone(severity?: string | null): StatusTone {
  switch ((severity ?? "").toLowerCase()) {
    case "p0":
      return "bad";
    case "p1":
      return "warn";
    case "p2":
      return "info";
    default:
      return "idle";
  }
}

/** Map a Gap.status to a status tone. */
export function gapStatusTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "fixed":
      return "good";
    case "wontfix":
    case "deferred":
      return "idle";
    case "blocked":
      return "bad";
    case "in_progress":
      return "running";
    case "open":
      return "warn";
    default:
      return "idle";
  }
}

/** Map a WorkflowRun.status to a status tone. */
export function workflowRunTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running":
      return "running";
    case "completed":
      return "good";
    case "failed":
      return "bad";
    case "pending":
    case "paused":
      return "idle";
    default:
      return "idle";
  }
}

/** Map a WorkflowStep.status to a status tone. */
export function workflowStepTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running":
      return "running";
    case "completed":
      return "good";
    case "failed":
      return "bad";
    case "queued":
      return "idle";
    case "cached":
      return "info";
    default:
      return "idle";
  }
}

/** Map a timeline item kind (+ severity for warnings) to a status tone. */
export function timelineTone(
  kind: string,
  severity?: "high" | "medium" | "low",
): StatusTone {
  if (kind === "warning") return severityTone(severity);
  switch (kind) {
    case "message":
      return "info";
    case "proposal":
      return "decision";
    case "decision":
      return "good";
    case "session":
      return "running";
    default:
      return "idle";
  }
}
