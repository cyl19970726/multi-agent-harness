import type { StatusTone } from "./atoms";

export function memberTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running": return "running";
    case "blocked":
    case "stale":
    case "failed": return "bad";
    case "idle":
    case "": return "idle";
    default: return "info";
  }
}

export function workflowRunTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running": return "running";
    case "completed": return "good";
    case "failed": return "bad";
    case "pending":
    case "paused": return "idle";
    default: return "info";
  }
}

export function workflowStepTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running": return "running";
    case "completed":
    case "cached": return "good";
    case "failed": return "bad";
    case "queued": return "idle";
    default: return "info";
  }
}
