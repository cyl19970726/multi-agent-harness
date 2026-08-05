import { ArrowRight, Inbox, Mail, MessageSquare, SendHorizontal, SquareArrowOutUpRight } from "lucide-react";

import { cn } from "@/lib/utils";
import { memberModelLabel, providerStackLine } from "@/lib/provider";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Avatar } from "@/components/workbench/Avatar";
import type { StatusTone } from "@/components/workbench/atoms";

import type { MemberRun, TeamMessage } from "../../../types";
import { effectiveTeamMessageResponseIntent } from "../../../types";
import {
  formatTime,
  hostDelivery,
  hostDeliveryStatus,
  memberTone,
  messageSenderParticipantId,
  messageTone,
  shortId,
  teamMessageActorLabel,
} from "./teamFormat";

/**
 * Coordination-pressure band for the Team War Room.
 *
 * Mailboxes are read-model projections over TeamMessage recipients and
 * delivery rows, never a separate stored object, and the Host mailbox exists
 * without fabricating a Host MemberRun.
 */
export function TeamCoordinationPressure({
  members,
  messages,
  pendingInteractions,
  onOpenActivity,
  className,
}: {
  members: MemberRun[];
  messages: TeamMessage[];
  pendingInteractions: number;
  onOpenActivity: () => void;
  className?: string;
}) {
  const unread = messages.filter((message) => hostDeliveryStatus(message) === "delivered").length;
  const blocked = members.filter((member) => member.status === "blocked").length;
  return (
    <button
      type="button"
      onClick={onOpenActivity}
      className={cn(
        "flex w-full flex-wrap items-center gap-x-4 gap-y-1 rounded-xl border border-border/70 bg-card/70 px-3 py-2 text-left transition-colors hover:border-primary/25 hover:bg-primary/[0.025]",
        className,
      )}
      aria-label="Open complete Team mailboxes and Lead Inbox"
    >
      <span className="inline-flex items-center gap-2 text-[11px] font-semibold text-foreground"><Inbox className="size-3.5 text-primary" /> <span className="hidden sm:inline">Coordination pressure</span><span className="sm:hidden">Pressure</span></span>
      <span className={cn("text-[10px]", unread ? "text-status-warn" : "text-muted-foreground")}>{unread} unread<span className="hidden sm:inline"> for Lead</span></span>
      <span className={cn("text-[10px]", pendingInteractions ? "text-status-warn" : "text-muted-foreground")}>{pendingInteractions} pending<span className="hidden sm:inline"> interaction{pendingInteractions === 1 ? "" : "s"}</span></span>
      <span className={cn("text-[10px]", blocked ? "text-status-bad" : "text-muted-foreground")}>{blocked} blocked<span className="hidden sm:inline"> member{blocked === 1 ? "" : "s"}</span></span>
      <span className="ml-auto inline-flex items-center gap-1 text-[10px] font-medium text-primary"><span className="hidden sm:inline">Open Activity</span><ArrowRight className="size-3" /></span>
    </button>
  );
}

