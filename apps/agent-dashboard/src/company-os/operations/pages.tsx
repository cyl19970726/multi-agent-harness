import { useRef, useState, type ReactNode } from "react";
import { Bot, Building2, FileText, Landmark, Network, Plus, Scale, Send, ShieldCheck, Tag, Users } from "lucide-react";

import {
  ActorPill, ContextRail, DecisionNotice,
  FinancialRecordCard, GovernedActionButton, LinkedRecord, PageFrame, Panel, PolicyNote, RoleLine, StatusTag,
} from "./components";
import { prototypeTrademarkOperationsProjection } from "./fixture";
import { buildApprovalDecisionCommand } from "./approvalAction";
import type { ActorSummary, ApprovalDecision, ApprovalDecisionCommand, TrademarkOperationsProjection, WorkItemView } from "./types";
import { ActorAvatar, ObjectEmblem } from "../visuals";

type OperationsPageProps = { data?: TrademarkOperationsProjection };
type ApprovalFocusProps = OperationsPageProps & {
  actionEnabled?: boolean;
  onDecision?: (command: ApprovalDecisionCommand, capabilityToken: string) => Promise<boolean>;
};

function projection(data?: TrademarkOperationsProjection): TrademarkOperationsProjection {
  return data ?? prototypeTrademarkOperationsProjection;
}

function actorOr(data: TrademarkOperationsProjection, id: string, fallback: ActorSummary): ActorSummary {
  return data.actors[id] ?? fallback;
}

function humanReadable(value: string, fallback: string): string {
  const raw = value.trim();
  if (!raw) return fallback;
  // Sentences are already authored business copy; only humanize machine labels.
  if (/[.!?]$/.test(raw) || raw.length > 72) return raw;
  const normalized = raw.replace(/[._-]+/g, " ").replace(/\s+/g, " ").trim();
  if (normalized.toLowerCase() === "commitment append") return "Commitment update";
  return /^[a-z][a-z ]+$/.test(normalized)
    ? normalized.replace(/\b\w/g, (letter) => letter.toUpperCase())
    : normalized;
}

function actorDescriptor(actor: ActorSummary | undefined): string {
  if (!actor) return "Unassigned";
  const kind = actor.kind === "human" ? "Human" : actor.kind === "standing_agent" ? "Standing Agent" : actor.kind === "external" ? "External" : "Service";
  return `${actor.name} · ${kind}`;
}

function actorSemanticKind(actor: ActorSummary | undefined): string | undefined {
  if (!actor) return undefined;
  return actor.kind === "human" ? "Human" : actor.kind === "standing_agent" ? "Standing Agent" : actor.kind === "external" ? "External" : "Service";
}

function displayTimestamp(value: string): string {
  const match = value.match(/^(\d{4})-(\d{2})-(\d{2})T?(\d{2}):(\d{2})/);
  if (!match) return value || "No update time recorded";
  const [, year, month, day, hour, minute] = match;
  const names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return `${day} ${names[Number(month) - 1] ?? month} ${year} · ${hour}:${minute}`;
}

const commandUnavailable = "No approved action transport is connected to this read-only projection.";

