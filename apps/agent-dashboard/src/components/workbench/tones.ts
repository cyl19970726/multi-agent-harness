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
