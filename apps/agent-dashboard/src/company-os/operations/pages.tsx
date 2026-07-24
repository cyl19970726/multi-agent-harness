import { useRef, useState, type ReactNode } from "react";
import { Bot, BriefcaseBusiness, Building2, CheckCircle2, CircleDollarSign, Clock3, FileCheck2, FileText, KeyRound, Landmark, Library, Network, Plus, Route, Scale, Search, Send, ShieldCheck, Sparkles, Tag, Users, Wrench } from "lucide-react";

import {
  ActorPill, ContextRail, DecisionNotice,
  FinancialRecordCard, GovernedActionButton, LinkedRecord, PageFrame, Panel, PolicyNote, RoleLine, StatusTag,
} from "./components";
import { prototypeTrademarkOperationsProjection } from "./fixture";
import { buildApprovalDecisionCommand } from "./approvalAction";
import { buildWorkItemTransitionCommand } from "./workItemAction";
import type { ActorSummary, ApprovalDecision, ApprovalDecisionCommand, TrademarkOperationsProjection, WorkItemTransitionCommand, WorkItemTransitionStatus, WorkItemView } from "./types";
import { ActorAvatar, ObjectEmblem } from "../visuals";
import { ActivityStream, type WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";
import { ContextModule, ContextRail as WorkbenchContextRail } from "@/components/workbench/context/ContextRail";
import { FocusHeader, FocusShell } from "@/components/workbench/layout/FocusShell";
import { Badge } from "@/components/ui/badge";
import type { SelectionState } from "@/app/selection";

type OperationsPageProps = { data?: TrademarkOperationsProjection };
type ApprovalFocusProps = OperationsPageProps & {
  actionEnabled?: boolean;
  onDecision?: (command: ApprovalDecisionCommand, capabilityToken: string) => Promise<boolean>;
};
type WorkItemFocusProps = OperationsPageProps & {
  actionEnabled?: boolean;
  onTransition?: (command: WorkItemTransitionCommand, capabilityToken: string) => Promise<boolean>;
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

export function StandingAgentFocus({ data, actorId, onSelectionChange }: OperationsPageProps & { actorId?: string; onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
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
  const membershipUnit = view.organization.units.find((unit) => unit.actorIds.includes(actor.id));
  const reportsTo = actor.membershipRole === "lead"
    ? undefined
    : view.actors[membershipUnit?.agentLeadActorId ?? ""];
  const assignedItems = (view.workItems ?? [view.workItem]).filter((workItem) =>
    workItem.assignees.some((assignee) => assignee.id === actor.id)
    || workItem.accountableOwner.id === actor.id
    || workItem.submittedBy.id === actor.id,
  );
  const actorAssignments = (view.assignments ?? []).filter((assignment) => assignment.recipient.id === actor.id);
  const assignedWork = assignedItems.length > 0;
  const maintainedDocuments = (actor.maintainedDocumentRefs ?? []).map((recordRef) => {
    if (recordRef === view.sourceDocument.id) return view.sourceDocument;
    if (recordRef === view.contentPlanDocument.id) return view.contentPlanDocument;
    return { id: recordRef, label: recordRef, detail: "Maintained document reference" };
  });
  const activity: WorkbenchActivityItem[] = [
    ...actorAssignments.map((assignment) => ({
      id: `assignment-${assignment.id}`,
      kind: "delegation" as const,
      glyph: "assignment" as const,
      tone: assignment.deliveryState === "failed" ? "bad" as const : "info" as const,
      title: `${assignment.sender.name} assigned ${assignment.assignedRole}`,
      body: assignment.scope,
      actor: assignment.sender.name,
      timestamp: displayTimestamp(assignment.assignedAt),
      evidenceRefs: [assignment.deliveryEvidenceRef, assignment.correlationId].filter((value): value is string => Boolean(value)),
    })),
    ...assignedItems.map((workItem) => ({
      id: `work-${workItem.id}`,
      kind: "action" as const,
      glyph: workItem.status === "completed" ? "complete" as const : "start" as const,
      tone: workItem.status === "completed" ? "good" as const : "running" as const,
      title: workItem.title,
      body: `Organization work · ${humanReadable(workItem.status, workItem.status)}${workItem.outcomeSummary ? ` · ${workItem.outcomeSummary}` : ""}`,
      actor: actor.name,
      timestamp: displayTimestamp(workItem.updatedAt),
      evidenceRefs: [workItem.sourceDocument.id],
    })),
    ...(authoredProposal ? [{
      id: `proposal-${authoredProposal.id}`,
      kind: "decision" as const,
      glyph: "decision" as const,
      tone: "decision" as const,
      title: humanReadable(authoredProposal.label, "Governance proposal"),
      body: authoredProposal.detail ?? "Submitted for governed organization review.",
      actor: actor.name,
      evidenceRefs: [authoredProposal.id],
    }] : []),
    ...maintainedDocuments.map((document) => ({
      id: `document-${document.id}`,
      kind: "evidence" as const,
      glyph: "artifact" as const,
      title: document.label,
      body: "Durable company context maintained by this Standing Agent.",
      actor: actor.name,
      evidenceRefs: [document.id],
    })),
  ];
  const configurationEmpty = !actor.systemPromptRef
    && !(actor.toolRefs?.length)
    && !(actor.skillRefs?.length)
    && !(actor.permissionPolicyRefs?.length);
  return <div className="h-full min-h-0 bg-[#fdfcf9]" data-standing-agent-workspace data-company-os-ref={actor.id}>
    <FocusShell
      className="h-full min-h-0 bg-[#fdfcf9]"
      headerClassName="bg-[#fdfcf9] px-6 py-4 sm:px-8"
      composerClassName="bg-background px-6 py-3 shadow-[0_-12px_30px_-28px_rgba(15,23,42,0.55)] sm:px-8"
      responsiveContextVariant="sheet"
      mainLabel="Standing Agent work and activity"
      header={<FocusHeader
        eyebrow="Organization · Standing Agent"
        title={<span className="flex items-center gap-3"><ActorAvatar identity={`${actor.id} ${actor.role}`} name={actor.name} size="md" ring={actor.availability === "available" ? "good" : "neutral"} /><span>{actor.name}</span></span>}
        description={actor.responsibilitySummary ?? "A durable organization identity. Runtime attempts and private reasoning do not define membership or authority."}
        meta={<><Badge tone={actor.availability === "available" ? "good" : "muted"}>{actor.availability ?? "availability unknown"}</Badge><Badge tone="muted">{actor.role}</Badge>{actor.unit && <Badge tone="muted">{actor.unit}</Badge>}</>}
      />}
      context={<WorkbenchContextRail label="Organization context" quiet>
        <ContextModule title="Organization identity" kicker={actor.membershipRole ?? "member"} icon={<Bot className="size-3.5" />} tone={actor.availability === "available" ? "good" : undefined}>
          <dl className="space-y-2 text-xs"><RailFact label="Unit" value={actor.unit ?? "Not linked"} /><RailFact label="Reports to" value={reportsTo?.name ?? (isLead ? "Human Owner / company policy" : "Not recorded")} /><RailFact label="Capacity" value={assignedWork ? "Active assignment visible" : "No linked active assignment"} /></dl>
          {isLead && directReports.length > 0 && <div className="mt-3 border-t border-border/70 pt-3"><p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Direct reports</p><div className="space-y-2">{directReports.map((report) => <ActorPill key={report.id} actor={report} compact />)}</div></div>}
        </ContextModule>
        <ContextModule title="Permissions" kicker="Organization-owned authority" icon={<KeyRound className="size-3.5" />} collapsible>
          <ReferenceList values={actor.permissionPolicyRefs} empty="No permission policy is recorded." />
        </ContextModule>
        <ContextModule title="Prompt, tools & skills" kicker="Configuration references" icon={<Wrench className="size-3.5" />} collapsible defaultOpen={!configurationEmpty}>
          <ReferenceGroup label="System prompt" values={actor.systemPromptRef ? [actor.systemPromptRef] : []} />
          <ReferenceGroup label="Tools" values={actor.toolRefs} />
          <ReferenceGroup label="Skills" values={actor.skillRefs} />
          {configurationEmpty && <p className="text-xs leading-5 text-muted-foreground">This projection does not yet provide native configuration references.</p>}
        </ContextModule>
        <ContextModule title="Work routing" kicker="Accepted work & escalation" icon={<Route className="size-3.5" />} collapsible>
          <ReferenceGroup label="Work types" values={actor.acceptedWorkTypeRefs} />
          <ReferenceGroup label="Escalation" values={actor.escalationPolicyRef ? [actor.escalationPolicyRef] : []} />
        </ContextModule>
        <ContextModule title="Maintained Docs" kicker="Linked company memory" icon={<Library className="size-3.5" />} collapsible>
          {maintainedDocuments.length > 0 ? maintainedDocuments.map((document) => <LinkedRecord key={document.id} wrapLabel recordRef={document.id} label={document.label} detail={document.detail} onClick={onSelectionChange ? () => onSelectionChange({ surface: "docs", documentId: document.id }) : undefined} />) : <p className="text-xs text-muted-foreground">No maintained Document is recorded.</p>}
        </ContextModule>
        <ContextModule title="Authority boundary" icon={<ShieldCheck className="size-3.5" />}>
          <p className="text-xs leading-5 text-muted-foreground">Tools and Skills enable work; they never grant authority. Money requires Finance policy, and sensitive company actions may still require a named Human approval.</p>
        </ContextModule>
      </WorkbenchContextRail>}
      composer={<form aria-label="Message Standing Agent" className="mx-auto flex w-full max-w-[1080px] items-end gap-2"><div className="min-w-0 flex-1"><label className="sr-only" htmlFor="standing-agent-message">Message {actor.name}</label><textarea id="standing-agent-message" disabled rows={2} placeholder={`Message ${actor.name}…`} aria-describedby="standing-agent-message-reason" className="min-h-14 w-full resize-none rounded-xl border border-input bg-muted/65 px-3 py-2 text-sm text-muted-foreground" /><p id="standing-agent-message-reason" className="mt-1 text-[10px] text-muted-foreground">{commandUnavailable}</p></div><button type="submit" disabled title={commandUnavailable} aria-label={`Send message. Unavailable: ${commandUnavailable}`} className="grid size-11 shrink-0 cursor-not-allowed place-items-center rounded-xl bg-muted text-muted-foreground"><Send className="size-4" /></button></form>}
    >
      <div className="mx-auto w-full max-w-[1080px] space-y-5 px-5 py-6 sm:px-8">
        <section aria-labelledby="standing-agent-current-work" className="rounded-2xl border border-border bg-card/85 p-5 shadow-sm">
          <div className="flex items-center gap-3"><span className="grid size-9 place-items-center rounded-xl border border-primary/20 bg-primary/[0.07] text-primary"><BriefcaseBusiness className="size-4" /></span><div><h2 id="standing-agent-current-work" className="text-lg font-semibold tracking-tight">Current work</h2><p className="text-xs text-muted-foreground">Native WorkItems linked through accountable actor references</p></div></div>
          {assignedWork ? <div className="mt-4 space-y-3">{assignedItems.map((workItem) => <div key={workItem.id} className="rounded-xl border border-primary/20 bg-primary/[0.035] p-4" data-company-os-ref={workItem.id}><div className="flex flex-wrap items-start justify-between gap-3"><div><p className="text-[10px] font-semibold uppercase tracking-wider text-primary">{workItem.status.replace(/_/g, " ")}</p>{onSelectionChange ? <button type="button" onClick={() => onSelectionChange({ surface: "work", workItemId: workItem.id })} className="mt-1 text-left font-semibold underline-offset-4 hover:text-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">{workItem.title}</button> : <h3 className="mt-1 font-semibold">{workItem.title}</h3>}{workItem.outcomeSummary && <p className="mt-1 text-xs leading-5 text-muted-foreground">{workItem.outcomeSummary}</p>}</div><StatusTag status={workItem.status} /></div><div className="mt-3"><LinkedRecord recordRef={workItem.sourceDocument.id} label={workItem.sourceDocument.label} detail="Source Document" onClick={onSelectionChange ? () => onSelectionChange({ surface: "docs", documentId: workItem.sourceDocument.id }) : undefined} /></div></div>)}</div> : <p className="mt-4 rounded-xl border border-dashed border-border p-4 text-sm text-muted-foreground">No WorkItem in the current Store projection is assigned to this Standing Agent.</p>}
        </section>
        <section aria-labelledby="standing-agent-activity" className="overflow-hidden rounded-2xl border border-border bg-card/85 shadow-sm"><header className="flex items-center justify-between gap-3 border-b border-border px-5 py-4"><div className="flex items-center gap-3"><Sparkles className="size-4 text-primary" /><div><h2 id="standing-agent-activity" className="text-lg font-semibold tracking-tight">Activity & collaboration</h2><p className="text-xs text-muted-foreground">Durable work, messages, decisions, evidence and Docs updates · never private thinking</p></div></div><Badge tone="muted">{activity.length} records</Badge></header><ActivityStream items={activity} variant="timeline" empty={<p className="text-sm text-muted-foreground">No durable activity is linked in this projection.</p>} className="px-5 py-2" /></section>
      </div>
    </FocusShell>
  </div>;
}

function RailFact({ label, value }: { label: string; value: string }) {
  return <div className="grid grid-cols-[5rem_minmax(0,1fr)] gap-2"><dt className="text-muted-foreground">{label}</dt><dd className="break-words text-foreground">{value}</dd></div>;
}

function ReferenceList({ values, empty }: { values?: string[]; empty: string }) {
  return values?.length ? <ul className="space-y-1.5">{values.map((value) => <li key={value} title={value} className="min-w-0 rounded-md border border-border/70 bg-background/70 px-2 py-1.5"><span className="block break-words text-[11px] font-medium text-foreground">{humanReadable(value, value)}</span><code className="mt-0.5 block truncate font-mono text-[9px] text-muted-foreground">{value}</code></li>)}</ul> : <p className="text-xs leading-5 text-muted-foreground">{empty}</p>;
}

function ReferenceGroup({ label, values }: { label: string; values?: string[] }) {
  if (!values?.length) return null;
  return <div className="mb-3 last:mb-0"><p className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</p><ReferenceList values={values} empty="" /></div>;
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

export function WorkItemFocus({ data, actionEnabled = false, onTransition }: WorkItemFocusProps) {
  const view = projection(data);
  const { workItem, commitment, approval } = view;
  const approvalTitle = humanReadable(approval.title, "Approval decision");
  const [capabilityToken, setCapabilityToken] = useState("");
  const [transitionNote, setTransitionNote] = useState("");
  const [submitting, setSubmitting] = useState<WorkItemTransitionStatus | null>(null);
  const [feedback, setFeedback] = useState<string | null>(null);
  const intents = useRef<Partial<Record<WorkItemTransitionStatus, { id: string; transitionedAt: string }>>>({});
  const canTransition = actionEnabled && Boolean(onTransition) && Boolean(workItem.transitionContext) && workItem.status !== "completed";
  const targets: Array<{ status: WorkItemTransitionStatus; label: string }> = workItem.status === "in_progress"
    ? [{ status: "in_review", label: "Submit result" }, { status: "blocked", label: "Mark blocked" }]
    : workItem.status === "in_review"
      ? [{ status: "completed", label: "Complete" }, { status: "in_progress", label: "Resume work" }]
      : workItem.status === "completed"
        ? []
        : [{ status: "in_progress", label: workItem.status === "blocked" ? "Resume work" : "Start preparation" }];
  async function transition(targetStatus: WorkItemTransitionStatus) {
    if (!canTransition || !onTransition || !capabilityToken.trim() || !transitionNote.trim()) return;
    const intent = intents.current[targetStatus] ?? {
      id: `action-browser-${workItem.id}-${targetStatus}-${crypto.randomUUID()}`,
      transitionedAt: new Date().toISOString(),
    };
    intents.current[targetStatus] = intent;
    setSubmitting(targetStatus);
    setFeedback(null);
    try {
      const command = buildWorkItemTransitionCommand({ workItem, targetStatus, note: transitionNote, commandId: intent.id, transitionedAt: intent.transitionedAt });
      const accepted = await onTransition(command, capabilityToken.trim());
      if (accepted) { setCapabilityToken(""); setTransitionNote(""); }
      setFeedback(accepted ? `WorkItem moved to ${humanReadable(targetStatus, targetStatus)} in Store truth.` : "Transition was not applied. Review the action error above and retry with the same intent.");
    } catch (error) {
      setFeedback(error instanceof Error ? error.message : String(error));
    } finally {
      setSubmitting(null);
    }
  }
  const unavailableReason = !actionEnabled
    ? commandUnavailable
    : !workItem.transitionContext
      ? "The current projection does not expose a complete work_item.transition contract."
      : workItem.status === "completed"
        ? "This WorkItem is completed and cannot be reopened by the V1 transition contract."
        : !capabilityToken.trim() || !transitionNote.trim()
          ? "Enter the session capability and a durable transition note."
          : undefined;
  const transitionControls = <div className="w-full max-w-lg space-y-2" aria-label="WorkItem transition controls" data-company-os-action-state={canTransition ? "available" : "unavailable"}><div className="grid gap-2 sm:grid-cols-2"><label className="text-xs font-medium text-muted-foreground">Session capability<input data-company-os-action-token type="password" autoComplete="off" value={capabilityToken} onChange={(event) => setCapabilityToken(event.target.value)} disabled={!canTransition} placeholder="Not stored" className="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground disabled:bg-muted" /></label><label className="text-xs font-medium text-muted-foreground">Transition note<input data-company-os-work-note value={transitionNote} onChange={(event) => setTransitionNote(event.target.value)} disabled={!canTransition} placeholder="Required for durable outcome" className="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground disabled:bg-muted" /></label></div><div className="flex flex-wrap gap-2">{targets.map((target) => { const approvalBlocked = target.status === "completed" && approval.status !== "approved"; const ready = canTransition && Boolean(capabilityToken.trim()) && Boolean(transitionNote.trim()) && !submitting && !approvalBlocked; return <GovernedActionButton key={target.status} label={submitting === target.status ? `${target.label}…` : target.label} reason={approvalBlocked ? "Every linked Approval must be approved before completion." : unavailableReason} disabled={!ready} onClick={() => void transition(target.status)} />; })}</div><p className="max-w-lg text-xs leading-5 text-muted-foreground">{feedback ?? unavailableReason ?? "The server validates lifecycle, responsibility, provenance, policy, scope and idempotency before appending the next WorkItem version."}</p></div>;
  const stateNotice = workItem.status === "completed" ? <><strong>Work completed.</strong> The durable result is linked; completion did not create a Payment or accept an execution run.</> : workItem.status === "in_review" ? <><strong>Result submitted for review.</strong> The accountable owner may complete it only after every linked Approval is approved.</> : workItem.status === "in_progress" ? <><strong>Preparation is in progress.</strong> The assignee can submit a durable result or record a blocker.</> : <><strong>Blocked by authorization, not execution.</strong> Preparation may continue within policy; filing and the linked commitment require human approval.</>;
  return <div data-company-os-ref={workItem.id} data-work-item-status={workItem.status}><PageFrame eyebrow="Work item" title={workItem.title} description="A linked business commitment with explicit responsibility, result provenance and governed lifecycle actions." action={transitionControls} context={<ContextRail><StatusTag status={workItem.status} /><Panel title="Source"><LinkedRecord recordRef={workItem.sourceDocument.id} label={workItem.sourceDocument.label} detail="Durable source context" /><LinkedRecord recordRef={view.typedApplication.id} label={view.typedApplication.label} detail={view.typedApplication.detail} /></Panel><Panel title="Last updated"><p className="text-sm">{displayTimestamp(workItem.updatedAt)}</p></Panel>{workItem.outcomeSummary && <Panel title="Latest outcome"><p className="text-sm leading-6">{workItem.outcomeSummary}</p></Panel>}</ContextRail>}>
    <div className="space-y-5"><DecisionNotice>{stateNotice}</DecisionNotice><Panel title="Evidence"><div className="space-y-1">{view.evidence.length > 0 ? view.evidence.map((evidence) => <LinkedRecord key={evidence.id} recordRef={evidence.id} label={evidence.label} detail={evidence.detail} />) : <p className="text-sm text-muted-foreground">No evidence is linked in this projection.</p>}</div></Panel><div className="grid gap-5 lg:grid-cols-2"><Panel title="Approval decision"><LinkedRecord wrapLabel recordRef={approval.id} label={approvalTitle} detail={`${humanReadable(approval.status, "Unknown")} · ${actorDescriptor(approval.requiredApprover)}`} /><p className="mt-3 break-words text-sm leading-6 text-muted-foreground">{humanReadable(approval.actionSummary, "No approval summary was supplied.")}</p></Panel><Panel title="Financial relation"><FinancialRecordCard record={commitment} /></Panel></div><Panel title="Responsibility"><WorkRoleTable workItem={workItem} /></Panel></div>
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
    ["Cost context", record.costContext?.label ?? "No Milestone or business context linked"],
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
  return <PageFrame
    eyebrow="Business module · proposed"
    title={view.businessModule.label}
    description="End-to-end trademark operations: source knowledge, accountable work, Human approval, evidence, and monetary effects remain one linked truth."
    dense
    action={<button type="button" disabled className="inline-flex h-10 cursor-not-allowed items-center gap-2 rounded-lg bg-primary px-4 text-sm font-semibold text-primary-foreground opacity-75"><Plus className="size-4" />New application</button>}
    context={<ContextRail label="Decision & control">
      <section className="rounded-xl border border-primary/35 bg-primary/[0.04] p-4" data-company-os-ref={view.approval.id}>
        <div className="flex items-start justify-between gap-3"><div><p className="text-[10px] font-semibold uppercase tracking-wider text-primary">Human decision</p><h2 className="company-editorial-title mt-2 text-xl">Approve filing commitment</h2></div><CircleDollarSign className="size-8 text-primary" /></div>
        <p className="mt-3 text-xs leading-5 text-muted-foreground">{view.approval.actionSummary}</p><p className="company-editorial-title mt-4 text-3xl">{view.commitment.amount}</p>
        <div className="mt-4"><p className="mb-2 text-[10px] uppercase tracking-wider text-muted-foreground">Required approver</p><ActorPill actor={view.approval.requiredApprover} /></div>
        <button type="button" disabled className="mt-4 h-10 w-full cursor-not-allowed rounded-lg bg-primary text-sm font-semibold text-primary-foreground opacity-80">Review and approve</button>
      </section>
      <Panel title="Operating team"><div className="space-y-3"><ActorPill actor={view.workItem.accountableOwner} />{view.workItem.assignees[0] && <ActorPill actor={view.workItem.assignees[0]} />}{view.workItem.reviewer && <ActorPill actor={view.workItem.reviewer} />}{view.workItem.contributors[0] && <ActorPill actor={view.workItem.contributors[0]} />}</div></Panel>
      <Panel title="Financial truth"><FinancialRecordCard record={view.commitment} /><p className="mt-3 text-xs text-muted-foreground">Payment · 0 recorded</p></Panel>
    </ContextRail>}
  >
    <div className="space-y-4" data-company-os-ref={view.businessModule.id}>
      <DecisionNotice><strong>Governance truth:</strong> this module is awaiting final approval and does not assert that it was created from an approved Module Design. Its current records remain visible and auditable.</DecisionNotice>
      <section className="rounded-xl border border-border bg-card/65 px-5 py-4"><div className="flex items-center justify-between gap-2">{[
        ["Prepare", true], ["Review", true], ["Approve", false], ["File", false], ["Monitor", false],
      ].map(([label, complete], index) => <div key={String(label)} className="flex min-w-0 flex-1 items-center"><div className="flex items-center gap-2"><span className={`grid size-8 place-items-center rounded-full border ${complete ? "border-status-good/40 bg-status-good/10 text-status-good" : index === 2 ? "border-primary/40 bg-primary/10 text-primary" : "border-border text-muted-foreground"}`}>{complete ? <CheckCircle2 className="size-4" /> : index === 2 ? <Clock3 className="size-4" /> : <FileCheck2 className="size-4" />}</span><span className={`hidden text-xs font-medium sm:block ${index === 2 ? "text-primary" : ""}`}>{String(label)}</span></div>{index < 4 && <span className="mx-3 h-px flex-1 bg-border" />}</div>)}</div></section>
      <Panel title="Current applications" action={<span className="text-xs text-muted-foreground">1 native record</span>}>
        <div className="overflow-x-auto"><table className="min-w-[720px] w-full text-left text-xs"><thead className="text-[9px] uppercase tracking-wider text-muted-foreground"><tr>{["Brand / Mark", "Application", "Jurisdiction", "Stage", "Approval", "Owner"].map((label) => <th key={label} className="border-b border-border px-3 py-2 font-semibold">{label}</th>)}</tr></thead><tbody><tr data-company-os-ref={view.typedApplication.id}><td className="px-3 py-3 font-semibold">Brand A</td><td className="px-3 py-3">{view.typedApplication.label}</td><td className="px-3 py-3">China</td><td className="px-3 py-3"><StatusTag status={view.workItem.status} /></td><td className="px-3 py-3 text-status-warn">Human decision</td><td className="px-3 py-3"><ActorPill actor={view.workItem.accountableOwner} compact /></td></tr></tbody></table></div>
      </Panel>
      <Panel title="Work ledger" action={<span className="text-xs text-muted-foreground">1 linked WorkItem</span>}>
        <article className="grid gap-3 rounded-lg border border-border bg-background/60 p-3 sm:grid-cols-[minmax(0,1fr)_9rem_11rem]" data-company-os-ref={view.workItem.id}><div><p className="text-sm font-semibold">{view.workItem.title}</p><p className="mt-1 text-xs text-muted-foreground">Legal · source-linked filing work</p></div><div><p className="text-[9px] uppercase tracking-wider text-muted-foreground">Assigned</p>{view.workItem.assignees[0] ? <ActorPill actor={view.workItem.assignees[0]} compact /> : <p className="text-xs">Unassigned</p>}</div><div className="flex items-center justify-end"><StatusTag status={view.workItem.status} /></div></article>
      </Panel>
      <Panel title="Knowledge & evidence" action={<Search className="size-4 text-muted-foreground" />}>
        <div className="divide-y divide-border">{view.evidence.map((record) => <div key={record.id} className="py-1.5"><LinkedRecord recordRef={record.id} label={record.label} detail={record.detail ?? "Durable execution evidence"} /></div>)}</div>
      </Panel>
    </div>
  </PageFrame>;
}