export function OrganizationPage({ data }: OperationsPageProps) {
  const view = projection(data);
  const brandUnit = view.organization.units.find((unit) => unit.id === view.organization.brandUnit.id) ?? {
    ...view.organization.brandUnit, actorIds: [],
  };
  const governanceUnit = view.organization.units.find((unit) => unit.label.toLowerCase() === "governance");
  const brandActors = membersForUnit(view, brandUnit.id);
  const governanceActors = governanceUnit ? membersForUnit(view, governanceUnit.id) : [];
  const governanceLead = governanceActors.find((actor) => actor.id === view.governanceProposal.proposedById)
    ?? governanceActors.find((actor) => actor.availability)
    ?? governanceActors[0];
  const humanOwner = view.actorList.find((actor) => actor.kind === "human") ?? brandActors.find((actor) => actor.kind === "human");
  const lead = brandActors.find((actor) => /lead/i.test(actor.name)) ?? governanceLead;
  const standingAgentRoster = view.actorList.filter((actor) => actor.kind === "standing_agent" && actor.id !== lead?.id);
  const externalActors = view.actorList.filter((actor) => actor.kind === "external");
  const secondaryUnits = view.organization.units.filter((unit) => unit.id !== view.organization.company.id && unit.id !== brandUnit.id);

  return <PageFrame dense eyebrow="Organization" title="Company OS" description="Responsibility, authority, and capability across Humans, Standing Agents, and external collaborators." action={<div className="flex flex-wrap gap-2"><button type="button" disabled title="A governed organization action requires an approved proposal." className="inline-flex min-h-10 cursor-not-allowed items-center gap-2 rounded-lg border border-primary/25 bg-primary/[0.07] px-4 py-2 text-sm font-medium text-primary"><Bot className="size-4" />Propose agent</button><button type="button" disabled title="A governed organization action requires an approved proposal." className="inline-flex min-h-10 cursor-not-allowed items-center gap-2 rounded-lg border border-border bg-card/80 px-4 py-2 text-sm font-medium text-muted-foreground"><Plus className="size-4" />Create org unit</button></div>} context={<ContextRail label="Company lead context"><PolicyNote>Organization changes are proposed and reviewed. This view cannot grant authority, legal access, or financial permissions.</PolicyNote>{humanOwner && <Panel title="Ultimate authority"><ActorPill actor={humanOwner} /></Panel>}<Panel title="Authority boundary"><div className="space-y-3 text-xs leading-5 text-muted-foreground"><p className="flex gap-2"><ShieldCheck className="mt-0.5 size-4 shrink-0 text-status-good" />Standing Agents may own and coordinate WorkItems within explicit scope.</p><p className="flex gap-2"><Scale className="mt-0.5 size-4 shrink-0 text-primary" />Financial, legal, and organization-wide changes remain Human-governed.</p></div></Panel><LinkedRecord wrapLabel recordRef={view.governanceProposal.id} label={view.governanceProposal.label} detail={view.governanceProposal.detail} icon={<Scale className="size-4" />} /></ContextRail>}>
    <section aria-label="Organization tree" className="relative overflow-hidden rounded-2xl border border-border bg-card/70 p-4 shadow-sm sm:p-6" data-company-os-ref={view.organization.company.id}>
      <div className="pointer-events-none absolute -left-24 -top-24 size-72 rounded-full border border-primary/15" /><div className="pointer-events-none absolute -left-10 -top-10 size-44 rounded-full border border-primary/20" />
      <div className="relative mx-auto max-w-5xl">
        <div className="flex items-center justify-center gap-2 text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground"><Building2 className="size-4 text-primary" />{view.organization.company.label}</div>
        {humanOwner && <OrgActorCard actor={humanOwner} variant="owner" className="mx-auto mt-5 max-w-md" />}
        <Connector />
        {lead ? <OrgActorCard actor={lead} variant="lead" className="mx-auto max-w-xl" /> : <OrganizationNode icon={<Network className="size-5" />} label={brandUnit.label} recordRef={brandUnit.id} className="mx-auto max-w-xl" />}
        <div className="mx-auto h-7 w-px bg-primary/35" aria-hidden />
        <div className="relative border-t border-primary/35 pt-7"><p className="mb-4 text-center text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Cross-unit Standing Agent capability roster</p><div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">{standingAgentRoster.map((actor) => <OrgActorCard key={actor.id} actor={actor} linkedDocument={actor.id === view.workItem.assignees[0]?.id ? view.sourceDocument : undefined} />)}</div></div>
        {externalActors.length > 0 && <div className="mt-7 flex justify-end"><div className="w-full max-w-sm border-l border-dashed border-sky-500/40 pl-5"><p className="mb-3 flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-sky-700"><Users className="size-4" />External collaboration</p>{externalActors.map((actor) => <OrgActorCard key={actor.id} actor={actor} variant="external" />)}</div></div>}
        <div className="mt-7 flex items-center justify-between gap-4 border-t border-border pt-4 text-xs text-muted-foreground"><span data-company-os-ref={brandUnit.id} className="inline-flex items-center gap-2"><Tag className="size-3.5 text-primary" />Primary operating unit · {brandUnit.label}</span><span>{standingAgentRoster.length} cross-unit Standing Agent roles shown · not a reporting relation</span></div>
        {secondaryUnits.length > 0 && <details className="mt-4 rounded-lg border border-border bg-background/50 p-3"><summary className="cursor-pointer text-xs font-medium text-muted-foreground">Other explicit organization units ({secondaryUnits.length})</summary><div className="mt-3 flex flex-wrap gap-2">{secondaryUnits.map((unit) => <span key={unit.id} data-company-os-ref={unit.id} className="rounded-full border border-border bg-card px-3 py-1.5 text-xs">{unit.label} · {membersForUnit(view, unit.id).length} members</span>)}</div></details>}
      </div>
    </section>
  </PageFrame>;
}

function Connector() {
  return <div className="mx-auto flex h-9 w-px items-center justify-center bg-primary/35" aria-hidden><span className="size-2.5 rounded-full border border-primary bg-background" /></div>;
}

function OrgActorCard({ actor, linkedDocument, variant = "member", className }: { actor: ActorSummary; linkedDocument?: TrademarkOperationsProjection["sourceDocument"]; variant?: "owner" | "lead" | "member" | "external"; className?: string }) {
  const proposed = actor.organizationRoleState === "proposed";
  const actorKind = actorSemanticKind(actor);
  return <article data-company-os-ref={actor.id} data-actor-kind={actorKind} data-actor-type={actorKind} className={`${variant === "lead" ? "border-status-good/45 bg-status-good/[0.045]" : variant === "external" ? "border-sky-500/35 bg-sky-500/[0.035]" : proposed ? "border-primary/45 border-dashed bg-primary/[0.035]" : "border-border bg-card/90"} rounded-xl border p-4 shadow-sm ${className ?? ""}`}><div className="flex items-start gap-3"><ActorAvatar identity={`${actor.id} ${actor.role}`} name={actor.name} size={variant === "lead" || variant === "owner" ? "lg" : "md"} ring={variant === "owner" ? "warm" : variant === "lead" ? "good" : variant === "external" ? "external" : "neutral"} /><div className="min-w-0 flex-1"><div className="flex flex-wrap items-center gap-2"><h3 className={`${variant === "lead" || variant === "owner" ? "company-editorial-title text-xl" : "text-sm font-semibold"}`}>{actor.name}</h3>{actor.availability === "available" && <span className="size-2 rounded-full bg-status-good" title="Explicitly reported available" />}{proposed && <StatusTag status="proposed" />}</div><p className="mt-1 text-xs text-muted-foreground">{actor.kind === "human" ? "Human" : actor.kind === "external" ? "External Collaborator" : "Standing Agent"} · {actor.role}</p></div></div>{variant === "lead" && <div className="mt-4 grid grid-cols-3 divide-x divide-border rounded-lg border border-border bg-background/60 py-3 text-center text-xs"><div><strong className="block text-base">{actor.availability === "available" ? "Available" : "Active"}</strong><span className="text-muted-foreground">Presence</span></div><div><strong className="block text-base">{actor.unit ?? "Company"}</strong><span className="text-muted-foreground">Scope</span></div><div><strong className="block text-base">Lead</strong><span className="text-muted-foreground">Role</span></div></div>}{linkedDocument && <div className="mt-3 border-t border-border pt-2"><LinkedRecord recordRef={linkedDocument.id} label={linkedDocument.label} detail="Linked work source" icon={<FileText className="size-4" />} /></div>}</article>;
}

