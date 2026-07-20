import type { ReactNode } from "react";
import { ArrowUpRight, CheckCircle2, CircleAlert, FileText, ShieldCheck, UserRound } from "lucide-react";

import { cn } from "@/lib/utils";
import { ActorAvatar, ArtField, EditorialTitle } from "../visuals";

import type { ActorSummary, FinancialRecordView, PageFrameProps } from "./types";

/**
 * The application router owns the URL.  It wraps the mounted surface with this
 * stable visual-contract marker so screenshot capture never mistakes an old
 * workbench route for a Company OS page.
 */
export function CompanyOsPageRoot({
  page,
  children,
}: {
  page:
    | "home"
    | "workboard"
    | "work-item-focus"
    | "finance"
    | "agents-organization"
    | "standing-agent-focus"
    | "governance-proposal"
    | "approval-focus"
    | "human-member-focus";
  children: ReactNode;
}) {
  return <div data-company-os-page={page} data-company-os-fixture="company-os-trademark-v1" data-company-os-ready="true">{children}</div>;
}

const actorLabels = {
  human: "Human",
  standing_agent: "Standing Agent",
  external: "External",
  service: "Service",
} as const;

const actorDataKinds = {
  human: "Human",
  standing_agent: "Standing Agent",
  external: "External",
  service: "Service",
} as const;

export function ActorPill({ actor, compact = false }: { actor: ActorSummary; compact?: boolean }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-2 text-sm text-foreground" data-company-os-ref={actor.id} data-actor-kind={actorDataKinds[actor.kind]} data-actor-type={actorDataKinds[actor.kind]}>
      <ActorAvatar identity={`${actor.id} ${actor.role}`} name={actor.name} size={compact ? "sm" : "md"} ring={actor.kind === "human" ? "warm" : actor.kind === "external" ? "external" : actor.availability === "available" ? "good" : "neutral"} />
      <span className="min-w-0">
        <span className="block truncate font-medium">{actor.name}</span>
        {!compact && <span className="block truncate text-xs text-muted-foreground">{actorLabels[actor.kind]} · {actor.role}</span>}
      </span>
      {actor.availability === "available" && <span className="size-2 shrink-0 rounded-full bg-status-good" title="Explicitly reported available" aria-label="Explicitly reported available" />}
    </span>
  );
}

export function StatusTag({ status }: { status: string }) {
  const copy: Record<string, string> = {
    waiting_for_approval: "Waiting for approval",
    pending_approval: "Pending approval",
    requested: "Decision requested",
    proposed: "Proposed",
    awaiting_final_approval: "Awaiting final approval",
    available: "Reported available",
  };
  const alert = ["waiting_for_approval", "pending_approval", "requested", "proposed", "awaiting_final_approval"].includes(status);
  return <span className={cn("inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] font-medium", alert ? "border-primary/30 bg-primary/10 text-primary" : "border-status-good/30 bg-status-good/10 text-status-good")}><span className="size-1.5 rounded-full bg-current" />{copy[status] ?? status}</span>;
}

export function PageFrame({ eyebrow, title, description, action, children, context, dense = false }: PageFrameProps) {
  return (
    <div className="company-workbench h-full overflow-y-auto bg-background">
      <ArtField />
      <div className={cn("relative mx-auto grid w-full max-w-[1480px] grid-cols-1 gap-7 px-5 lg:grid-cols-[minmax(0,1fr)_310px] lg:px-9", dense ? "py-5" : "py-8")}>
        <main className="min-w-0">
          <header className={cn("flex flex-wrap items-start justify-between gap-4 border-b border-border/80", dense ? "mb-4 pb-4" : "mb-7 pb-6")}>
            <div>
              <p className="mb-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-primary">{eyebrow}</p>
              <EditorialTitle className={dense ? "text-4xl sm:text-5xl" : undefined}>{title}</EditorialTitle>
              {description && <p className="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">{description}</p>}
            </div>
            {action}
          </header>
          {children}
        </main>
        {context && <aside className="self-start border-l border-border pl-0 lg:sticky lg:top-0 lg:pl-6">{context}</aside>}
      </div>
    </div>
  );
}