export function TeamMailboxStrip({
  members,
  messages,
  selectedId,
  selectedMemberId,
  showAllMembers,
  onToggleAllMembers,
  onSelect,
  onOpenMember,
}: {
  members: MemberRun[];
  messages: TeamMessage[];
  selectedId: string;
  selectedMemberId?: string;
  showAllMembers: boolean;
  onToggleAllMembers: () => void;
  onSelect: (id: string) => void;
  onOpenMember: (member: MemberRun) => void;
}) {
  const participants = [
    { id: "host", name: "Host Lead", role: "Team lead", tone: "info" as StatusTone, member: undefined },
    ...members.map((member) => ({
      id: member.id,
      name: member.name ?? member.id,
      role: member.role ?? "member",
      tone: memberTone(member.status),
      member,
    })),
  ];
  return (
    <section aria-label="Participant mailboxes" className="border-b border-border/70 pb-1.5">
      <header className="mb-1 flex items-end justify-between gap-3">
        <div>
          <h2 className="text-[13px] font-semibold text-foreground">Team mailboxes</h2>
          <p className="hidden text-[10px] text-muted-foreground sm:block">Inbox and Outbox are live projections of coordination delivery, not duplicate stored objects.</p>
        </div>
        <button type="button" onClick={() => onSelect("all")} className="text-[11px] font-medium text-primary hover:underline">
          All activity
        </button>
      </header>
      <div className="flex snap-x gap-2 overflow-x-auto pb-1 xl:grid xl:grid-cols-5 xl:overflow-visible" data-testid="team-mailbox-strip">
        {participants.map((participant, index) => {
          const inbox = messages.filter((message) => (message.to_member_ids ?? []).includes(participant.id));
          const outbox = messages.filter(
            (message) => messageSenderParticipantId(message) === participant.id,
          );
          const awaiting = inbox.filter((message) => message.deliveries?.some(
            (delivery) => delivery.member_id === participant.id && ["queued", "delivered"].includes(delivery.status ?? ""),
          )).length;
          return (
            <article
              key={participant.id}
              className={cn(
                "relative min-w-[13rem] snap-start rounded-lg border bg-card/65 p-1.5 shadow-[0_12px_32px_-30px_rgba(15,23,42,.75)] transition-colors xl:min-w-0",
                selectedId === participant.id ? "border-primary/45 bg-primary/[0.045]" : "border-border/75 hover:border-primary/25",
                index > 1 && !showAllMembers ? "hidden sm:block" : undefined,
              )}
            >
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  data-testid={`mailbox-${participant.id}`}
                  onClick={() => onSelect(participant.id)}
                  className="flex min-w-0 flex-1 items-center gap-2 rounded-md text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                >
                  <Avatar name={participant.name} tone={participant.tone} />
                  <span className="min-w-0">
                    <span className="block truncate text-[11px] font-semibold text-foreground">{participant.name}</span>
                    <span className="block truncate text-[9px] uppercase tracking-wider text-muted-foreground">{participant.role}</span>
                    {participant.member && (
                      <span
                        data-testid={`mailbox-provider-stack-${participant.id}`}
                        className="block truncate text-[9px] text-muted-foreground/90"
                        title={providerStackLine(
                          participant.member.provider,
                          participant.member.provider_profile?.execution_mode ?? participant.member.native_session?.execution_mode,
                          memberModelLabel(participant.member),
                        )}
                      >
                        {providerStackLine(
                          participant.member.provider,
                          participant.member.provider_profile?.execution_mode ?? participant.member.native_session?.execution_mode,
                          memberModelLabel(participant.member),
                        )}
                      </span>
                    )}
                  </span>
                </button>
                {participant.member && (
                  <button
                    type="button"
                    data-testid={`mailbox-open-${participant.id}`}
                    onClick={() => onOpenMember(participant.member!)}
                    className="grid size-11 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground sm:size-8"
                    aria-label={`Open ${participant.name}`}
                  >
                    <SquareArrowOutUpRight className="size-3.5" />
                  </button>
                )}
              </div>
              <div className="mt-1 grid grid-cols-2 divide-x divide-border/70 rounded-md border border-border/60 bg-background/60">
                <button type="button" onClick={() => onSelect(participant.id)} className="px-2 py-1 text-left hover:bg-accent/50">
                  <span className="flex items-center gap-1 text-[9px] uppercase tracking-wider text-muted-foreground"><Mail className="size-3" /> Inbox</span>
                  <span className="mt-0.5 flex items-end gap-1.5"><strong className="text-base leading-none">{inbox.length}</strong>{awaiting > 0 && <span className="text-[8px] text-status-warn">{awaiting} needs attention</span>}</span>
                </button>
                <button type="button" onClick={() => onSelect(participant.id)} className="px-2 py-1 text-left hover:bg-accent/50">
                  <span className="flex items-center gap-1 text-[9px] uppercase tracking-wider text-muted-foreground"><SendHorizontal className="size-3" /> Outbox</span>
                  <span className="mt-0.5 flex items-end gap-1.5"><strong className="text-base leading-none">{outbox.length}</strong><span className="text-[8px] text-muted-foreground">sent</span></span>
                </button>
              </div>
              {participant.member && participant.id === selectedMemberId && <span className="absolute right-2 top-2 size-1.5 rounded-full bg-primary" />}
            </article>
          );
        })}
      </div>
      {participants.length > 2 && (
        <button type="button" className="mt-2 text-[10px] font-medium text-primary sm:hidden" onClick={onToggleAllMembers}>
          {showAllMembers ? "Show priority mailboxes" : `Show all ${participants.length} mailboxes`}
        </button>
      )}
    </section>
  );
}

