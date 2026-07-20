import { ChevronRight, Users } from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { StatusDot, type StatusTone } from "@/components/workbench/atoms";
import { Avatar } from "@/components/workbench/Avatar";
import type { MemberRun, TeamRun } from "@/types";

export function TeamRunMicro({ run, memberCount, className }: { run: TeamRun; memberCount?: number; className?: string }) {
  const tone = teamRunTone(run.status);
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground", className)}>
      <StatusDot tone={tone} pulse={tone === "running"} />
      <span className="min-w-0 truncate text-foreground">Agent Team</span>
      {memberCount !== undefined && <span>{memberCount} members</span>}
    </span>
  );
}

export function TeamRunCompact({
  run,
  members = [],
  needsYouCount,
  onOpen,
  className,
}: {
  run: TeamRun;
  members?: MemberRun[];
  needsYouCount?: number;
  onOpen?: () => void;
  className?: string;
}) {
  const tone = teamRunTone(run.status);
  const summary = memberSummary(members);
  const content = (
    <>
      <div className="flex min-w-0 items-start gap-2.5">
        <span className="grid size-7 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground"><Users className="size-3.5" /></span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <StatusDot tone={tone} pulse={tone === "running"} />
            <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Agent Team</span>
            <Badge tone={tone}>{run.status ?? "planning"}</Badge>
          </div>
          <p className="mt-1 line-clamp-2 text-[12px] font-semibold leading-snug text-foreground">{run.objective ?? "Team attempt"}</p>
        </div>
        {onOpen && <ChevronRight className="mt-1 size-4 shrink-0 text-muted-foreground" />}
      </div>
      <div className="mt-2.5 flex min-w-0 items-center gap-2 border-t border-border/60 pt-2">
        <div className="flex -space-x-1.5">{members.slice(0, 4).map((member) => <Avatar key={member.id} name={member.name ?? member.id} tone={memberTone(member.status)} />)}</div>
        <span className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">{summary}</span>
        {(needsYouCount ?? 0) > 0 && <Badge tone="warn">{needsYouCount} needs you</Badge>}
      </div>
    </>
  );
  const baseClass = cn("block w-full rounded-md border border-border bg-card px-3 py-2.5 text-left transition-colors", onOpen && "hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", className);
  return onOpen ? <button type="button" className={baseClass} onClick={onOpen}>{content}</button> : <div className={baseClass}>{content}</div>;
}

export function TeamRunPanel({
  run,
  members = [],
  needsYouCount = 0,
  onOpen,
  className,
}: {
  run: TeamRun;
  members?: MemberRun[];
  needsYouCount?: number;
  onOpen?: () => void;
  className?: string;
}) {
  const tone = teamRunTone(run.status);
  return (
    <section className={cn("rounded-lg border border-border bg-card p-3.5", className)}>
      <div className="flex min-w-0 items-start gap-3">
        <span className="grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground"><Users className="size-4" /></span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5"><StatusDot tone={tone} pulse={tone === "running"} /><span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Agent Team attempt</span><Badge tone={tone}>{run.status ?? "planning"}</Badge></div>
          <h3 className="mt-1 line-clamp-2 text-[14px] font-semibold leading-snug text-foreground">{run.objective ?? "Team attempt"}</h3>
        </div>
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5 border-t border-border/60 pt-3">
        <Badge tone="muted">{members.length} members</Badge>
        {needsYouCount > 0 && <Badge tone="warn">{needsYouCount} needs you</Badge>}
        {run.previous_run_id && <Badge tone="muted">retry attempt</Badge>}
      </div>
      <ul className="mt-3 space-y-1.5">
        {members.slice(0, 5).map((member) => <li key={member.id} className="flex min-w-0 items-center gap-2"><Avatar name={member.name ?? member.id} tone={memberTone(member.status)} /><span className="min-w-0 flex-1 truncate text-[12px] text-foreground">{member.name ?? member.id}</span><span className="shrink-0 text-[11px] text-muted-foreground">{member.role ?? "member"}</span></li>)}
      </ul>
      {onOpen && <button type="button" onClick={onOpen} className="mt-3 inline-flex items-center gap-1 text-[11px] font-medium text-primary hover:underline">Open war room <ChevronRight className="size-3.5" /></button>}
    </section>
  );
}

function memberSummary(members: MemberRun[]): string {
  if (members.length === 0) return "No members recorded";
  const blocked = members.filter((member) => member.status === "blocked" || member.status === "failed").length;
  const running = members.filter((member) => member.status === "running").length;
  return [running > 0 ? `${running} active` : undefined, blocked > 0 ? `${blocked} blocked` : undefined, `${members.length} members`].filter(Boolean).join(" · ");
}

function teamRunTone(status?: string | null): StatusTone {
  if (status === "completed") return "good";
  if (status === "failed" || status === "cancelled") return "bad";
  if (status === "waiting" || status === "reviewing") return "warn";
  if (status === "running") return "running";
  if (status === "planning") return "info";
  return "idle";
}

function memberTone(status?: string | null): StatusTone {
  if (status === "completed") return "good";
  if (status === "blocked" || status === "failed" || status === "stopped") return "bad";
  if (status === "waiting" || status === "reviewing") return "warn";
  if (status === "running") return "running";
  if (status === "queued" || status === "starting") return "info";
  return "idle";
}