export function Panel({ title, children, action, className }: { title: string; children: ReactNode; action?: ReactNode; className?: string }) {
  return <section className={cn("overflow-hidden rounded-lg border border-border bg-card", className)}><header className="flex items-center justify-between gap-3 border-b border-border px-4 py-3"><h2 className="text-sm font-semibold">{title}</h2>{action}</header><div className="p-4">{children}</div></section>;
}

export function ContextRail({ label = "Context", children }: { label?: string; children: ReactNode }) {
  return <div className="space-y-5"><p className="text-[11px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">{label}</p>{children}</div>;
}

export function LinkedRecord({ label, detail, recordRef, wrapLabel = false, icon = <FileText className="size-4" /> }: { label: string; detail?: string; recordRef?: string; wrapLabel?: boolean; icon?: ReactNode }) {
  return <button type="button" data-company-os-ref={recordRef} className="flex w-full items-start gap-2 rounded-md p-2 text-left transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"><span className="mt-0.5 text-muted-foreground">{icon}</span><span className="min-w-0 flex-1"><span className={cn("block text-sm font-medium", wrapLabel ? "whitespace-normal leading-5" : "truncate")}>{label}</span>{detail && <span className="block text-xs leading-5 text-muted-foreground">{detail}</span>}</span><ArrowUpRight className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" /></button>;
}

export function FinancialRecordCard({ record }: { record: FinancialRecordView }) {
  return <div className="rounded-md border border-primary/25 bg-primary/[0.045] p-3" data-company-os-ref={record.id} data-financial-record-type={record.type} data-financial-type={record.type} data-financial-status={record.status}><div className="flex items-start justify-between gap-3"><div><p className="text-sm font-medium">{record.label}</p><p className="mt-1 text-xs text-muted-foreground">Commitment · {record.status === "pending_approval" ? "pending approval" : record.status}</p></div><strong className="text-lg font-semibold text-foreground">{record.amount}</strong></div><p className="mt-3 border-t border-primary/15 pt-3 text-xs leading-5 text-muted-foreground">This is a pre-approval commitment, not a payment.</p></div>;
}

export function DecisionNotice({ children }: { children: ReactNode }) {
  return <div className="flex gap-3 rounded-lg border border-primary/25 bg-primary/[0.055] p-3 text-sm leading-5 text-foreground"><CircleAlert className="mt-0.5 size-4 shrink-0 text-primary" /><div>{children}</div></div>;
}

export function RoleLine({ label, actor }: { label: string; actor?: ActorSummary }) {
  if (!actor) return null;
  return <div className="grid grid-cols-[8.5rem_minmax(0,1fr)] gap-3 py-2 text-sm"><span className="text-muted-foreground">{label}</span><ActorPill actor={actor} compact /></div>;
}

export function PolicyNote({ children }: { children: ReactNode }) {
  return <div className="flex gap-2 border-l-2 border-primary pl-3 text-xs leading-5 text-muted-foreground"><ShieldCheck className="mt-0.5 size-4 shrink-0 text-primary" />{children}</div>;
}

export function EmptyRuntimeBoundary() {
  return <div className="flex items-start gap-2 rounded-md bg-muted/65 p-3 text-xs leading-5 text-muted-foreground"><UserRound className="mt-0.5 size-4 shrink-0" />Organization identity and responsibility are shown here. Provider sessions and MemberRuns remain execution history, not membership.</div>;
}

export function CompleteMark({ label }: { label: string }) {
  return <span className="inline-flex items-center gap-1.5 text-xs text-status-good"><CheckCircle2 className="size-3.5" />{label}</span>;
}

/** Honest action affordance for read-only projections with no command transport. */
export function GovernedActionButton({ label, reason }: { label: string; reason: string }) {
  return <button type="button" disabled title={reason} aria-label={`${label}. Unavailable: ${reason}`} className="inline-flex min-h-10 cursor-not-allowed items-center justify-center rounded-md border border-border bg-muted px-3 py-2 text-sm font-medium text-muted-foreground">{label}</button>;
}
