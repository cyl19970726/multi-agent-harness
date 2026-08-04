import type { ReactNode } from "react";
import {
  Activity,
  ArrowRight,
  BadgeCheck,
  BrainCircuit,
  CheckCircle2,
  CircleCheckBig,
  CircleHelp,
  FileCheck2,
  Handshake,
  ListTodo,
  Megaphone,
  MessageCircleWarning,
  MessageSquare,
  MessageSquareReply,
  OctagonAlert,
  ScanSearch,
  ShieldAlert,
  Wrench,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import { Markdown } from "@/components/workbench/Markdown";
import type { StatusTone } from "@/components/workbench/atoms";
import type { WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";

import type { TeamActivityItem } from "./teamActivityItems";
import { formatAbsolute, isoTime, messageTone, shortId } from "./teamFormat";

/**
 * One source-aware Team conversation. Every row keeps its durable identity
 * route (sender → full recipient list), its source/status, and an absolute
 * time on disclosure; nothing here rewrites the underlying record.
 */
export function TeamConversationStream({ items, empty, onOpenWork }: { items: TeamActivityItem[]; empty: ReactNode; onOpenWork: (workId: string) => void }) {
  if (!items.length) return <div className="grid min-h-48 place-items-center px-6 py-10 text-center">{empty}</div>;
  return (
    <ol data-testid="team-conversation-list" className="relative py-1 before:absolute before:bottom-5 before:left-[1.05rem] before:top-5 before:w-px before:bg-border/80">
      {items.map((item) => <li key={item.id} data-conversation-row="true"><TeamConversationRow item={item} onOpenWork={onOpenWork} /></li>)}
    </ol>
  );
}

function TeamConversationRow({ item, onOpenWork }: { item: TeamActivityItem; onOpenWork: (workId: string) => void }) {
  const kind = item.messageKind ?? item.kind;
  const tone = item.tone ?? "idle";
  const plan = kind === "plan_proposal";
  const pressure = ["blocker", "review_request", "plan_feedback"].includes(kind);
  const accepted = ["plan_approval", "review_result"].includes(kind);
  const handoff = kind === "handoff";
  const execution = item.kind === "action" || item.kind === "evidence";
  const absolute = formatAbsolute(item.occurredAt);
  return (
    <article className="relative grid grid-cols-[2.25rem_4.25rem_minmax(0,1fr)] gap-x-2.5 py-1.5">
      <ConversationNode kind={kind} tone={tone} avatarName={item.actorAvatarName} avatarTone={item.actorTone} onActorClick={item.onActorClick} />
      <time
        dateTime={isoTime(item.occurredAt)}
        title={absolute}
        className="pt-1 text-right text-[10px] font-medium text-muted-foreground"
      >
        {item.timestamp}
        <span className="sr-only"> ({absolute})</span>
      </time>
      <div className="min-w-0">
        <ConversationMeta item={item} label={conversationLabel(kind)} />
        <div className={cn(
          "mt-1 overflow-hidden rounded-lg border bg-card/75 shadow-[0_18px_42px_-38px_rgba(15,23,42,.75)]",
          plan && "border-[#8b5cf6]/30 bg-[linear-gradient(145deg,hsl(var(--card)),rgba(139,92,246,.035))]",
          pressure && "border-status-warn/35 bg-[linear-gradient(145deg,hsl(var(--card)),hsl(var(--status-warn)/.045))]",
          accepted && "border-status-good/30 bg-[linear-gradient(145deg,hsl(var(--card)),hsl(var(--status-good)/.04))]",
          handoff && "border-primary/30 bg-[linear-gradient(145deg,hsl(var(--card)),hsl(var(--primary)/.035))]",
          execution && "border-status-info/25",
        )}>
          <div className="flex min-w-0 items-center gap-2 border-b border-border/55 px-2.5 py-1.5">
            <div className="min-w-0 flex-1">
              {item.messageKind ? <ConversationRoute item={item} /> : <div className="text-[11px] font-semibold text-foreground">{item.title}</div>}
            </div>
            {item.workId && <button type="button" onClick={() => onOpenWork(item.workId!)} className="shrink-0 rounded-full border border-primary/20 bg-primary/[0.055] px-2 py-1 font-mono text-[9px] text-primary hover:bg-primary/10">Work · {shortId(item.workId)}</button>}
            {item.action && <div className="shrink-0">{item.action}</div>}
          </div>
          <div className="px-2.5 py-2">
            {plan && item.bodySource
              ? <PlanProposalBody source={item.bodySource} />
              : <div className="text-[11px] leading-relaxed text-foreground/85">{item.body}</div>}
            {(item.evidenceRefs?.length ?? 0) > 0 && (
              <div className="mt-2 grid gap-1 border-t border-border/55 pt-2 sm:grid-cols-2">
                {item.evidenceRefs?.map((ref) => (
                  <span key={ref} className="inline-flex min-w-0 items-center gap-1.5 rounded-md bg-muted/45 px-2 py-1 text-[9px] text-muted-foreground">
                    <FileCheck2 className="size-3 shrink-0 text-status-good" />
                    <span className="truncate">{ref}</span>
                  </span>
                ))}
              </div>
            )}
          </div>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-t border-border/50 bg-muted/20 px-2.5 py-1 text-[9px] text-muted-foreground">
            {item.actor && <span>{item.actor}</span>}
            {item.source === "provider-native" && <span>native session</span>}
            {item.statusLabel && <span className={accepted ? "text-status-good" : undefined}>{item.statusLabel}</span>}
            <span className="ml-auto">{item.kind === "message" || item.kind === "blocker" || item.kind === "decision" ? "coordination record" : "Harness evidence"}</span>
          </div>
        </div>
      </div>
    </article>
  );
}

function ConversationMeta({ item, label }: { item: WorkbenchActivityItem; label: string }) {
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[9px] uppercase tracking-[0.11em] text-muted-foreground">
      <span className="font-semibold text-foreground/75">{label}</span>
      {item.actorLabel && <span className="normal-case tracking-normal">{item.actorLabel}</span>}
    </div>
  );
}

function ConversationRoute({ item }: { item: WorkbenchActivityItem }) {
  const presentation = messagePresentation(item.messageKind);
  const TypeIcon = presentation.icon;
  const recipients = item.recipientLabels ?? [];
  const overflow = recipients.slice(3);
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5">
      <span className="inline-flex items-center gap-1 text-[11px] font-semibold text-foreground">
        <span className={cn("size-1.5 rounded-full", presentation.dotClass)} />
        {item.actorLabel ?? "Host"}
      </span>
      <ArrowRight className="size-3 shrink-0 text-muted-foreground" />
      <span className="flex min-w-0 items-center gap-1">
        {recipients.slice(0, 3).map((recipient) => (
          <span key={recipient} className="inline-flex items-center gap-1 rounded-full bg-muted/45 py-0.5 pl-0.5 pr-1.5">
            <Avatar name={recipient} tone="idle" size="xs" />
            <span className="max-w-28 truncate text-[10px] font-medium text-foreground/80">{recipient}</span>
          </span>
        ))}
        {overflow.length > 0 && (
          // The visible `+N` stays compact, but the complete recipient route
          // must remain available to assistive technology rather than being
          // collapsed into an opaque count.
          <span className="text-[9px] text-muted-foreground" title={overflow.join(", ")}>
            +{overflow.length}
            <span className="sr-only"> more recipients: {overflow.join(", ")}</span>
          </span>
        )}
      </span>
      <Badge tone={messageTone(item.messageKind)}>
        <TypeIcon className="mr-1 size-3" />
        {presentation.label}
      </Badge>
    </div>
  );
}

