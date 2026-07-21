import type { ReactNode } from "react";
import {
  ArrowRightLeft,
  Bot,
  CheckCircle2,
  FileCheck2,
  MessageSquare,
  SendHorizontal,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  TerminalSquare,
  Wrench,
} from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { StatusDot, type StatusTone } from "@/components/workbench/atoms";

export type WorkbenchActivityKind =
  | "message"
  | "action"
  | "evidence"
  | "decision"
  | "blocker"
  | "delegation"
  | "thinking";

export type WorkbenchActivityGlyph =
  | "assignment"
  | "handoff"
  | "runtime"
  | "artifact"
  | "review"
  | "decision"
  | "message";

export interface WorkbenchActivityItem {
  id: string;
  kind: WorkbenchActivityKind;
  title: ReactNode;
  body?: ReactNode;
  actor?: ReactNode;
  timestamp?: ReactNode;
  tone?: StatusTone;
  evidenceRefs?: string[];
  /** Volatile UI state only. Never pass persisted provider thinking here. */
  transient?: boolean;
  action?: ReactNode;
  /** Optional semantic glyph; the durable record kind remains authoritative. */
  glyph?: WorkbenchActivityGlyph;
  /** Controls only the default visual projection; every record remains available. */
  prominence?: "primary" | "detail" | "pressure";
}

/**
 * A single chronological event language for chat, runtime actions, evidence,
 * and decisions. It avoids a separate tab per record type while preserving
 * each record's provenance with labels and semantic tones.
 */
export function ActivityStream({
  items,
  empty,
  className,
  variant = "rows",
}: {
  items: WorkbenchActivityItem[];
  empty?: ReactNode;
  className?: string;
  variant?: "rows" | "spine" | "timeline";
}) {
  if (items.length === 0) {
    return (
      <div className={cn("grid min-h-48 place-items-center px-6 py-10 text-center", className)}>
        {empty ?? <p className="text-sm text-muted-foreground">No activity yet.</p>}
      </div>
    );
  }

  return (
    <ol className={cn(
      variant === "spine" && "activity-spine",
      variant === "timeline" && "activity-timeline",
      variant === "rows" && "divide-y divide-border/60",
      className,
    )}>
      {items.map((item) => (
        <li key={item.id}>
          <ActivityRow item={item} variant={variant} />
        </li>
      ))}
    </ol>
  );
}