function membersForUnit(view: TrademarkOperationsProjection, unitId: string): ActorSummary[] {
  const unit = view.organization.units.find((entry) => entry.id === unitId);
  const members = unit?.actorIds.map((id) => view.actors[id]).filter((actor): actor is ActorSummary => Boolean(actor))
    ?? view.actorList.filter((actor) => actor.unit === unit?.label);
  return [...members].sort((left, right) => organizationRank(left) - organizationRank(right) || left.name.localeCompare(right.name));
}

function organizationRank(actor: ActorSummary): number {
  if (actor.kind === "human") return 0;
  if (actor.kind === "standing_agent" && actor.organizationRoleState !== "proposed") return 1;
  if (actor.kind === "standing_agent") return 2;
  if (actor.kind === "external") return 3;
  return 4;
}

function OrganizationNode({ icon, label, recordRef, className }: { icon: ReactNode; label: string; recordRef: string; className?: string }) {
  return <div data-company-os-ref={recordRef} className={`flex items-center gap-3 rounded-lg border border-border bg-card px-4 py-3 shadow-sm ${className ?? ""}`}><span className="grid size-8 place-items-center rounded-full bg-muted text-foreground">{icon}</span><span className="text-base font-semibold">{label}</span></div>;
}

function OrganizationMember({ actor, linkedDocument, availabilityNote = false }: { actor: ActorSummary; linkedDocument?: TrademarkOperationsProjection["sourceDocument"]; availabilityNote?: boolean }) {
  return <article className="relative mb-2" data-company-os-ref={actor.id}><span aria-hidden className="absolute -left-4 top-6 h-px w-4 bg-border sm:-left-7 sm:w-7" /><div className="rounded-lg border border-border bg-card p-2.5 shadow-sm"><div className="flex flex-wrap items-center justify-between gap-2"><ActorPill actor={actor} />{actor.organizationRoleState === "proposed" && <StatusTag status="proposed" />}{availabilityNote && actor.availability === "available" && <span className="inline-flex items-center gap-2 rounded-md border border-status-good/30 bg-status-good/10 px-2 py-1 text-xs font-medium text-status-good"><span className="size-2 rounded-full bg-status-good" />{actor.name} · Available</span>}</div>{linkedDocument && <div className="mt-2 border-t border-border pt-1"><LinkedRecord recordRef={linkedDocument.id} label={linkedDocument.label} detail="Linked organization context" icon={<FileText className="size-4" />} /></div>}</div></article>;
}

export function HumanMemberFocus({ data }: OperationsPageProps) {
  const view = projection(data);
  const actor = actorOr(view, "actor-human-brand-owner", view.workItem.accountableOwner);
  return <PageFrame eyebrow="Human member" title={actor.name} description="Owns Brand A decisions and retains the accountable human authority for the trademark filing." context={<ContextRail><Panel title="Authority"><p className="text-sm">Required human approver for the filing commitment and legal submission.</p></Panel><Panel title="Membership"><ActorPill actor={actor} /></Panel></ContextRail>}>
    <div className="space-y-5"><DecisionNotice><strong>Decision required.</strong> The pending {view.commitment.amount} commitment requires {actor.name} as the named human approver.</DecisionNotice><Panel title="Accountable work"><div data-company-os-ref={view.workItem.id}><WorkRoleTable workItem={view.workItem} /></div></Panel><Panel title="Owned documents"><div className="space-y-1"><LinkedRecord recordRef={view.contentPlanDocument.id} label={view.contentPlanDocument.label} detail={view.contentPlanDocument.detail} /><LinkedRecord recordRef={view.sourceDocument.id} label={view.sourceDocument.label} detail="Accountable owner · Brand & IP" /></div></Panel><Panel title="Approvals"><LinkedRecord recordRef={view.approval.id} label={view.approval.title} detail="Required approver · decision requested" /></Panel><FinancialRecordCard record={view.commitment} /></div>
  </PageFrame>;
}

