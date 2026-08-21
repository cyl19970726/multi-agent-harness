import type { StatusTone } from "./atoms";

export function memberTone(status?: string | null): StatusTone {
  switch ((status ?? "").toLowerCase()) {
    case "running": return "running";
    case "blocked":
    case "stale":
    case "failed": return "bad";
    case "disconnected":
    case "waiting":
    case "reviewing": return "warn";
    case "queued":
    case "starting": return "info";
    case "completed": return "good";
    case "stopped": return "bad";
    case "idle":
    case "": return "idle";
    default: return "info";
  }
}
