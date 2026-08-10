import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Markdown } from "@/components/workbench/Markdown";
import type { WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";

import type { StableTeamActivity } from "../../../model/teamSelectors";
import type { MemberRun, PendingInteraction, ProviderDispatchEnvelope } from "../../../types";
import { effectiveTeamMessageResponseIntent } from "../../../types";
import {
  formatTime,
  memberLabel,
  memberTone,
  messageTone,
  shortId,
  teamMessageActorLabel,
} from "./teamFormat";

/**
 * Projection from durable Team records onto renderable activity rows.
 *
 * This is display shaping only. It never merges, rewrites, or suppresses a
 * durable record: `prominence` selects the default projection while the full
 * record set stays reachable behind the Activity filter controls.
 */

export type TeamActivityItem = WorkbenchActivityItem & { workId?: string };

export type StreamFilter = "all" | "messages" | "actions" | "decisions" | "evidence";

export const FILTERS: Array<{ id: StreamFilter; label: string }> = [
  { id: "all", label: "All" },
  { id: "messages", label: "Messages" },
  { id: "actions", label: "Actions" },
  { id: "decisions", label: "Decisions" },
  { id: "evidence", label: "Evidence" },
];

export const KEY_ACTIVITY_MESSAGE_KINDS = new Set([
  "message",
  "plan_request",
  "plan_proposal",
  "plan_feedback",
  "plan_approval",
  "question",
  "answer",
  "handoff",
]);

export function matchesFilter(item: WorkbenchActivityItem, filter: StreamFilter): boolean {
  if (filter === "all") return true;
  if (filter === "messages") return item.kind === "message" || item.kind === "blocker";
  if (filter === "actions") return item.kind === "action";
  if (filter === "decisions") return item.kind === "decision";
  return item.kind === "evidence" || Boolean(item.evidenceRefs?.length);
}

export function teamMessageGlyph(kind?: string | null, hasEvidence = false): WorkbenchActivityItem["glyph"] {
  if (hasEvidence) return "artifact";
  switch (kind) {
    case "handoff": return "handoff";
    case "review_request": return "review";
    case "review_result": return "decision";
    default: return "message";
  }
}

export function summarizeDeliveries(message: ProviderDispatchEnvelope, members: Map<string, MemberRun>): string | undefined {
  const deliveries = message.deliveries ?? [];
  if (!deliveries.length) return undefined;
  const acknowledged = deliveries.filter((delivery) => delivery.status === "acknowledged").length;
  const delivered = deliveries.filter((delivery) => delivery.status === "delivered").length;
  const queued = deliveries.filter((delivery) => delivery.status === "queued").length;
  const claimed = deliveries.filter((delivery) => delivery.status === "claimed").length;
  const nextRoundBatched = deliveries.filter((delivery) =>
    delivery.status === "queued"
    && delivery.member_id
    && members.get(delivery.member_id)?.provider_profile?.ordinary_message_boundary === "next_round_batched"
  ).length;
  if (acknowledged === deliveries.length) return `ACK ${acknowledged}/${deliveries.length}`;
  if (claimed) return `${claimed} provider receipt pending${queued ? ` · ${queued} queued` : ""}`;
  if (queued) {
    // Informational mail never starts a provider round on its own (ADR 0046
    // §4): show that it waits for the next response-required trigger instead
    // of implying an imminent delivery.
    if (effectiveTeamMessageResponseIntent(message) === "informational") {
      return `${queued} informational · batched on next response round${delivered || acknowledged ? ` · ${delivered + acknowledged} received` : ""}`;
    }
    const queuedLabel = nextRoundBatched === queued
      ? `${queued} response required · next provider round`
      : nextRoundBatched
        ? `${queued} response required · ${nextRoundBatched} next-round`
        : `${queued} response required · queued`;
    return `${queuedLabel}${delivered || acknowledged ? ` · ${delivered + acknowledged} received` : ""}`;
  }
  return `${delivered} delivered${acknowledged ? ` · ${acknowledged} ACK` : ""}`;
}

export function toInteractionActivity(
  interaction: PendingInteraction,
  members: Map<string, MemberRun>,
  actionsEnabled: boolean,
  onResolve: (optionId: string) => void,
): WorkbenchActivityItem {
  return {
    id: `interaction:${interaction.id}`,
    kind: "decision",
    glyph: interaction.kind === "question" ? "message" : "decision",
    title: (
      <span className="inline-flex flex-wrap items-center gap-2">
        <span>{interaction.title}</span>
        <Badge tone="warn">{interaction.route} decision</Badge>
      </span>
    ),
    body: interaction.prompt,
    actor: memberLabel(members, interaction.member_run_id),
    timestamp: formatTime(interaction.created_at),
    occurredAt: interaction.created_at,
    tone: "warn",
    prominence: "pressure",
    action: (
      <div className="flex max-w-72 flex-wrap justify-end gap-1.5 rounded-lg border border-status-warn/25 bg-status-warn/[0.055] p-2">
        {interaction.options.map((option) => (
          <Button
            key={option.id}
            size="sm"
            className="min-h-11 sm:min-h-0"
            variant={option.intent?.startsWith("reject") ? "secondary" : "default"}
            disabled={!actionsEnabled || interaction.route === "policy"}
            onClick={() => onResolve(option.id)}
          >
            {option.label}
          </Button>
        ))}
        {interaction.route === "policy" && (
          <span className="self-center text-[10px] text-muted-foreground">Awaiting governed policy decision</span>
        )}
        {interaction.options.length === 0 && (
          <span className="text-[10px] text-muted-foreground">No compatible response option</span>
        )}
      </div>
    ),
  };
}

export function toActivityItems(
  items: StableTeamActivity[],
  members: Map<string, MemberRun>,
  onOpenMember: (member: MemberRun) => void,
): TeamActivityItem[] {
  return items.map((item) => {
    const actor = item.sourceMemberRunId ? memberLabel(members, item.sourceMemberRunId) : "Host";
    if (item.kind === "message") {
      const message = item.message;
      const messageActor = teamMessageActorLabel(message, members);
      const recipients = (message.recipient_runtime_ids ?? []).map((id) => memberLabel(members, id)).join(", ") || "team";
      const evidenceRefs = message.evidence_refs ?? [];
      const actorMember = message.sender?.kind === "member_run" || !message.sender
        ? (message.sender_runtime_id ? members.get(message.sender_runtime_id) : undefined)
        : undefined;
      const deliverySummary = summarizeDeliveries(message, members);
      return {
        id: item.id,
        kind: message.kind === "blocker" ? "blocker" : message.kind === "review_result" ? "decision" : evidenceRefs.length ? "evidence" : "message",
        glyph: teamMessageGlyph(message.kind, evidenceRefs.length > 0),
        title: (
          <span className="flex flex-wrap items-center gap-2">
            <span>{messageActor} <span className="font-normal text-muted-foreground">to {recipients}</span></span>
            <Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge>
            {deliverySummary && <span className="text-[10px] font-normal text-muted-foreground">{deliverySummary}</span>}
          </span>
        ),
        body: message.body ? <Markdown source={message.body} compact /> : undefined,
        actor: message.correlation_id
          ? `work chain ${shortId(message.correlation_id)}${message.causation_id ? ` · reply to ${shortId(message.causation_id)}` : ""}`
          : undefined,
        timestamp: formatTime(message.created_at),
        occurredAt: message.created_at,
        evidenceRefs,
        tone: messageTone(message.kind),
        actorAvatarName: messageActor,
        actorTone: actorMember ? memberTone(actorMember.status) : "info",
        onActorClick: actorMember ? () => onOpenMember(actorMember) : undefined,
        relatedMemberIds: [
          ...(message.sender_runtime_id ? [message.sender_runtime_id] : []),
          ...(message.recipient_runtime_ids ?? []),
        ],
        rawText: `${message.kind ?? ""} ${message.body ?? ""} ${messageActor} ${recipients} ${message.correlation_id ?? ""} ${message.causation_id ?? ""}`,
        actorLabel: messageActor,
        statusLabel: deliverySummary,
        messageKind: message.kind ?? "message",
        bodySource: message.body ?? undefined,
        recipientLabels: (message.recipient_runtime_ids ?? []).map((id) => memberLabel(members, id)),
        workId: message.work_id ?? undefined,
        prominence: KEY_ACTIVITY_MESSAGE_KINDS.has(message.kind ?? "")
          ? "primary"
          : ["blocker", "review_request", "review_result"].includes(message.kind ?? "")
            ? "pressure"
            : "detail",
      };
    }
    if (item.kind === "action") {
      const action = item.action;
      const evidenceRefs = action.evidence_refs ?? [];
      const status = [action.provider_status, action.semantic_status].filter(Boolean).join(" · ");
      return { id: item.id, kind: evidenceRefs.length ? "evidence" : "action", glyph: evidenceRefs.length ? "artifact" : "runtime", title: action.title ?? action.action_type ?? "Member action", body: status ? <><span>{action.summary}</span><span className="mt-1 block text-[10px] text-muted-foreground">Harness action · provider {action.provider_status ?? "unknown"} · semantic {action.semantic_status ?? "not classified"}</span></> : action.summary, actor, timestamp: formatTime(action.started_at ?? action.completed_at), occurredAt: action.started_at ?? action.completed_at, evidenceRefs, tone: action.status === "failed" ? "bad" : action.status === "succeeded" ? "good" : "running", prominence: "detail", relatedMemberIds: item.sourceMemberRunId ? [item.sourceMemberRunId] : [], rawText: `${action.title ?? ""} ${action.summary ?? ""} ${actor}`, actorLabel: actor };
    }
    if (item.kind === "work_event") {
      const event = item.workEvent;
      const pressure = event.kind.includes("block")
        || event.kind.includes("cancel")
        || event.kind === "changes_requested";
      const accepted = event.kind.includes("accept") || event.kind.includes("complete");
      return {
        id: item.id,
        kind: pressure ? "blocker" : accepted ? "decision" : "action",
        glyph: accepted ? "complete" : pressure ? "review" : "runtime",
        title: `Work ${event.kind.replace(/_/g, " ")}`,
        body: `Version ${event.expected_version} → ${event.resulting_version}`,
        actor,
        actorLabel: actor,
        timestamp: formatTime(event.created_at),
        occurredAt: event.created_at,
        tone: pressure ? "bad" : accepted ? "good" : "info",
        prominence: pressure ? "pressure" : "detail",
        relatedMemberIds: item.sourceMemberRunId ? [item.sourceMemberRunId] : [],
        rawText: `${event.kind} ${event.work_id} ${actor}`,
        workId: event.work_id,
      };
    }
    if (item.kind === "work_delivery") {
      const delivery = item.workDelivery;
      const recipient = memberLabel(members, delivery.recipient_member_run_id);
      const pressure = delivery.status === "failed";
      const deliveryBody = pressure && delivery.failure_reason
        ? `Work v${delivery.work_version} · attempt ${delivery.attempt} · ${delivery.failure_reason}`
        : `Work v${delivery.work_version} · attempt ${delivery.attempt}`;
      return {
        id: item.id,
        kind: pressure ? "blocker" : "action",
        glyph: pressure ? "review" : delivery.status === "provider_received" ? "complete" : "queued",
        title: `Work delivery ${delivery.status.replace(/_/g, " ")}`,
        body: deliveryBody,
        actor: `to ${recipient}`,
        actorLabel: recipient,
        timestamp: formatTime(delivery.updated_at),
        occurredAt: delivery.updated_at,
        tone: pressure ? "bad" : delivery.status === "provider_received" ? "good" : "info",
        prominence: pressure ? "pressure" : "detail",
        relatedMemberIds: [delivery.recipient_member_run_id],
        rawText: `${delivery.status} ${delivery.work_id} ${recipient} ${delivery.failure_reason ?? ""}`,
        workId: delivery.work_id,
      };
    }
    const event = item.event;
    const decision = event.entity_type === "wave" || event.operation === "completed" || /gate|decision/i.test(event.summary ?? "");
    return { id: item.id, kind: decision ? "decision" : "action", glyph: decision ? "decision" : "runtime", title: event.summary ?? `${event.entity_type ?? "Team"} ${event.operation ?? "updated"}`, actor, timestamp: formatTime(event.occurred_at), occurredAt: event.occurred_at, tone: decision ? "decision" : "info", prominence: "detail", relatedMemberIds: item.sourceMemberRunId ? [item.sourceMemberRunId] : [], rawText: `${event.summary ?? ""} ${actor}`, actorLabel: actor };
  });
}