export function StandingAgentFocus({ data, actorId }: OperationsPageProps & { actorId?: string }) {
  const view = projection(data);
  const actor = actorOr(view, actorId ?? "actor-agent-document-architecture", view.actorList[0] ?? view.workItem.submittedBy);
  const authoredProposal = view.governanceProposal.proposedById === actor.id && !view.governanceProposal.id.startsWith("unresolved")
    ? view.governanceProposal
    : undefined;
  const isLead = /lead/i.test(`${actor.name} ${actor.role}`);
  const leadUnit = isLead
    ? view.organization.units.find((unit) => unit.agentLeadActorId === actor.id)
      ?? view.organization.units.find((unit) => unit.actorIds.includes(actor.id))
    : undefined;
  const directReports = leadUnit
    ? leadUnit.actorIds
      .map((candidateId) => view.actors[candidateId])
      .filter((candidate): candidate is ActorSummary => candidate?.kind === "standing_agent" && candidate.id !== actor.id)
    : [];
  return <PageFrame eyebrow="Standing Agent workspace" title={actor.name} description="A durable organization role with documented collaboration. Execution attempts and hidden reasoning do not define membership." context={<ContextRail label="Organization context"><Panel title="Identity"><div className="flex flex-col items-center py-2 text-center"><ActorAvatar identity={`${actor.id} ${actor.role}`} name={actor.name} size="hero" ring={actor.availability === "available" ? "good" : "neutral"} /><p className="mt-4 font-semibold">{actor.name}</p><p className="mt-1 text-xs text-muted-foreground">{actor.role} · {actor.unit ?? "No unit linked"}</p>{actor.availability === "available" ? <p className="mt-3 inline-flex items-center gap-2 rounded-full border border-status-good/30 bg-status-good/10 px-3 py-1 text-xs font-medium text-status-good"><span className="size-2 rounded-full bg-status-good" />Available · explicitly reported</p> : <p className="mt-3 text-xs leading-5 text-muted-foreground">Availability has not been explicitly reported.</p>}</div></Panel><Panel title="Related structure"><LinkedRecord wrapLabel recordRef={view.businessModule.id} label={humanReadable(view.businessModule.label, "Business module")} detail={view.businessModule.detail} /></Panel><Panel title="Authority boundary"><p className="text-xs leading-5 text-muted-foreground">This role may propose document structure. Authority changes remain governed company actions.</p></Panel></ContextRail>}>
    <section aria-label="Standing Agent collaboration" className="flex min-h-[36rem] flex-col overflow-hidden rounded-2xl border border-border bg-card/80 shadow-sm"><header className="flex items-center justify-between gap-3 border-b border-border px-5 py-4"><div className="flex items-center gap-3"><ObjectEmblem kind="agent" /><div><h2 className="company-editorial-title text-2xl">Collaboration</h2><p className="mt-1 text-xs text-muted-foreground">Activities, messages, WorkItems, and governed proposals · no hidden reasoning</p></div></div>{actor.availability === "available" && <span className="inline-flex items-center gap-2 text-xs font-medium text-status-good"><span className="size-2 rounded-full bg-status-good" />Available</span>}</header><div className="min-h-0 flex-1 space-y-4 bg-muted/[0.16] p-5">{authoredProposal ? <article className="max-w-3xl rounded-xl border border-border bg-background/90 p-4 shadow-sm" data-company-os-ref={authoredProposal.id}><div className="flex items-start justify-between gap-3"><ActorPill actor={actor} compact /><span className="text-xs text-muted-foreground">Documented proposal</span></div><div className="mt-3"><LinkedRecord wrapLabel recordRef={authoredProposal.id} label={humanReadable(authoredProposal.label, "Governance proposal")} detail={authoredProposal.detail} /><p className="mt-2 text-sm leading-6 text-muted-foreground">Proposed company structure for a governed review. The linked module remains related structure, not a second authored activity.</p></div></article> : isLead ? <div className="space-y-5"><section className="rounded-xl border border-border bg-background/90 p-4"><p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">Direct reports from organization projection{leadUnit ? ` · ${leadUnit.label}` : ""}</p><div className="mt-4 grid gap-3 sm:grid-cols-2">{directReports.length > 0 ? directReports.map((report) => <div key={report.id} className="rounded-lg border border-border bg-card p-3"><ActorPill actor={report} /></div>) : <p className="text-sm text-muted-foreground">No Standing Agent direct report is recorded for this unit.</p>}</div></section><section className="rounded-xl border border-primary/20 bg-primary/[0.035] p-4" data-company-os-ref={view.workItem.id}><p className="text-[11px] font-semibold uppercase tracking-wider text-primary">Current linked pressure</p><h3 className="mt-2 font-semibold">{view.workItem.title}</h3><p className="mt-1 text-sm text-muted-foreground">{view.workItem.status.replace(/_/g, " ")} · Human approval remains required</p></section></div> : <p className="max-w-xl rounded-md border border-dashed border-border p-4 text-sm leading-6 text-muted-foreground">No documented collaboration entry is linked to this Standing Agent in the current projection.</p>}</div><form className="border-t border-border bg-card p-4" aria-label="Message composer"><label className="sr-only" htmlFor="standing-agent-message">Message {actor.name}</label><div className="flex items-end gap-2"><textarea id="standing-agent-message" disabled rows={2} placeholder={`Message ${actor.name}…`} aria-describedby="standing-agent-message-reason" className="min-h-14 flex-1 resize-none rounded-xl border border-input bg-muted/70 px-3 py-2 text-sm text-muted-foreground" /><button type="submit" disabled title={commandUnavailable} aria-label={`Send message. Unavailable: ${commandUnavailable}`} className="grid size-11 shrink-0 cursor-not-allowed place-items-center rounded-xl bg-muted text-muted-foreground"><Send className="size-4" /></button></div><p id="standing-agent-message-reason" className="mt-2 text-xs leading-5 text-muted-foreground">{commandUnavailable}</p></form></section>
  </PageFrame>;
}

export function WorkboardPage({ data }: OperationsPageProps) {
  const view = projection(data);
  return <PageFrame eyebrow="Work" title="Milestones & WorkItems" description="One durable work ledger for development, legal, procurement, operations, and every other company commitment." context={<ContextRail label="Work context"><Panel title="Ledger rules"><p className="text-xs leading-5 text-muted-foreground">Milestone grouping never replaces WorkItem identity. Requester, submitter, accountable owner, assignee, approval, and evidence remain separate facts.</p></Panel><LinkedRecord recordRef={view.sourceDocument.id} label={view.sourceDocument.label} detail="Source document" /><Panel title="Milestone coverage"><p className="text-sm font-medium">Unassigned milestone</p><p className="mt-1 text-xs leading-5 text-muted-foreground">The current projection supplies a WorkItem but no native Milestone record. The UI keeps that gap visible instead of inventing one.</p></Panel></ContextRail>}>
    <div className="space-y-5">
      <section className="grid gap-3 sm:grid-cols-3"><WorkStat label="Open WorkItems" value="1" detail="From current projection" tone="warm" /><WorkStat label="Waiting for approval" value={view.workItem.status === "waiting_for_approval" ? "1" : "0"} detail={view.approval.status === "requested" ? "Human decision requested" : "No request supplied"} tone="warn" /><WorkStat label="Milestone assignment" value="—" detail="Not supplied" tone="quiet" /></section>
      <section className="overflow-hidden rounded-2xl border border-border bg-card/75 shadow-sm"><header className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-5 py-4"><div className="flex items-center gap-3"><ObjectEmblem kind="work" /><div><h2 className="company-editorial-title text-2xl">Unassigned milestone</h2><p className="mt-0.5 text-xs text-muted-foreground">WorkItems awaiting an explicit Milestone relation</p></div></div><StatusTag status={view.workItem.status} /></header><div className="p-4"><WorkLedgerItem workItem={view.workItem} approval={view.approval} /></div></section>
    </div>
  </PageFrame>;
}

function WorkStat({ label, value, detail, tone }: { label: string; value: string; detail: string; tone: "warm" | "warn" | "quiet" }) {
  return <div className={`${tone === "warm" ? "border-primary/25 bg-primary/[0.05]" : tone === "warn" ? "border-status-warn/30 bg-status-warn/[0.05]" : "border-border bg-card/70"} rounded-xl border p-4`}><p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</p><p className="company-editorial-title mt-2 text-3xl">{value}</p><p className="mt-1 text-xs text-muted-foreground">{detail}</p></div>;
}

function WorkLedgerItem({ workItem, approval }: { workItem: WorkItemView; approval: TrademarkOperationsProjection["approval"] }) {
  return <article data-company-os-ref={workItem.id} data-work-item-status={workItem.status} className="rounded-xl border border-border bg-background/70 p-4"><div className="flex flex-wrap items-start justify-between gap-4"><div className="min-w-0"><div className="flex items-center gap-2"><ObjectEmblem kind="work" className="size-8 rounded-lg" /><span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Legal WorkItem</span></div><h3 className="mt-3 text-base font-semibold">{workItem.title}</h3><div className="mt-1 max-w-xl"><LinkedRecord recordRef={workItem.sourceDocument.id} label={workItem.sourceDocument.label} detail="Source document" /></div></div><div className="flex -space-x-2">{[workItem.accountableOwner, workItem.assignees[0], workItem.contributors[0]].filter((actor): actor is ActorSummary => Boolean(actor)).map((actor) => <ActorAvatar key={actor.id} identity={`${actor.id} ${actor.role}`} name={actor.name} size="md" ring={actor.kind === "human" ? "warm" : actor.kind === "external" ? "external" : "neutral"} />)}</div></div><dl className="mt-4 grid gap-3 border-t border-border pt-4 sm:grid-cols-2 xl:grid-cols-3"><BoardFact label="Requested by" actor={workItem.requestedBy} /><BoardFact label="Submitted by" actor={workItem.submittedBy} /><BoardFact label="Accountable" actor={workItem.accountableOwner} /><BoardFact label="Assignee" actor={workItem.assignees[0]} /><BoardFact label="Contributor" actor={workItem.contributors[0]} /><BoardFact label="Finance reviewer" actor={workItem.reviewer} /></dl><div className="mt-4 rounded-lg border border-primary/20 bg-primary/[0.035] p-2"><LinkedRecord recordRef={approval.id} label="Approval requested" detail={`Human decision required · ${actorDescriptor(approval.requiredApprover)}`} /></div></article>;
}

function BoardColumn({ title, items, approval }: { title: string; items: WorkItemView[]; approval: TrademarkOperationsProjection["approval"] }) {
  return <section className="min-h-64 rounded-lg border border-border bg-muted/35 p-3"><h2 className="mb-3 text-sm font-semibold">{title}<span className="ml-2 text-muted-foreground">{items.length}</span></h2><div className="space-y-3">{items.map((workItem) => <article key={workItem.id} data-company-os-ref={workItem.id} data-work-item-status={workItem.status} className="rounded-md border border-border bg-card p-3"><StatusTag status={workItem.status} /><h3 className="mt-3 text-sm font-semibold">{workItem.title}</h3><LinkedRecord recordRef={workItem.sourceDocument.id} label={workItem.sourceDocument.label} detail="Source document" /><dl className="mt-4 space-y-2 border-t border-border pt-3 text-sm"><BoardFact label="Requested by" actor={workItem.requestedBy} /><BoardFact label="Submitted by" actor={workItem.submittedBy} /><BoardFact label="Accountable" actor={workItem.accountableOwner} /><BoardFact label="Assignee" actor={workItem.assignees[0]} /><BoardFact label="Contributor" actor={workItem.contributors[0]} /><BoardFact label="Finance reviewer" actor={workItem.reviewer} /></dl><LinkedRecord recordRef={approval.id} label="Approval requested" detail={`Human decision required · ${actorDescriptor(approval.requiredApprover)}`} /></article>)}</div></section>;
}

function BoardFact({ label, actor }: { label: string; actor?: ActorSummary }) {
  const kind = actorSemanticKind(actor);
  return <div className="grid grid-cols-[6.5rem_minmax(0,1fr)] gap-2"><dt className="text-xs text-muted-foreground">{label}</dt><dd className="min-w-0 break-words text-sm leading-5 text-foreground" data-company-os-ref={actor?.id} data-actor-kind={kind} data-actor-type={kind}>{actorDescriptor(actor)}</dd></div>;
}

export function WorkItemFocus({ data }: OperationsPageProps) {
  const view = projection(data);
  const { workItem, commitment, approval } = view;
  const approvalTitle = humanReadable(approval.title, "Approval decision");
  return <div data-company-os-ref={workItem.id} data-work-item-status={workItem.status}><PageFrame eyebrow="Work item" title={workItem.title} description="A linked business commitment with explicit responsibility, evidence and the human authorization it still needs." context={<ContextRail><StatusTag status={workItem.status} /><Panel title="Source"><LinkedRecord recordRef={workItem.sourceDocument.id} label={workItem.sourceDocument.label} detail="Durable source context" /><LinkedRecord recordRef={view.typedApplication.id} label={view.typedApplication.label} detail={view.typedApplication.detail} /></Panel><Panel title="Last updated"><p className="text-sm">{displayTimestamp(workItem.updatedAt)}</p></Panel></ContextRail>}>
    <div className="space-y-5"><DecisionNotice><strong>Blocked by authorization, not execution.</strong> Preparation may continue within policy; filing and the linked commitment require human approval.</DecisionNotice><Panel title="Evidence"><div className="space-y-1">{view.evidence.length > 0 ? view.evidence.map((evidence) => <LinkedRecord key={evidence.id} recordRef={evidence.id} label={evidence.label} detail={evidence.detail} />) : <p className="text-sm text-muted-foreground">No evidence is linked in this projection.</p>}</div></Panel><div className="grid gap-5 lg:grid-cols-2"><Panel title="Approval decision"><LinkedRecord wrapLabel recordRef={approval.id} label={approvalTitle} detail={`Decision requested from ${actorDescriptor(approval.requiredApprover)}`} /><p className="mt-3 break-words text-sm leading-6 text-muted-foreground">{humanReadable(approval.actionSummary, "No approval summary was supplied.")}</p></Panel><Panel title="Financial relation"><FinancialRecordCard record={commitment} /></Panel></div><Panel title="Responsibility"><WorkRoleTable workItem={workItem} /></Panel></div>
  </PageFrame></div>;
}

function WorkRoleTable({ workItem }: { workItem: WorkItemView }) {
  return <div className="divide-y divide-border"><RoleLine label="Requested by" actor={workItem.requestedBy} /><RoleLine label="Submitted by" actor={workItem.submittedBy} /><RoleLine label="Accountable owner" actor={workItem.accountableOwner} /><RoleLine label="Assignee" actor={workItem.assignees[0]} /><RoleLine label="Contributor" actor={workItem.contributors[0]} /><RoleLine label="Finance reviewer" actor={workItem.reviewer} /><RoleLine label="Legal reviewer" actor={workItem.legalReviewer} /><RoleLine label="Approver" actor={workItem.approver} /></div>;
}

export function ApprovalFocus({ data, actionEnabled = false, onDecision }: ApprovalFocusProps) {
  const view = projection(data);
  const { approval, commitment } = view;
  const approvalTitle = humanReadable(approval.title, "Approval decision");
  const [capabilityToken, setCapabilityToken] = useState("");
  const [decisionNote, setDecisionNote] = useState("");
  const [submitting, setSubmitting] = useState<ApprovalDecision | null>(null);
  const [feedback, setFeedback] = useState<string | null>(null);
  const intents = useRef<Partial<Record<ApprovalDecision, { id: string; decidedAt: string }>>>({});
  const canDecide = actionEnabled && Boolean(onDecision) && approval.status === "requested" && Boolean(approval.decisionContext);
  const ready = canDecide && Boolean(capabilityToken.trim()) && Boolean(decisionNote.trim()) && !submitting;
  async function decide(decision: ApprovalDecision) {
    if (!ready || !onDecision) return;
    const intent = intents.current[decision] ?? {
      id: `action-browser-${approval.id}-${decision}-${crypto.randomUUID()}`,
      decidedAt: new Date().toISOString(),
    };
    intents.current[decision] = intent;
    setSubmitting(decision);
    setFeedback(null);
    try {
      const command = buildApprovalDecisionCommand({ approval, decision, note: decisionNote, commandId: intent.id, decidedAt: intent.decidedAt });
      const accepted = await onDecision(command, capabilityToken.trim());
      if (accepted) {
        setCapabilityToken("");
        setDecisionNote("");
      }
      setFeedback(accepted ? `${decision === "approved" ? "Approval" : "Rejection"} recorded in Store truth.` : "Decision was not applied. Review the action error above and retry with the same intent.");
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(null);
    }
  }
  const unavailableReason = !actionEnabled
    ? commandUnavailable
    : !approval.decisionContext
      ? "The current projection does not expose a complete approval.decide contract."
      : approval.status !== "requested"
        ? `This Approval is already ${approval.status}.`
        : !capabilityToken.trim() || !decisionNote.trim()
          ? "Enter the session capability and a durable decision note."
          : undefined;
  const decisionControls = <div className="w-full max-w-lg space-y-2" aria-label="Approval decision controls" data-company-os-action-state={canDecide ? "available" : "unavailable"}><div className="grid gap-2 sm:grid-cols-2"><label className="text-xs font-medium text-muted-foreground">Session capability<input data-company-os-action-token type="password" autoComplete="off" value={capabilityToken} onChange={(event) => setCapabilityToken(event.target.value)} disabled={!canDecide} placeholder="Not stored" className="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground disabled:bg-muted" /></label><label className="text-xs font-medium text-muted-foreground">Decision note<input data-company-os-decision-note value={decisionNote} onChange={(event) => setDecisionNote(event.target.value)} disabled={!canDecide} placeholder="Required for audit" className="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground disabled:bg-muted" /></label></div><div className="flex flex-wrap gap-2"><GovernedActionButton label={submitting === "approved" ? "Approving…" : "Approve"} reason={unavailableReason} disabled={!ready} onClick={() => void decide("approved")} /><GovernedActionButton label="Request changes" reason="Request changes needs a separate native Approval status or follow-up WorkItem contract." /><GovernedActionButton label={submitting === "rejected" ? "Rejecting…" : "Reject"} reason={unavailableReason} disabled={!ready} onClick={() => void decide("rejected")} /></div><p className="max-w-lg text-xs leading-5 text-muted-foreground">{feedback ?? unavailableReason ?? "The capability remains in this browser session only. The server still validates Human identity, permission, policy, scope and idempotency."}</p></div>;
  return (
    <PageFrame
      eyebrow="Approval"
      title={approvalTitle}
      description="A formal authorization record. A review, activity event or Agent recommendation cannot substitute for it."
      action={decisionControls}
      context={<ContextRail><StatusTag status={approval.status} /><Panel title="Expires"><p className="text-sm">{approval.expiresAt ? displayTimestamp(approval.expiresAt) : "No expiry recorded"}</p></Panel><Panel title="Policy"><p className="text-xs leading-5 text-muted-foreground">Human approval for financial and legal submission</p></Panel></ContextRail>}
    >
      <div className="space-y-5" data-company-os-ref={approval.id}>
        <DecisionNotice>{approval.status === "requested" ? <><strong>Human action required.</strong> {actorDescriptor(approval.requiredApprover)} is the required approver; no payment is authorized or recorded by this pending approval.</> : <><strong>Decision recorded: {approval.status}.</strong> The Approval changed state, while the linked Commitment and any future Payment remain separate governed records.</>}</DecisionNotice>
        <Panel title="Evidence"><div className="space-y-1">{view.evidence.length > 0 ? view.evidence.map((evidence) => <LinkedRecord key={evidence.id} recordRef={evidence.id} label={evidence.label} detail={evidence.detail} />) : <p className="text-sm text-muted-foreground">No evidence is linked in this projection.</p>}</div></Panel>
        <Panel title="Proposed action"><p className="break-words text-sm leading-6">{humanReadable(approval.actionSummary, "No approval summary was supplied.")}</p><LinkedRecord recordRef={view.workItem.id} label={view.workItem.title} detail="Linked WorkItem" /><LinkedRecord recordRef={view.sourceDocument.id} label={view.sourceDocument.label} detail="Source document" /></Panel>
        <Panel title="Participants"><div className="divide-y divide-border"><RoleLine label="Requested by" actor={approval.requestedBy} /><RoleLine label="Required approver" actor={approval.requiredApprover} /><RoleLine label="Finance reviewed by" actor={approval.financeReviewer} /><RoleLine label="Legal reviewed by" actor={approval.legalReviewer} /></div></Panel>
        <Panel title="Linked financial record"><FinancialRecordCard record={commitment} /></Panel>
      </div>
    </PageFrame>
  );
}

export function FinancePage({ data }: OperationsPageProps) {
  const view = projection(data);
  const approvalTitle = humanReadable(view.approval.title, "Approval decision");
  return <PageFrame eyebrow="Finance" title="Finance overview" description="A typed, auditable relation graph. Documents render the same financial records; they do not become a second ledger." context={<ContextRail><Panel title={view.julySpendMetric.label}><div data-company-os-ref={view.julySpendMetric.id}><p className="text-2xl font-semibold">{view.julySpendAmount}</p><p className="mt-1 text-xs text-muted-foreground">Observed from the resolved projection</p></div></Panel><PolicyNote>Agents can prepare or review. A named human remains required to authorize a new commitment or payment.</PolicyNote></ContextRail>}>
    <div className="space-y-5"><DecisionNotice><strong>One commitment needs a decision.</strong> The {view.commitment.label} is not paid or settled; it is a pending {view.commitment.amount} commitment.</DecisionNotice><Panel title="Financial record"><FinanceRecordTable record={view.commitment} approval={view.approval} /></Panel><Panel title="Approval context"><LinkedRecord wrapLabel recordRef={view.approval.id} label={approvalTitle} detail={`Required approver · ${actorDescriptor(view.approval.requiredApprover)}`} /><p className="mt-3 break-words text-sm leading-6 text-muted-foreground">{humanReadable(view.approval.actionSummary, "No approval summary was supplied.")}</p><div className="mt-3 grid gap-3 sm:grid-cols-2"><ActorPill actor={view.commitment.accountableOwner} />{view.approval.financeReviewer && <ActorPill actor={view.approval.financeReviewer} />}</div></Panel></div>
  </PageFrame>;
}

function FinanceRecordTable({ record, approval }: { record: TrademarkOperationsProjection["commitment"]; approval: TrademarkOperationsProjection["approval"] }) {
  const rows = [
    ["Record type", humanReadable(record.type, "Unknown")],
    ["Amount", record.amount],
    ["Project", record.project?.label ?? "No project linked"],
    ["Source", record.sourceDocument.label],
    ["Approval status", humanReadable(approval.status, "Unknown")],
  ];
  return <div className="overflow-x-auto" data-company-os-ref={record.id} data-financial-record-type={record.type} data-financial-type={record.type} data-financial-status={record.status}><table className="min-w-[34rem] w-full border-collapse text-left text-sm"><caption className="sr-only">Auditable fields for {record.label}</caption><tbody>{rows.map(([label, value]) => <tr key={label} className="border-b border-border last:border-0"><th scope="row" className="w-40 py-3 pr-4 text-xs font-medium uppercase tracking-wide text-muted-foreground">{label}</th><td className="py-3 break-words text-foreground" data-company-os-ref={label === "Source" ? record.sourceDocument.id : undefined}>{value}</td></tr>)}</tbody></table></div>;
}

export function GovernanceProposalFocus({ data }: OperationsPageProps) {
  const view = projection(data);
  const proposer = actorOr(view, "actor-agent-document-architecture", view.workItem.submittedBy);
  const proposalTitle = humanReadable(view.governanceProposal.label, "Governance proposal");
  const proposedAgent = view.workItem.assignees[0];
  return <PageFrame eyebrow="Governance proposal" title={proposalTitle} description="A proposal joins document architecture, organization capacity, work and financial controls without creating authority by itself." context={<ContextRail><StatusTag status="awaiting_final_approval" /><Panel title="Proposed by"><ActorPill actor={proposer} /></Panel><Panel title="Proposed home"><p className="text-sm">{view.organization.brandUnit.label}</p><p className="mt-1 text-xs text-muted-foreground">Proposed role · {proposedAgent?.role ?? "No role specified"}</p></Panel></ContextRail>}>
    <div className="space-y-5" data-company-os-ref={view.governanceProposal.id}><DecisionNotice><strong>Awaiting final approval.</strong> The module and the proposed role remain governed changes. Human approval is required for filing fees and legal submission.</DecisionNotice><Panel title="Impact surfaces"><div className="grid gap-3 md:grid-cols-2"><ImpactSurface label="Business module" link={view.businessModule} /><ImpactSurface label="Application record" link={view.typedApplication} /><ImpactSurface label="Linked work" link={{ id: view.workItem.id, label: view.workItem.title, detail: `Assignee · ${actorDescriptor(proposedAgent)}` }} /><ImpactSurface label="Financial commitment" financialRecord={view.commitment} /></div></Panel><Panel title="Review participants"><div className="divide-y divide-border"><RoleLine label="Accountable owner" actor={view.workItem.accountableOwner} /><RoleLine label="Finance reviewer" actor={view.approval.financeReviewer} /><RoleLine label="Legal reviewer" actor={view.approval.legalReviewer} /></div></Panel><Panel title="Governed actions"><div className="flex flex-wrap gap-2"><GovernedActionButton label="Approve proposal" reason={commandUnavailable} /><GovernedActionButton label="Request changes" reason={commandUnavailable} /><GovernedActionButton label="Reject proposal" reason={commandUnavailable} /></div><p className="mt-3 text-xs leading-5 text-muted-foreground">{commandUnavailable}</p></Panel></div>
  </PageFrame>;
}

function ImpactSurface({ label, link, financialRecord }: { label: string; link?: { id: string; label: string; detail?: string }; financialRecord?: TrademarkOperationsProjection["commitment"] }) {
  return <div className="rounded-md border border-border p-3"><p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{label}</p>{financialRecord ? <div className="mt-2"><FinancialRecordCard record={financialRecord} /></div> : link && <LinkedRecord wrapLabel recordRef={link.id} label={humanReadable(link.label, "Unresolved record")} detail={link.detail} />}</div>;
}

export function BusinessModuleFocus({ data }: OperationsPageProps) {
  const view = projection(data);
  return <PageFrame eyebrow="Business module · proposed" title={view.businessModule.label} description="A governed operating area joining applications, WorkItems, evidence, participants, and financial effects. This view does not assert that it was created from an approved Module Design." context={<ContextRail><StatusTag status="awaiting_final_approval" /><Panel title="Module identity"><div className="flex items-center gap-3"><ObjectEmblem kind="module" /><div><p className="font-semibold">{view.businessModule.label}</p><p className="text-xs text-muted-foreground">{view.organization.brandUnit.label}</p></div></div></Panel><Panel title="Owner"><ActorPill actor={view.workItem.accountableOwner} /></Panel><Panel title="Governance"><LinkedRecord wrapLabel recordRef={view.governanceProposal.id} label={view.governanceProposal.label} detail={view.governanceProposal.detail} /></Panel></ContextRail>}>
    <div className="space-y-5" data-company-os-ref={view.businessModule.id}><section className="relative overflow-hidden rounded-2xl border border-primary/20 bg-gradient-to-br from-primary/[0.09] via-card to-card p-6"><div className="pointer-events-none absolute -right-20 -top-20 size-64 rounded-full border border-primary/20" /><div className="pointer-events-none absolute -right-7 -top-7 size-36 rounded-full border border-primary/25" /><div className="relative flex flex-wrap items-center justify-between gap-5"><div className="flex items-center gap-4"><ObjectEmblem kind="module" className="size-16 rounded-2xl" /><div><p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-primary">Trademark lifecycle</p><h2 className="company-editorial-title mt-1 text-3xl">Governed filing operations</h2><p className="mt-2 max-w-xl text-sm leading-6 text-muted-foreground">Application preparation, legal evidence, Human approval, and financial commitment stay linked as one operating truth.</p></div></div><StatusTag status="awaiting_final_approval" /></div></section><DecisionNotice><strong>Module proposal pending.</strong> The module is awaiting final approval; the Trademark Agent role is proposed rather than active organization capacity.</DecisionNotice><nav aria-label="Module sections" className="flex gap-1 rounded-xl border border-border bg-card/70 p-1 text-sm"><button className="rounded-lg bg-primary/[0.09] px-4 py-2 font-medium text-primary" type="button">Overview</button><button className="rounded-lg px-4 py-2 text-muted-foreground hover:bg-muted" type="button">Applications</button><button className="rounded-lg px-4 py-2 text-muted-foreground hover:bg-muted" type="button">Work</button><button className="rounded-lg px-4 py-2 text-muted-foreground hover:bg-muted" type="button">Finance</button></nav><div className="grid gap-5 lg:grid-cols-2"><Panel title="Application"><LinkedRecord recordRef={view.typedApplication.id} label={view.typedApplication.label} detail={view.typedApplication.detail} /></Panel><Panel title="Linked work"><LinkedRecord recordRef={view.workItem.id} label={view.workItem.title} detail={`Assignee · ${view.workItem.assignees[0]?.name ?? "Unassigned"}`} /></Panel><Panel title="Finance"><FinancialRecordCard record={view.commitment} /></Panel><Panel title="Participants"><div className="space-y-3"><ActorPill actor={view.workItem.accountableOwner} />{view.workItem.assignees[0] && <ActorPill actor={view.workItem.assignees[0]} />}{view.workItem.contributors[0] && <ActorPill actor={view.workItem.contributors[0]} />}</div></Panel></div></div>
  </PageFrame>;
}