function ConversationNode({ kind, tone, avatarName, avatarTone, onActorClick }: {
  kind: string;
  tone: StatusTone;
  avatarName?: string;
  avatarTone?: StatusTone;
  onActorClick?: () => void;
}) {
  if (avatarName) {
    // The timeline identity is always the sender portrait; the corner mark
    // communicates message type without replacing or obscuring authorship.
    const presentation = messagePresentation(kind);
    const TypeIcon = presentation.icon;
    return (
      <button type="button" disabled={!onActorClick} onClick={onActorClick} className="relative z-[1] rounded-full ring-4 ring-background focus-visible:outline-none focus-visible:ring-primary">
        <Avatar name={avatarName} tone={avatarTone ?? tone} />
        <span className={cn(
          "absolute -bottom-1 -right-1 grid size-4 place-items-center rounded-full border border-background text-white shadow-sm",
          presentation.iconClass,
        )}>
          <TypeIcon className="size-2.5" strokeWidth={2.4} />
        </span>
      </button>
    );
  }
  const Icon = kind === "plan_proposal" ? BrainCircuit
      : kind === "plan_feedback" || kind === "blocker" ? ShieldAlert
        : kind === "plan_approval" || kind === "review_result" ? CheckCircle2
          : kind === "handoff" ? ArrowRight
            : kind === "evidence" ? FileCheck2
              : kind === "action" ? Wrench
                : MessageSquare;
  return (
    <span className={cn(
      "relative z-[1] grid size-8 place-items-center rounded-xl border ring-4 ring-background",
      tone === "bad" && "border-status-bad/25 bg-status-bad/8 text-status-bad",
      tone === "warn" && "border-status-warn/25 bg-status-warn/8 text-status-warn",
      tone === "good" && "border-status-good/25 bg-status-good/8 text-status-good",
      tone === "decision" && "border-[#8b5cf6]/25 bg-[#8b5cf6]/[0.08] text-[#7653c6]",
      ["info", "running"].includes(tone) && "border-status-info/25 bg-status-info/8 text-status-info",
      tone === "idle" && "border-border bg-card text-muted-foreground",
    )}>
      <Icon className="size-3.5" />
    </span>
  );
}