export function ActivityRow({ item, className, variant = "rows" }: { item: WorkbenchActivityItem; className?: string; variant?: "rows" | "spine" | "timeline" }) {
  const Icon = activityIcon(item.kind, item.glyph);
  const tone = item.tone ?? activityTone(item.kind);
  if (variant === "timeline") {
    return (
      <article
        className={cn(
          "activity-timeline-row relative grid min-w-0 grid-cols-[2.25rem_minmax(0,1fr)] gap-x-3 py-2.5 sm:grid-cols-[2.25rem_5rem_minmax(0,1fr)_auto]",
          item.transient && "bg-status-info/5",
          className,
        )}
      >
        <div className="hidden pt-1 text-right text-[10px] font-medium text-muted-foreground sm:col-start-2 sm:block">
          {item.timestamp}
        </div>
        <span className={cn(
          "relative z-[1] col-start-1 row-start-1 mt-0.5 grid size-8 shrink-0 place-items-center rounded-full border shadow-[0_5px_16px_-13px_currentColor]",
          activityIconSurface(tone),
        )}>
          <Icon className="size-3.5" strokeWidth={2.15} aria-hidden />
          <StatusDot
            tone={tone}
            pulse={item.transient || tone === "running"}
            className="absolute -bottom-0.5 -right-0.5 ring-2 ring-background"
          />
        </span>
        <div className="col-start-2 row-start-1 min-w-0 space-y-1 sm:col-start-3">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[10px]">
            <span className="font-semibold uppercase tracking-wider text-muted-foreground">
              {activityLabel(item.kind)}
            </span>
            {item.actor && <span className="text-muted-foreground">{item.actor}</span>}
            {item.timestamp && <span className="text-muted-foreground/80 sm:hidden">{item.timestamp}</span>}
            {item.transient && <Badge tone="info">live only</Badge>}
          </div>
          <div className="text-[12px] font-medium leading-snug text-foreground">{item.title}</div>
          {item.body && <div className="whitespace-pre-wrap text-[12px] leading-relaxed text-muted-foreground">{item.body}</div>}
          {(item.evidenceRefs?.length ?? 0) > 0 && (
            <div className="flex flex-wrap gap-1 pt-0.5">
              {item.evidenceRefs?.map((ref) => <Badge key={ref} tone="muted">{ref}</Badge>)}
            </div>
          )}
        </div>
        {item.action && (
          <div className="col-start-2 mt-2 self-center sm:col-start-4 sm:row-start-1 sm:ml-4 sm:mt-0">
            {item.action}
          </div>
        )}
      </article>
    );
  }
  return (
    <article
      className={cn(
        "group flex min-w-0 gap-3 px-4 py-3 sm:px-5",
        variant === "spine" && "activity-spine-row relative border-0 py-2.5",
        item.transient && "bg-status-info/5",
        className,
      )}
    >
      <span className={cn(
        "relative mt-0.5 grid size-8 shrink-0 place-items-center border shadow-[0_5px_16px_-13px_currentColor]",
        activityIconSurface(tone),
        variant === "spine" ? "z-[1] rounded-full" : "rounded-lg",
      )}>
        <Icon className="size-3.5" strokeWidth={2.15} aria-hidden />
        <StatusDot
          tone={tone}
          pulse={item.transient || tone === "running"}
          className="absolute -bottom-0.5 -right-0.5 ring-2 ring-background"
        />
      </span>
      <div className="min-w-0 flex-1 space-y-1">
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-[11px]">
          <span className="font-semibold uppercase tracking-wider text-muted-foreground">
            {activityLabel(item.kind)}
          </span>
          {item.actor && <span className="text-muted-foreground">{item.actor}</span>}
          {item.timestamp && <span className="text-muted-foreground/80">{item.timestamp}</span>}
          {item.transient && <Badge tone="info">live only</Badge>}
          {item.action && <span className="ml-auto shrink-0">{item.action}</span>}
        </div>
        <div className="text-[12px] font-medium leading-snug text-foreground">{item.title}</div>
        {item.body && <div className="whitespace-pre-wrap text-[12px] leading-relaxed text-muted-foreground">{item.body}</div>}
        {(item.evidenceRefs?.length ?? 0) > 0 && (
          <div className="flex flex-wrap gap-1 pt-0.5">
            {item.evidenceRefs?.map((ref) => (
              <Badge key={ref} tone="muted">{ref}</Badge>
            ))}
          </div>
        )}
      </div>
    </article>
  );
}

function activityIcon(kind: WorkbenchActivityKind, glyph?: WorkbenchActivityGlyph) {
  switch (glyph) {
    case "assignment": return SendHorizontal;
    case "handoff": return ArrowRightLeft;
    case "runtime": return TerminalSquare;
    case "artifact": return FileCheck2;
    case "review": return ShieldCheck;
    case "decision": return CheckCircle2;
    case "message": return MessageSquare;
  }
  switch (kind) {
    case "message": return MessageSquare;
    case "action": return Wrench;
    case "evidence": return FileCheck2;
    case "decision": return CheckCircle2;
    case "blocker": return ShieldAlert;
    case "delegation": return Bot;
    case "thinking": return Sparkles;
  }
}

function activityIconSurface(tone: StatusTone): string {
  switch (tone) {
    case "bad": return "border-status-bad/25 bg-status-bad/10 text-status-bad";
    case "warn": return "border-status-warn/25 bg-status-warn/10 text-status-warn";
    case "good": return "border-status-good/25 bg-status-good/10 text-status-good";
    case "decision": return "border-status-decision/25 bg-status-decision/10 text-status-decision";
    case "running":
    case "info": return "border-status-running/25 bg-status-running/10 text-status-running";
    default: return "border-border bg-card text-muted-foreground";
  }
}

function activityLabel(kind: WorkbenchActivityKind): string {
  return kind === "thinking" ? "Thinking preview" : kind;
}

function activityTone(kind: WorkbenchActivityKind): StatusTone {
  switch (kind) {
    case "action":
    case "thinking": return "running";
    case "evidence":
    case "decision": return "good";
    case "blocker": return "bad";
    case "delegation": return "decision";
    default: return "info";
  }
}
