import type { ReactNode } from "react";
import {
  Bot,
  CheckCircle2,
  FileCheck2,
  MessageSquare,
  ShieldAlert,
  Sparkles,
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
}: {
  items: WorkbenchActivityItem[];
  empty?: ReactNode;
  className?: string;
}) {
  if (items.length === 0) {
    return (
      <div className={cn("grid min-h-48 place-items-center px-6 py-10 text-center", className)}>
        {empty ?? <p className="text-sm text-muted-foreground">No activity yet.</p>}
      </div>
    );
  }

  return (
    <ol className={cn("divide-y divide-border/60", className)}>
      {items.map((item) => (
        <li key={item.id}>
          <ActivityRow item={item} />
        </li>
      ))}
    </ol>
  );
}

export function ActivityRow({ item, className }: { item: WorkbenchActivityItem; className?: string }) {
  const Icon = activityIcon(item.kind);
  const tone = item.tone ?? activityTone(item.kind);
  return (
    <article
      className={cn(
        "group flex min-w-0 gap-3 px-4 py-3.5 sm:px-5",
        item.transient && "bg-status-info/5",
        className,
      )}
    >
      <span className="relative mt-0.5 grid size-7 shrink-0 place-items-center rounded-md border border-border bg-card">
        <Icon className="size-3.5 text-muted-foreground" aria-hidden />
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
        <div className="text-[13px] font-medium leading-snug text-foreground">{item.title}</div>
        {item.body && <div className="whitespace-pre-wrap text-[13px] leading-relaxed text-muted-foreground">{item.body}</div>}
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

function activityIcon(kind: WorkbenchActivityKind) {
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

