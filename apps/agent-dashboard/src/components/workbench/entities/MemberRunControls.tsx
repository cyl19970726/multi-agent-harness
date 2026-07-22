import type { ReactNode } from "react";
import { Bot, ChevronRight, ShieldCheck, Sparkles } from "lucide-react";

import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Avatar } from "@/components/workbench/Avatar";
import { StatusDot, type StatusTone } from "@/components/workbench/atoms";
import type { AgentMember, MemberRun } from "@/types";

/**
 * A MemberRun is a one-attempt participation record within a TeamRun. It is
 * intentionally not rendered by StandingAgent controls: the cards below show
 * run-scoped assignment, current action, and ephemeral live state only.
 */
export function MemberRunMicro({ member, className }: { member: MemberRun; className?: string }) {
  const tone = memberRunTone(member.status);
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground", className)}>
      <StatusDot tone={tone} pulse={tone === "running"} />
      <span className="min-w-0 truncate text-foreground">{member.name ?? member.id}</span>
      {member.role && <span className="truncate">· {member.role}</span>}
    </span>
  );
}

export function MemberRunCompact({
  member,
  assignment,
  currentAction,
  thinkingPreview,
  onOpen,
  className,
}: {
  member: MemberRun;
  assignment?: string | null;
  currentAction?: string | null;
  /** Volatile display-only thinking. It must never be read from durable state. */
  thinkingPreview?: string | null;
  onOpen?: () => void;
  className?: string;
}) {
  const tone = memberRunTone(member.status);
  const content = (
    <>
      <div className="flex min-w-0 items-start gap-2.5">
        <Avatar name={member.name ?? member.id} tone={tone} />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-1.5">
            <span className="truncate text-[13px] font-semibold text-foreground">{member.name ?? member.id}</span>
            <Badge tone={tone}>{member.status ?? "idle"}</Badge>
          </div>
          <p className="mt-0.5 truncate text-[11px] text-muted-foreground">{member.role ?? "member"} · {member.provider ?? "provider"}{member.model ? ` · ${member.model}` : ""}</p>
        </div>
        {onOpen && <ChevronRight className="mt-1 size-4 shrink-0 text-muted-foreground" />}
      </div>
      {(assignment || currentAction || thinkingPreview) && (
        <div className="mt-2.5 space-y-1.5 border-t border-border/60 pt-2.5 text-[11px]">
          {assignment && <MemberLine icon={<ShieldCheck className="size-3" />} label="Assignment" value={assignment} />}
          {currentAction && <MemberLine icon={<Bot className="size-3" />} label="Now" value={currentAction} live />}
          {thinkingPreview && <MemberLine icon={<Sparkles className="size-3" />} label="Thinking" value={thinkingPreview} live transient />}
        </div>
      )}
    </>
  );
  const baseClass = cn("block w-full rounded-md border border-border bg-card px-3 py-2.5 text-left transition-colors", onOpen && "hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", className);
  return onOpen ? <button type="button" onClick={onOpen} className={baseClass}>{content}</button> : <div className={baseClass}>{content}</div>;
}

export function MemberRunPanel({
  member,
  assignment,
  currentAction,
  thinkingPreview,
  outputCount,
  messageCount,
  onOpen,
  className,
}: {
  member: MemberRun;
  assignment?: string | null;
  currentAction?: string | null;
  thinkingPreview?: string | null;
  outputCount?: number;
  messageCount?: number;
  onOpen?: () => void;
  className?: string;
}) {
  const tone = memberRunTone(member.status);
  return (
    <section className={cn("rounded-lg border border-border bg-card p-3.5", className)}>
      <div className="flex min-w-0 items-start gap-3">
        <Avatar name={member.name ?? member.id} tone={tone} size="lg" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5"><span className="text-[14px] font-semibold text-foreground">{member.name ?? member.id}</span><Badge tone={tone}>{member.status ?? "idle"}</Badge></div>
          <p className="mt-0.5 text-[11px] text-muted-foreground">{member.role ?? "member"} · {member.provider ?? "provider"}{member.model ? ` · ${member.model}` : ""}</p>
        </div>
      </div>
      <dl className="mt-3 space-y-2 border-t border-border/60 pt-3 text-[11px]">
        <MemberFact label="Assignment" value={assignment ?? "No assignment recorded"} />
        {currentAction && <MemberFact label="Current action" value={currentAction} live />}
        {thinkingPreview && <MemberFact label="Thinking preview" value={thinkingPreview} live transient />}
        <MemberFact label="Native session" value={member.native_session?.native_session_id ?? "No session recorded"} mono />
        <MemberFact label="Worktree override" value={member.worktree_ref ?? "None"} mono />
        <MemberFact label="Actual cwd" value={member.workspace_snapshot?.cwd ?? "Not captured (legacy run)"} mono />
      </dl>
      <div className="mt-3 flex flex-wrap gap-1.5">
        {outputCount !== undefined && <Badge tone="muted">{outputCount} outputs</Badge>}
        {messageCount !== undefined && <Badge tone="muted">{messageCount} messages</Badge>}
        {(member.owned_paths?.length ?? 0) > 0 && <Badge tone="muted">{member.owned_paths?.length} owned paths</Badge>}
      </div>
      {onOpen && <button type="button" onClick={onOpen} className="mt-3 inline-flex items-center gap-1 text-[11px] font-medium text-primary hover:underline">Open member <ChevronRight className="size-3.5" /></button>}
    </section>
  );
}