export function LeadInbox({
  messages,
  members,
  actionsEnabled,
  onAnswer,
  onAcknowledge,
}: {
  messages: TeamMessage[];
  members: Map<string, MemberRun>;
  actionsEnabled: boolean;
  onAnswer: (message: TeamMessage) => void;
  onAcknowledge: (message: TeamMessage) => void;
}) {
  return (
    <section aria-label="Lead Inbox" className="py-2">
      <p className="mb-2 text-[10px] text-muted-foreground">Every Member message addressed to the Host, preserving its conversation thread and delivery state.</p>
      {messages.length === 0 ? (
        <p className="rounded-lg border border-dashed border-border px-3 py-3 text-[11px] text-muted-foreground">Inbox is clear. Provider pauses remain in PendingInteraction, not this coordination queue.</p>
      ) : (
        <div className="divide-y divide-border/60 overflow-hidden rounded-lg border border-border/70 bg-card">
          {messages.slice(0, 8).map((message) => {
            const delivery = hostDelivery(message);
            const canAnswer = Boolean(message.correlation_id && message.from_member_id && message.from_member_id !== "host");
            return (
              <article key={message.id} className="grid gap-2 px-3 py-2.5 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <Badge tone={messageTone(message.kind)}>{message.kind ?? "message"}</Badge>
                    <span className="text-[11px] font-semibold text-foreground">{teamMessageActorLabel(message, members)}</span>
                    <span className="text-[10px] text-muted-foreground">{formatTime(message.created_at)}</span>
                  </div>
                  <p className="mt-1 line-clamp-2 text-[11px] leading-relaxed text-foreground/85">{message.body || "No message body"}</p>
                  <div className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-[9px] uppercase tracking-wider text-muted-foreground">
                    <span>correlation · {message.correlation_id ? shortId(message.correlation_id) : "missing"}</span>
                    <span>caused by · {message.causation_id ? shortId(message.causation_id) : "root message"}</span>
                    <span>delivery · {delivery?.policy ?? "unknown"} / {delivery?.status ?? "unknown"}</span>
                    {effectiveTeamMessageResponseIntent(message) === "response_required" ? (
                      <span className="text-status-warn">intent · response required</span>
                    ) : (
                      <span>intent · informational</span>
                    )}
                    {delivery?.provider_receipt_id && <span className="text-status-good">provider receipt · {shortId(delivery.provider_receipt_id)}</span>}
                    {delivery?.status === "acknowledged" && <span className="text-status-good">ACK</span>}
                  </div>
                </div>
                <div className="flex items-center justify-end gap-1.5">
                  {delivery?.status === "delivered" && (
                    <Button size="sm" variant="secondary" className="min-h-11 sm:min-h-0" disabled={!actionsEnabled} onClick={() => onAcknowledge(message)}>ACK</Button>
                  )}
                  <Button
                    size="sm"
                    className="min-h-11 sm:min-h-0"
                    disabled={!actionsEnabled || !canAnswer}
                    title={canAnswer ? "Reply in this correlated conversation" : "Cannot answer until this message has a conversation correlation"}
                    onClick={() => onAnswer(message)}
                  >
                    <MessageSquare className="size-3.5" /> Answer
                  </Button>
                </div>
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
