import type { StatusTone } from "@/components/workbench/atoms";
import type { MemberRun, TeamMessage } from "../../../types";

/**
 * Presentation helpers shared by the Team War Room composition and its
 * extracted Works / Activity / Members components.
 *
 * These map durable status vocabulary onto tone and label. They never invent a
 * runtime fact: an unknown status falls back to a neutral tone rather than
 * implying health, and every formatter returns an explicit "not recorded"
 * string instead of an empty value.
 */

export function shortId(value: string): string {
  return value.length > 18 ? `${value.slice(0, 8)}…${value.slice(-5)}` : value;
}

export function timestamp(value?: string | null): number {
  if (!value) return 0;
  return value.startsWith("unix-ms:") ? Number(value.slice(8)) || 0 : Date.parse(value) || 0;
}

export function formatDate(value?: string | null): string {
  if (!value) return "Not recorded";
  const ms = timestamp(value);
  return ms
    ? new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(ms)
    : value;
}

export function formatTime(value?: string | null): string {
  if (!value) return "—";
  const ms = timestamp(value);
  return ms ? new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(ms) : value;
}

/** Absolute, locale-formatted time used for the accessible `title`/disclosure
 * of a row whose visible label is deliberately short. */
export function formatAbsolute(value?: string | null): string {
  if (!value) return "Time not recorded";
  const ms = timestamp(value);
  return ms
    ? new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "medium" }).format(ms)
    : value;
}

/** Machine-readable value for `<time dateTime>`; empty when nothing is recorded. */
export function isoTime(value?: string | null): string | undefined {
  const ms = timestamp(value);
  return ms ? new Date(ms).toISOString() : undefined;
}

export function relativeTime(value?: string | null): string {
  const ms = timestamp(value);
  if (!ms) return "no update";
  const delta = Math.max(0, Date.now() - ms);
  if (delta < 60_000) return "just now";
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`;
  if (delta < 86_400_000) return `${Math.floor(delta / 3_600_000)}h ago`;
  return `${Math.floor(delta / 86_400_000)}d ago`;
}

export function pressureLabel(status?: string | null): string {
  if (["blocked", "failed"].includes(status ?? "")) return "blocked";
  if (["waiting", "reviewing"].includes(status ?? "")) return "waiting";
  if (status === "running") return "active";
  return status ?? "idle";
}

export function memberPressureRank(status?: string | null): number {
  if (["blocked", "failed"].includes(status ?? "")) return 0;
  if (status === "disconnected") return 1;
  if (["waiting", "reviewing"].includes(status ?? "")) return 1;
  if (status === "running") return 2;
  if (status === "idle") return 3;
  if (status === "completed") return 4;
  return 5;
}

export function teamTone(status?: string | null): StatusTone {
  if (status === "running") return "running";
  if (status === "completed") return "good";
  if (["failed", "cancelled"].includes(status ?? "")) return "bad";
  if (["waiting", "reviewing"].includes(status ?? "")) return "warn";
  if (status === "planning") return "info";
  return "idle";
}

export function memberTone(status?: string | null): StatusTone {
  if (status === "running") return "running";
  if (status === "completed") return "good";
  if (["blocked", "failed", "stopped"].includes(status ?? "")) return "bad";
  if (["waiting", "reviewing", "disconnected"].includes(status ?? "")) return "warn";
  if (["queued", "starting"].includes(status ?? "")) return "info";
  return "idle";
}

export function workTone(status?: string | null): StatusTone {
  if (status === "done") return "good";
  if (status === "cancelled") return "bad";
  if (status === "blocked") return "warn";
  if (status === "in_progress") return "running";
  if (status === "review") return "info";
  return "idle";
}

export function waveTone(status?: string | null): StatusTone {
  if (status === "completed") return "good";
  if (["blocked", "failed", "cancelled"].includes(status ?? "")) return "bad";
  if (["waiting"].includes(status ?? "")) return "warn";
  if (status === "running") return "running";
  return "info";
}

export function gateTone(status?: string | null): StatusTone {
  if (status === "accepted") return "good";
  if (status === "blocked") return "bad";
  if (status === "revise") return "warn";
  return "decision";
}

export function messageTone(kind?: string | null): StatusTone {
  if (kind === "blocker") return "bad";
  if (["review_request", "plan_feedback"].includes(kind ?? "")) return "warn";
  if (["review_result", "answer", "plan_approval"].includes(kind ?? "")) return "good";
  if (["handoff", "question", "plan_proposal"].includes(kind ?? "")) return "decision";
  if (kind === "progress") return "running";
  if (["broadcast", "plan_request"].includes(kind ?? "")) return "info";
  return "idle";
}

export function memberLabel(members: Map<string, MemberRun>, id: string): string {
  return id === "host" ? "Host" : members.get(id)?.name ?? id;
}

export function teamLeadLabel(leadAgentId?: string | null): string {
  if (!leadAgentId || leadAgentId === "host") return "Current Host Agent";
  return leadAgentId;
}

export function attemptNumber(attempts: Array<{ id: string }>, id: string): number {
  return Math.max(1, attempts.findIndex((attempt) => attempt.id === id) + 1);
}

export function teamMessageActorLabel(message: TeamMessage, members: Map<string, MemberRun>): string {
  if (message.sender?.display_name) return message.sender.display_name;
  if (message.sender?.kind === "operator") return "Operator";
  if (message.sender?.kind === "service") return `Service · ${message.sender.id}`;
  if (message.sender?.kind === "agent_member") return `Agent · ${message.sender.id}`;
  return memberLabel(members, message.from_member_id ?? message.sender?.id ?? "host");
}

export function messageSenderParticipantId(message: TeamMessage): string | undefined {
  if (message.sender?.kind === "operator" || message.sender?.kind === "service") return undefined;
  if (message.sender?.kind === "host") return "host";
  return message.from_member_id;
}

export function hostDelivery(message: TeamMessage) {
  return message.deliveries?.find((delivery) => delivery.member_id === "host");
}

export function hostDeliveryStatus(message: TeamMessage): string | undefined {
  return hostDelivery(message)?.status;
}