/**
 * StandingAgent is a durable capability/runtime identity. Its controls share
 * visual vocabulary with MemberRun, but intentionally omit Wave assignment,
 * worktree ownership, and one-attempt output claims.
 */
export function StandingAgentMicro({ agent, className }: { agent: AgentMember; className?: string }) {
  const tone = standingAgentTone(agent.status ?? agent.runtime_status);
  return <span className={cn("inline-flex min-w-0 items-center gap-1.5 text-[11px] text-muted-foreground", className)}><StatusDot tone={tone} pulse={tone === "running"} /><span className="min-w-0 truncate text-foreground">{agent.name ?? agent.id}</span></span>;
}

export function StandingAgentCompact({
  agent,
  activeAssignmentCount,
  onOpen,
  className,
}: {
  agent: AgentMember;
  activeAssignmentCount?: number;
  onOpen?: () => void;
  className?: string;
}) {
  const tone = standingAgentTone(agent.status ?? agent.runtime_status);
  const content = (
    <>
      <div className="flex min-w-0 items-start gap-2.5">
        <Avatar name={agent.name ?? agent.id} tone={tone} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5"><span className="truncate text-[13px] font-semibold text-foreground">{agent.name ?? agent.id}</span><Badge tone={tone}>{agent.runtime_status ?? agent.status ?? "unknown"}</Badge></div>
          <p className="mt-0.5 truncate text-[11px] text-muted-foreground">{agent.role ?? "agent"} · {agent.provider ?? "provider"}{agent.model ? ` · ${agent.model}` : ""}</p>
        </div>
        {onOpen && <ChevronRight className="mt-1 size-4 shrink-0 text-muted-foreground" />}
      </div>
      <div className="mt-2.5 flex flex-wrap gap-1.5 border-t border-border/60 pt-2"><Badge tone="muted">standing agent</Badge>{activeAssignmentCount !== undefined && <Badge tone="muted">{activeAssignmentCount} active assignments</Badge>}</div>
    </>
  );
  const baseClass = cn("block w-full rounded-md border border-border bg-card px-3 py-2.5 text-left transition-colors", onOpen && "hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", className);
  return onOpen ? <button type="button" onClick={onOpen} className={baseClass}>{content}</button> : <div className={baseClass}>{content}</div>;
}

function MemberLine({ icon, label, value, live = false, transient = false }: { icon: ReactNode; label: string; value: string; live?: boolean; transient?: boolean }) {
  return <div className="flex min-w-0 items-start gap-1.5"><span className={cn("mt-0.5 shrink-0 text-muted-foreground", live && "text-status-running")}>{icon}</span><span className="shrink-0 text-muted-foreground">{label}</span><span className="min-w-0 flex-1 truncate text-foreground">{value}</span>{transient && <span className="shrink-0 text-[9px] font-semibold uppercase tracking-wide text-status-info">live</span>}</div>;
}

function MemberFact({ label, value, live = false, transient = false, mono = false }: { label: string; value: string; live?: boolean; transient?: boolean; mono?: boolean }) {
  return <div className="grid grid-cols-[5.5rem_1fr] gap-2"><dt className={cn("text-muted-foreground", live && "text-status-running")}>{label}</dt><dd className={cn("min-w-0 break-words text-foreground", mono && "font-mono text-[10px]")}>{value}{transient && <span className="ml-1 text-[9px] font-semibold uppercase tracking-wide text-status-info">live only</span>}</dd></div>;
}

function memberRunTone(status?: string | null): StatusTone {
  if (status === "completed") return "good";
  if (status === "blocked" || status === "failed" || status === "stopped") return "bad";
  if (status === "waiting" || status === "reviewing") return "warn";
  if (status === "running") return "running";
  if (status === "queued" || status === "starting") return "info";
  return "idle";
}

function standingAgentTone(status?: string | null): StatusTone {
  if (status === "running" || status === "active") return "running";
  if (status === "failed" || status === "stale" || status === "blocked") return "bad";
  if (status === "ready" || status === "available") return "good";
  if (status === "idle") return "idle";
  return "info";
}