function PlanProposalBody({ source }: { source: string }) {
  const sections = source.split(/^##\s+/m).map((value) => value.trim()).filter(Boolean).map((value) => {
    const [heading, ...lines] = value.split("\n");
    return { heading, body: lines.join("\n").trim() };
  });
  return (
    <div className="grid gap-2 sm:grid-cols-2">
      {sections.map((section) => (
        <section key={section.heading} className="rounded-lg border border-[#8b5cf6]/15 bg-background/75 px-2.5 py-2">
          <h3 className="mb-1 text-[9px] font-semibold uppercase tracking-[0.12em] text-[#7653c6]">{section.heading}</h3>
          <Markdown source={section.body} compact />
        </section>
      ))}
    </div>
  );
}

export function conversationLabel(kind: string): string {
  if (kind === "plan_request") return "Plan request";
  if (kind === "plan_proposal") return "Plan proposal";
  if (kind === "plan_feedback") return "Host challenge";
  if (kind === "plan_approval") return "Plan approved";
  if (kind === "review_request") return "Lead inbox";
  if (kind === "review_result") return "Review decision";
  if (kind === "handoff") return "Handoff";
  if (kind === "blocker") return "Blocker";
  if (kind === "evidence") return "Evidence";
  if (kind === "action") return "Execution";
  return kind.replace(/_/g, " ");
}

export function messagePresentation(kind?: string | null): {
  label: string;
  icon: typeof MessageSquare;
  iconClass: string;
  dotClass: string;
} {
  const normalized = kind ?? "message";
  if (normalized === "message") return { label: "Message", icon: MessageSquare, iconClass: "bg-[#64748b]", dotClass: "bg-[#64748b]" };
  if (normalized === "broadcast") return { label: "Broadcast", icon: Megaphone, iconClass: "bg-status-info", dotClass: "bg-status-info" };
  if (normalized === "question") return { label: "Question", icon: CircleHelp, iconClass: "bg-[#7c5bd6]", dotClass: "bg-[#7c5bd6]" };
  if (normalized === "answer") return { label: "Answer", icon: MessageSquareReply, iconClass: "bg-status-good", dotClass: "bg-status-good" };
  if (normalized === "progress") return { label: "Progress", icon: Activity, iconClass: "bg-status-info", dotClass: "bg-status-info" };
  if (normalized === "blocker") return { label: "Blocker", icon: OctagonAlert, iconClass: "bg-status-bad", dotClass: "bg-status-bad" };
  if (normalized === "review_request") return { label: "Lead inbox", icon: ScanSearch, iconClass: "bg-status-warn", dotClass: "bg-status-warn" };
  if (normalized === "review_result") return { label: "Review decision", icon: BadgeCheck, iconClass: "bg-status-good", dotClass: "bg-status-good" };
  if (normalized === "plan_request") return { label: "Plan request", icon: ListTodo, iconClass: "bg-status-info", dotClass: "bg-status-info" };
  if (normalized === "plan_proposal") return { label: "Plan proposal", icon: BrainCircuit, iconClass: "bg-[#7c5bd6]", dotClass: "bg-[#7c5bd6]" };
  if (normalized === "plan_feedback") return { label: "Host challenge", icon: MessageCircleWarning, iconClass: "bg-status-warn", dotClass: "bg-status-warn" };
  if (normalized === "plan_approval") return { label: "Plan approved", icon: CircleCheckBig, iconClass: "bg-status-good", dotClass: "bg-status-good" };
  if (normalized === "handoff") return { label: "Handoff", icon: Handshake, iconClass: "bg-[#ff725e]", dotClass: "bg-[#ff725e]" };
  if (normalized === "evidence") return { label: "Evidence", icon: FileCheck2, iconClass: "bg-status-good", dotClass: "bg-status-good" };
  if (normalized === "action") return { label: "Tool activity", icon: Wrench, iconClass: "bg-status-info", dotClass: "bg-status-info" };
  return { label: conversationLabel(normalized), icon: MessageSquare, iconClass: "bg-muted-foreground", dotClass: "bg-muted-foreground" };
}
