import { useRef, useState } from "react";
import { AlertTriangle, Bot, BriefcaseBusiness, Building2, CheckCircle2, CircleDollarSign, Clock3, FileCheck2, KeyRound, Landmark, Library, Network, Plus, Route, Scale, Search, Send, ShieldCheck, Sparkles, Wrench } from "lucide-react";

import {
  ActorPill, ContextRail, DecisionNotice,
  FinancialRecordCard, GovernedActionButton, LinkedRecord, PageFrame, Panel, PolicyNote, RoleLine, StatusTag,
} from "./components";
import { prototypeTrademarkOperationsProjection } from "./fixture";
import { buildApprovalDecisionCommand } from "./approvalAction";
import { buildWorkItemTransitionCommand } from "./workItemAction";
import type { ActorSummary, ApprovalDecision, ApprovalDecisionCommand, OrganizationMembershipView, OrganizationUnitView, RelatedLink, StandingLinkConflict, TrademarkOperationsProjection, WorkItemTransitionCommand, WorkItemTransitionStatus, WorkItemView } from "./types";
import { ActorAvatar } from "../visuals";
import { ActivityStream, type WorkbenchActivityItem } from "@/components/workbench/activity/ActivityStream";
import { ContextModule, ContextRail as WorkbenchContextRail } from "@/components/workbench/context/ContextRail";
import { FocusHeader, FocusShell } from "@/components/workbench/layout/FocusShell";
import { Markdown } from "@/components/workbench/Markdown";
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

/** Bounds how many conflict entries render inline so a pathological store cannot blow up the page. */
const STANDING_LINK_CONFLICT_VISIBLE_CAP = 5;

/**
 * Withheld-participation warning: two or more StandingAgents declared the
 * same execution_agent_member_ref, so the Company OS snapshot withholds that
 * agent_member_id from standingAssignments rather than guessing an owner. An
 * empty/absent list renders nothing so the healthy dashboard is unchanged.
 */
function StandingLinkConflictBanner({ conflicts }: { conflicts: StandingLinkConflict[] }) {
  if (conflicts.length === 0) return null;
  const visible = conflicts.slice(0, STANDING_LINK_CONFLICT_VISIBLE_CAP);
  const hiddenCount = conflicts.length - visible.length;
  return (
    <div role="alert" data-standing-link-conflicts data-standing-link-conflict-count={conflicts.length} className="mb-4 space-y-3 rounded-lg border border-status-bad/30 bg-status-bad/[0.05] p-4">
      <div className="flex items-center gap-2 text-sm font-semibold text-status-bad"><AlertTriangle className="size-4 shrink-0" />Standing Agent link conflicts ({conflicts.length})</div>
      <p className="text-xs leading-5 text-muted-foreground">These execution_agent_member_ref links are claimed by more than one Standing Agent. Affected participation is withheld from Agent Team participation until a human resolves the conflict.</p>
      <ul className="space-y-2">
        {visible.map((conflict) => <li key={conflict.id} data-company-os-ref={conflict.id} className="rounded-md border border-status-bad/20 bg-background/70 p-3 text-xs leading-5">
          <p><span className="font-medium text-foreground">{conflict.agentMemberId}</span> is claimed by <span className="font-medium text-foreground">{conflict.standingAgentIds.join(", ")}</span></p>
          {conflict.affectedMemberRunIds.length > 0 && <p className="mt-1 text-muted-foreground">Affected MemberRuns: {conflict.affectedMemberRunIds.join(", ")}</p>}
          {conflict.resolutionHint && <code className="mt-1 block break-words font-mono text-[10px] text-muted-foreground">{conflict.resolutionHint}</code>}
        </li>)}
      </ul>
      {hiddenCount > 0 && <p className="text-xs font-medium text-status-bad">+{hiddenCount} more</p>}
    </div>
  );
}

export function OrganizationPage({ data, onSelectionChange }: OperationsPageProps & { onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
  const view = projection(data);
  const standingLinkConflicts = view.standingAssignmentConflicts ?? [];
  const explicitHumanLeads = view.organization.units
    .map((unit) => unit.humanLeadActorId ? view.actors[unit.humanLeadActorId] : undefined)
    .filter((actor, index, all): actor is ActorSummary => Boolean(actor) && all.findIndex((candidate) => candidate?.id === actor?.id) === index);
  const hasGovernanceProposal = Boolean(view.governanceProposal.id)
    && !view.governanceProposal.id.startsWith("unresolved");

  return <PageFrame dense eyebrow="Organization" title="Company OS" description="Exact Store-truth OrgUnit hierarchy, memberships, accountable leads, and durable execution identity bindings." action={<div className="flex flex-wrap gap-2"><button type="button" disabled title="A governed organization action requires an approved proposal." className="inline-flex min-h-10 cursor-not-allowed items-center gap-2 rounded-lg border border-primary/25 bg-primary/[0.07] px-4 py-2 text-sm font-medium text-primary"><Bot className="size-4" />Propose agent</button><button type="button" disabled title="A governed organization action requires an approved proposal." className="inline-flex min-h-10 cursor-not-allowed items-center gap-2 rounded-lg border border-border bg-card/80 px-4 py-2 text-sm font-medium text-muted-foreground"><Plus className="size-4" />Create org unit</button></div>} context={<ContextRail label="Organization truth"><PolicyNote>Hierarchy comes only from OrgUnit.parent_unit_id. Leads come only from explicit Human and Agent lead references; membership, names, runtime state, and display order never promote an actor.</PolicyNote>{explicitHumanLeads.length > 0 && <Panel title="Explicit Human leads"><div className="space-y-2">{explicitHumanLeads.map((actor) => <ActorPill key={actor.id} actor={actor} />)}</div></Panel>}<Panel title="Projection provenance"><p className="text-xs leading-5 text-muted-foreground">{view.organization.units.length} OrgUnits · {view.organization.memberships.length} memberships · {view.organization.rootUnitIds.length} roots</p></Panel><Panel title="Authority boundary"><div className="space-y-3 text-xs leading-5 text-muted-foreground"><p className="flex gap-2"><ShieldCheck className="mt-0.5 size-4 shrink-0 text-status-good" />Standing Agents may own and coordinate WorkItems only within explicit scope.</p><p className="flex gap-2"><Scale className="mt-0.5 size-4 shrink-0 text-primary" />Financial, legal, and organization-wide changes remain Human-governed.</p></div></Panel>{hasGovernanceProposal && <LinkedRecord wrapLabel recordRef={view.governanceProposal.id} label={view.governanceProposal.label} detail={view.governanceProposal.detail} icon={<Scale className="size-4" />} />}</ContextRail>}>
    <StandingLinkConflictBanner conflicts={standingLinkConflicts} />
    <OrganizationIntegrity findings={view.organization.integrityFindings} />
    <section aria-label="Organization forest" className="relative overflow-hidden rounded-2xl border border-border bg-card/70 p-4 shadow-sm sm:p-6" data-organization-root-count={view.organization.rootUnitIds.length}>
      <div className="pointer-events-none absolute -left-24 -top-24 size-72 rounded-full border border-primary/15" /><div className="pointer-events-none absolute -left-10 -top-10 size-44 rounded-full border border-primary/20" />
      {view.organization.rootUnitIds.length > 0
        ? <div className="relative space-y-5">{view.organization.rootUnitIds.map((unitId) => <OrganizationUnitBranch key={unitId} view={view} unitId={unitId} depth={0} onSelectionChange={onSelectionChange} />)}</div>
        : <div className="relative rounded-xl border border-dashed border-border bg-background/60 p-8 text-center"><Building2 className="mx-auto size-7 text-primary" /><h2 className="mt-3 font-semibold">No rooted OrgUnit forest</h2><p className="mt-1 text-sm text-muted-foreground">Store truth contains no root whose parent_unit_id is null.</p></div>}
    </section>
    {view.organization.unplacedUnitIds.length > 0 && <section aria-label="Unplaced organization units" className="mt-4 rounded-xl border border-status-bad/30 bg-status-bad/[0.035] p-4"><h2 className="text-sm font-semibold text-status-bad">Unplaced OrgUnits</h2><p className="mt-1 text-xs text-muted-foreground">These units are preserved but cannot be inserted into the rooted forest because their parent relation is missing or cyclic.</p><div className="mt-3 grid gap-3 sm:grid-cols-2">{view.organization.unplacedUnitIds.map((unitId) => { const unit = view.organization.units.find((candidate) => candidate.id === unitId); return unit ? <OrganizationUnitSummary key={unit.id} unit={unit} /> : null; })}</div></section>}
    {view.organization.unassignedActorIds.length > 0 && <details className="mt-4 rounded-xl border border-border bg-card/60 p-4"><summary className="cursor-pointer text-xs font-semibold text-muted-foreground">Actors without OrganizationMembership ({view.organization.unassignedActorIds.length})</summary><div className="mt-3 grid gap-2 sm:grid-cols-2">{view.organization.unassignedActorIds.map((actorId) => { const actor = view.actors[actorId]; return actor ? <ActorPill key={actor.id} actor={actor} /> : <code key={actorId} className="text-xs">{actorId}</code>; })}</div></details>}
  </PageFrame>;
}

function OrganizationIntegrity({ findings }: { findings: TrademarkOperationsProjection["organization"]["integrityFindings"] }) {
  const material = findings.filter((finding) => finding.severity !== "info");
  if (material.length === 0) return null;
  return <div role="alert" data-organization-integrity-count={material.length} className="mb-4 rounded-xl border border-status-warn/35 bg-status-warn/[0.05] p-4"><div className="flex items-center gap-2 text-sm font-semibold text-status-warn"><AlertTriangle className="size-4" />Organization integrity findings ({material.length})</div><ul className="mt-3 space-y-2">{material.map((finding) => <li key={finding.id} data-organization-integrity-kind={finding.kind} className="rounded-lg border border-border/70 bg-background/70 p-3 text-xs leading-5"><p>{finding.detail}</p><code className="mt-1 block break-words font-mono text-[10px] text-muted-foreground">{finding.id}</code></li>)}</ul></div>;
}

function OrganizationUnitBranch({ view, unitId, depth, onSelectionChange }: { view: TrademarkOperationsProjection; unitId: string; depth: number; onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
  const unit = view.organization.units.find((candidate) => candidate.id === unitId);
  if (!unit) return null;
  const memberships = view.organization.memberships.filter((membership) => membership.orgUnitId === unit.id);
  const children = view.organization.units.filter((candidate) => candidate.parentId === unit.id);
  const nestedPadding = depth === 0 ? "p-4 sm:p-5" : "p-3 sm:p-4";
  const childIndent = depth === 0
    ? "border-l border-primary/20 pl-2 sm:pl-3"
    : depth === 1
      ? "border-l border-border/70 pl-2"
      : "border-l-0 pl-0";
  return <article data-company-os-ref={unit.id} data-org-parent-unit-id={unit.parentId ?? ""} data-org-depth={depth} className={`min-w-0 rounded-2xl border border-border bg-background/65 shadow-sm ${nestedPadding}`}><header className="flex min-w-0 flex-wrap items-start justify-between gap-3"><div className="flex min-w-0 items-start gap-3"><span className="grid size-9 shrink-0 place-items-center rounded-xl border border-primary/20 bg-primary/[0.07] text-primary"><Building2 className="size-4" /></span><div className="min-w-0"><p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">OrgUnit · {unit.status ?? "status not supplied"}</p><h2 className="company-editorial-title mt-1 break-words text-xl">{unit.label}</h2>{unit.purpose && <p className="mt-1 max-w-2xl break-words text-xs leading-5 text-muted-foreground">{unit.purpose}</p>}<code className="mt-1 block break-all font-mono text-[9px] text-muted-foreground">{unit.id}</code></div></div><div className="shrink-0 text-right text-[10px] text-muted-foreground"><p>{memberships.length} memberships</p><p>{children.length} child units</p></div></header><ExplicitUnitLeads view={view} unit={unit} onSelectionChange={onSelectionChange} /><div className="mt-4 border-t border-border pt-4"><p className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">All memberships</p>{memberships.length > 0 ? <div data-organization-membership-grid data-org-depth={depth} className="grid min-w-0 gap-3 [grid-template-columns:repeat(auto-fit,minmax(min(100%,18rem),1fr))]">{memberships.map((membership) => { const actor = view.actors[membership.actorId]; return actor ? <OrgActorCard key={membership.id} view={view} actor={actor} membership={membership} variant={actor.kind === "external" ? "external" : "member"} onOpen={selectionForActor(actor, onSelectionChange)} /> : <UnresolvedMembership key={membership.id} membership={membership} />; })}</div> : <p className="rounded-lg border border-dashed border-border p-3 text-xs text-muted-foreground">No OrganizationMembership rows link actors to this unit.</p>}</div>{children.length > 0 && <div className={`mt-4 min-w-0 space-y-4 ${childIndent}`}>{children.map((child) => <OrganizationUnitBranch key={child.id} view={view} unitId={child.id} depth={depth + 1} onSelectionChange={onSelectionChange} />)}</div>}</article>;
}

function ExplicitUnitLeads({ view, unit, onSelectionChange }: { view: TrademarkOperationsProjection; unit: OrganizationUnitView; onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
  const leads = [
    unit.humanLeadActorId ? { label: "Explicit Human lead", actor: view.actors[unit.humanLeadActorId], actorId: unit.humanLeadActorId, variant: "owner" as const } : undefined,
    unit.agentLeadActorId ? { label: "Explicit Agent lead", actor: view.actors[unit.agentLeadActorId], actorId: unit.agentLeadActorId, variant: "lead" as const } : undefined,
  ].filter((lead): lead is NonNullable<typeof lead> => Boolean(lead));
  if (leads.length === 0) return <p className="mt-4 rounded-lg border border-dashed border-border px-3 py-2 text-xs text-muted-foreground">No explicit Human or Agent lead reference is recorded.</p>;
  return <div className="mt-4 grid min-w-0 gap-3 [grid-template-columns:repeat(auto-fit,minmax(min(100%,18rem),1fr))]">{leads.map((lead) => <div key={`${unit.id}:${lead.label}`} className="min-w-0"><p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{lead.label}</p>{lead.actor ? <OrgActorCard view={view} actor={lead.actor} variant={lead.variant} onOpen={selectionForActor(lead.actor, onSelectionChange)} /> : <p className="break-words rounded-lg border border-status-bad/30 bg-status-bad/[0.04] p-3 text-xs text-status-bad">Unresolved actor · {lead.actorId}</p>}</div>)}</div>;
}

function selectionForActor(actor: ActorSummary, onSelectionChange?: (selection: Partial<SelectionState>) => void): (() => void) | undefined {
  if (!onSelectionChange) return undefined;
  if (actor.kind === "human") return () => onSelectionChange({ surface: "organization", personId: actor.id });
  if (actor.kind === "standing_agent") return () => onSelectionChange({ surface: "organization", standingAgentId: actor.id });
  return undefined;
}

function UnresolvedMembership({ membership }: { membership: OrganizationMembershipView }) {
  return <article data-company-os-ref={membership.id} className="rounded-xl border border-status-bad/30 bg-status-bad/[0.04] p-3"><p className="text-xs font-semibold text-status-bad">Unresolved membership actor</p><code className="mt-1 block break-words text-[10px]">{membership.actorId}</code><p className="mt-1 text-[10px] text-muted-foreground">{membership.membershipRole ?? "role not supplied"}</p></article>;
}

function OrganizationUnitSummary({ unit }: { unit: OrganizationUnitView }) {
  return <div data-company-os-ref={unit.id} data-org-parent-unit-id={unit.parentId ?? ""} className="min-w-0 rounded-lg border border-border bg-background/70 p-3"><p className="break-words text-sm font-semibold">{unit.label}</p><code className="mt-1 block break-all text-[10px] text-muted-foreground">{unit.id}</code><p className="mt-1 break-all text-xs text-muted-foreground">Parent: {unit.parentId ?? "none"}</p></div>;
}

function OrgActorCard({ view, actor, membership, variant = "member", className, onOpen }: { view: TrademarkOperationsProjection; actor: ActorSummary; membership?: OrganizationMembershipView; variant?: "owner" | "lead" | "member" | "external"; className?: string; onOpen?: () => void }) {
  const proposed = actor.organizationRoleState === "proposed";
  const actorKind = actorSemanticKind(actor);
  const assignments = actor.executionAgentMemberRef
    ? [...new Map(
        (view.standingAssignments ?? [])
          .filter((assignment) => assignment.agentMemberId === actor.executionAgentMemberRef)
          .map((assignment) => [assignment.memberRunId, assignment]),
      ).values()]
    : [];
  return <article data-company-os-ref={membership?.id ?? actor.id} data-organization-actor-ref={actor.id} data-actor-kind={actorKind} data-actor-type={actorKind} className={`${variant === "lead" ? "border-status-good/45 bg-status-good/[0.045]" : variant === "external" ? "border-sky-500/35 bg-sky-500/[0.035]" : proposed ? "border-primary/45 border-dashed bg-primary/[0.035]" : "border-border bg-card/90"} min-w-0 rounded-xl border p-4 shadow-sm ${className ?? ""}`}>{onOpen ? <button type="button" onClick={onOpen} className="w-full min-w-0 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"><OrgActorCardBody actor={actor} variant={variant} proposed={proposed} /></button> : <OrgActorCardBody actor={actor} variant={variant} proposed={proposed} />}{membership && <div className="mt-3 min-w-0 border-t border-border pt-2 text-[10px] text-muted-foreground"><p className="break-words">{membership.membershipRole ?? "role not supplied"}{membership.titleOrFunction ? ` · ${membership.titleOrFunction}` : ""}{membership.status ? ` · ${membership.status}` : ""}</p><code className="mt-1 block break-all font-mono">{membership.id}</code></div>}{actor.kind === "standing_agent" && <div className="mt-3 min-w-0 rounded-lg border border-border/70 bg-background/55 p-2 text-[10px]"><p className="font-semibold text-foreground">AgentMember / runtime binding</p>{actor.executionAgentMemberRef ? <><code className="mt-1 block break-all font-mono text-muted-foreground">{actor.executionAgentMemberRef}</code>{assignments.length > 0 ? <div className="mt-2 space-y-1">{assignments.map((assignment) => <div key={assignment.id} data-company-os-ref={assignment.memberRunId} className="min-w-0 rounded-md border border-border/50 bg-background/60 p-1.5 text-muted-foreground"><p className="break-words">{assignment.nativeSession?.provider ?? "provider unavailable"} · {assignment.status}</p><code className="mt-0.5 block break-all font-mono">{assignment.memberRunId}</code></div>)}</div> : <p className="mt-1 break-words text-muted-foreground">No MemberRun in this execution snapshot explicitly binds that AgentMember.</p>}</> : <p className="mt-1 break-words text-muted-foreground">No execution_agent_member_ref is recorded.</p>}</div>}</article>;
}

function OrgActorCardBody({ actor, variant, proposed }: { actor: ActorSummary; variant: "owner" | "lead" | "member" | "external"; proposed: boolean }) {
  return <><div className="flex min-w-0 items-start gap-3"><ActorAvatar identity={`${actor.id} ${actor.role}`} name={actor.name} size={variant === "lead" || variant === "owner" ? "lg" : "md"} ring={variant === "owner" ? "warm" : variant === "lead" ? "good" : variant === "external" ? "external" : "neutral"} /><div className="min-w-0 flex-1"><div className="flex min-w-0 flex-wrap items-center gap-2"><h3 className={`${variant === "lead" || variant === "owner" ? "company-editorial-title text-xl" : "text-sm font-semibold"} min-w-0 break-words`}>{actor.name}</h3>{actor.availability === "available" && <span className="size-2 shrink-0 rounded-full bg-status-good" title="Explicitly reported available" />}{proposed && <StatusTag status="proposed" />}</div><p className="mt-1 break-words text-xs text-muted-foreground">{actor.kind === "human" ? "Human" : actor.kind === "external" ? "External Collaborator" : "Standing Agent"} · {actor.role}</p></div></div>{variant === "lead" && <div className="mt-4 grid grid-cols-3 divide-x divide-border rounded-lg border border-border bg-background/60 py-3 text-center text-xs"><div><strong className="block break-words text-base">{actor.availability === "available" ? "Available" : "Active"}</strong><span className="text-muted-foreground">Presence</span></div><div><strong className="block break-words text-base">{actor.unit ?? "Company"}</strong><span className="text-muted-foreground">Scope</span></div><div><strong className="block text-base">Lead</strong><span className="text-muted-foreground">Role</span></div></div>}</>;
}

export function HumanMemberFocus({ data }: OperationsPageProps) {
  const view = projection(data);
  const actor = actorOr(view, "actor-human-brand-owner", view.workItem.accountableOwner);
  return <PageFrame eyebrow="Human member" title={actor.name} description="Retains accountable human authority for governed commitments and sensitive company decisions." context={<ContextRail><Panel title="Authority"><p className="text-sm">Required human approver for governed commitments and sensitive actions.</p></Panel><Panel title="Membership"><ActorPill actor={actor} /></Panel></ContextRail>}>
    <div className="space-y-5"><DecisionNotice><strong>Decision required.</strong> The pending {view.commitment.amount} commitment requires {actor.name} as the named human approver.</DecisionNotice><Panel title="Accountable work"><div data-company-os-ref={view.workItem.id}><WorkRoleTable workItem={view.workItem} /></div></Panel><Panel title="Owned documents"><div className="space-y-1"><LinkedRecord recordRef={view.contentPlanDocument.id} label={view.contentPlanDocument.label} detail={view.contentPlanDocument.detail} /><LinkedRecord recordRef={view.sourceDocument.id} label={view.sourceDocument.label} detail="Accountable owner · Brand & IP" /></div></Panel><Panel title="Approvals"><LinkedRecord recordRef={view.approval.id} label={view.approval.title} detail="Required approver · decision requested" /></Panel><FinancialRecordCard record={view.commitment} /></div>
  </PageFrame>;
}

export function StandingAgentFocus({ data, actorId, onSelectionChange }: OperationsPageProps & { actorId?: string; onSelectionChange?: (selection: Partial<SelectionState>) => void }) {
  const view = projection(data);
  const actor = actorId ? view.actors[actorId] : undefined;
  if (!actor || actor.kind !== "standing_agent") {
    return <PageFrame eyebrow="Organization · Standing Agent" title="Standing Agent not found" description="The selected durable Standing Agent id is absent from Store truth. No name, provider, list order, or MemberRun similarity was used as a fallback."><div className="rounded-xl border border-dashed border-border bg-card/60 p-6"><p className="text-sm text-muted-foreground">Requested actor: <code>{actorId ?? "none"}</code></p></div></PageFrame>;
  }
  const authoredProposal = view.governanceProposal.proposedById === actor.id && !view.governanceProposal.id.startsWith("unresolved")
    ? view.governanceProposal
    : undefined;
  const memberships = view.organization.memberships.filter((membership) => membership.actorId === actor.id);
  const membershipUnits = memberships
    .map((membership) => view.organization.units.find((unit) => unit.id === membership.orgUnitId))
    .filter((unit): unit is OrganizationUnitView => Boolean(unit));
  const leadUnits = view.organization.units.filter((unit) => unit.agentLeadActorId === actor.id);
  const isLead = leadUnits.length > 0;
  const directReports = leadUnits
    .flatMap((unit) => view.organization.memberships.filter((membership) => membership.orgUnitId === unit.id))
    .map((membership) => view.actors[membership.actorId])
    .filter((candidate, index, all): candidate is ActorSummary => Boolean(candidate) && candidate.id !== actor.id && all.findIndex((entry) => entry?.id === candidate?.id) === index);
  const reportsTo = membershipUnits
    .map((unit) => unit.agentLeadActorId ? view.actors[unit.agentLeadActorId] : undefined)
    .find((candidate) => candidate?.id !== actor.id);
  const relatedItems = (view.workItems ?? [view.workItem]).filter((workItem) =>
    workItem.assignees.some((assignee) => assignee.id === actor.id)
    || workItem.accountableOwner.id === actor.id
    || workItem.contributors.some((contributor) => contributor.id === actor.id)
    || workItem.reviewer?.id === actor.id
    || workItem.submittedBy.id === actor.id,
  );
  const activeRelatedItems = relatedItems.filter((workItem) =>
    !["completed", "cancelled", "archived"].includes(workItem.status));
  const workResponsibility = (workItem: WorkItemView) => {
    if (workItem.accountableOwner.id === actor.id) return "Accountable";
    if (workItem.assignees.some((assignee) => assignee.id === actor.id)) return "Assignee";
    if (workItem.reviewer?.id === actor.id) return "Reviewer";
    if (workItem.contributors.some((contributor) => contributor.id === actor.id)) return "Contributor";
    return "Submitted by";
  };
  const actorAssignments = (view.assignments ?? []).filter((assignment) => assignment.recipient.id === actor.id);
  const linkedExecutionAssignments = actor.executionAgentMemberRef
    ? (view.standingAssignments ?? []).filter((assignment) => assignment.agentMemberId === actor.executionAgentMemberRef)
    : [];
  const recentExecutionAssignments = linkedExecutionAssignments.slice(-20);
  const activeExecutionAssignments = linkedExecutionAssignments.filter((assignment) =>
    assignment.sourceKind === "agent_team_assignment"
    && Boolean(assignment.correlationId)
    && !["completed", "failed", "stopped"].includes(assignment.status));
  const assignedWork = activeRelatedItems.length > 0 || activeExecutionAssignments.length > 0;
  const actorLinkConflicts = (view.standingAssignmentConflicts ?? []).filter((conflict) => conflict.standingAgentIds.includes(actor.id));
  const maintainedDocuments = (actor.maintainedDocumentRefs ?? []).map((recordRef) => {
    if (recordRef === view.sourceDocument.id) return view.sourceDocument;
    if (recordRef === view.contentPlanDocument.id) return view.contentPlanDocument;
    return { id: recordRef, label: recordRef, detail: "Maintained document reference" };
  });
  const activity: WorkbenchActivityItem[] = [
    ...recentExecutionAssignments.map((assignment) => ({
      id: `execution-${assignment.id}`,
      kind: "delegation" as const,
      glyph: "assignment" as const,
      tone: ["failed", "blocked"].includes(assignment.status) ? "bad" as const : assignment.status === "stopped" ? "info" as const : "running" as const,
      title: assignment.title,
      body: `Agent Team · ${assignment.role} · ${humanReadable(assignment.status, assignment.status)} · ${assignment.nativeSession?.provider ?? "provider pending"}`,
      actor: actor.name,
      timestamp: displayTimestamp(assignment.lastActivityAt ?? assignment.assignedAt),
      evidenceRefs: [assignment.correlationId, assignment.memberRunId, assignment.nativeSession?.native_session_id].filter((value): value is string => Boolean(value)),
    })),
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
    ...relatedItems.map((workItem) => ({
      id: `work-${workItem.id}`,
      kind: "action" as const,
      glyph: workItem.status === "completed" ? "complete" as const : "start" as const,
      tone: workItem.status === "completed" ? "good" as const : "running" as const,
      title: workItem.title,
      body: `${workResponsibility(workItem)} · ${humanReadable(workItem.status, workItem.status)}${workItem.outcomeSummary ? ` · ${workItem.outcomeSummary}` : ""}`,
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
        meta={<><Badge tone={actor.availability === "available" ? "good" : "muted"}>{actor.availability ?? "availability unknown"}</Badge><Badge tone="muted">{actor.role}</Badge>{membershipUnits.map((unit) => <Badge key={unit.id} tone="muted">{unit.label}</Badge>)}</>}
      />}
      context={<WorkbenchContextRail label="Organization context" quiet>
        <ContextModule title="Organization identity" kicker={`${memberships.length} explicit memberships`} icon={<Bot className="size-3.5" />} tone={actor.availability === "available" ? "good" : undefined}>
          <dl className="space-y-2 text-xs"><RailFact label="Units" value={membershipUnits.map((unit) => unit.label).join(", ") || "Not linked"} /><RailFact label="Reports to" value={reportsTo?.name ?? (isLead ? "Explicit Agent lead" : "Not recorded")} /><RailFact label="Capacity" value={assignedWork ? "Active assignment visible" : "No linked active assignment"} /></dl>
          {isLead && directReports.length > 0 && <div className="mt-3 border-t border-border/70 pt-3"><p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">Direct reports</p><div className="space-y-2">{directReports.map((report) => <ActorPill key={report.id} actor={report} compact />)}</div></div>}
        </ContextModule>
        <ContextModule title="Agent Team participation" kicker="Explicit identity links only" icon={<Network className="size-3.5" />} collapsible defaultOpen={linkedExecutionAssignments.length > 0}>
          {linkedExecutionAssignments.length > 0 ? <div className="space-y-2"><p className="text-[10px] text-muted-foreground">Showing {recentExecutionAssignments.length} of {linkedExecutionAssignments.length} chronological records.</p>{recentExecutionAssignments.map((assignment) => <button key={assignment.id} type="button" onClick={onSelectionChange ? () => onSelectionChange({ surface: "team", teamId: assignment.teamRunId, memberRunId: assignment.memberRunId, missionId: assignment.missionId, waveId: assignment.waveId }) : undefined} className="w-full rounded-lg border border-border bg-background/70 p-2 text-left transition hover:border-primary/35 hover:bg-primary/[0.035] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"><span className="block text-xs font-semibold">{assignment.role}</span><span className="mt-1 block text-[10px] text-muted-foreground">{humanReadable(assignment.status, assignment.status)} · {assignment.nativeSession?.provider ?? "runtime pending"}</span><code className="mt-1 block truncate font-mono text-[9px] text-muted-foreground">{assignment.memberRunId}</code></button>)}</div> : <p className="text-xs leading-5 text-muted-foreground">No MemberRun explicitly links this durable identity. Similar names or providers are not joined.</p>}
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
        <StandingLinkConflictBanner conflicts={actorLinkConflicts} />
        <section aria-labelledby="standing-agent-current-work" className="rounded-2xl border border-border bg-card/85 p-5 shadow-sm">
          <div className="flex items-center gap-3"><span className="grid size-9 place-items-center rounded-xl border border-primary/20 bg-primary/[0.07] text-primary"><BriefcaseBusiness className="size-4" /></span><div><h2 id="standing-agent-current-work" className="text-lg font-semibold tracking-tight">Current work</h2><p className="text-xs text-muted-foreground">Native WorkItems linked through accountable actor references</p></div></div>
          {assignedWork ? <div className="mt-4 space-y-3">{activeExecutionAssignments.map((assignment) => <button key={assignment.id} type="button" onClick={onSelectionChange ? () => onSelectionChange({ surface: "team", teamId: assignment.teamRunId, memberRunId: assignment.memberRunId, missionId: assignment.missionId, waveId: assignment.waveId }) : undefined} className="block w-full rounded-xl border border-status-good/25 bg-status-good/[0.035] p-4 text-left transition hover:border-status-good/45 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"><span className="text-[10px] font-semibold uppercase tracking-wider text-status-good">Agent Team · {humanReadable(assignment.status, assignment.status)}</span><span className="mt-1 block font-semibold">{assignment.title}</span><span className="mt-1 block text-xs text-muted-foreground">{assignment.role} · correlation {assignment.correlationId!}</span></button>)}{activeRelatedItems.map((workItem) => <div key={workItem.id} className="rounded-xl border border-primary/20 bg-primary/[0.035] p-4" data-company-os-ref={workItem.id}><div className="flex flex-wrap items-start justify-between gap-3"><div><p className="text-[10px] font-semibold uppercase tracking-wider text-primary">{workResponsibility(workItem)} · {workItem.status.replace(/_/g, " ")}</p>{onSelectionChange ? <button type="button" onClick={() => onSelectionChange({ surface: "work", workItemId: workItem.id })} className="mt-1 text-left font-semibold underline-offset-4 hover:text-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">{workItem.title}</button> : <h3 className="mt-1 font-semibold">{workItem.title}</h3>}{workItem.outcomeSummary && <p className="mt-1 text-xs leading-5 text-muted-foreground">{workItem.outcomeSummary}</p>}</div><StatusTag status={workItem.status} /></div><div className="mt-3"><LinkedRecord recordRef={workItem.sourceDocument.id} label={workItem.sourceDocument.label} detail="Source Document" onClick={onSelectionChange ? () => onSelectionChange({ surface: "docs", documentId: workItem.sourceDocument.id }) : undefined} /></div></div>)}</div> : <p className="mt-4 rounded-xl border border-dashed border-border p-4 text-sm text-muted-foreground">No active WorkItem or Agent Team assignment is linked. Completed participation remains in Activity and the context rail.</p>}
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
  const itemsById = new Map((view.workItems ?? []).map((item) => [item.id, item]));
  const columns = Object.entries(view.work.board);
  return <PageFrame eyebrow="Work" title="Milestones & WorkItems" description="One durable work ledger for development, legal, procurement, operations, and every other company commitment." context={<ContextRail label="Work context"><Panel title="Aggregate provenance"><p className="text-xs leading-5 text-muted-foreground">{view.work.provenance === "company_os.work" ? "Summary, board lanes, milestones, types, and workload come from company_os.work." : "Prototype compatibility data; no Store aggregate was supplied."}</p></Panel><Panel title="Ledger rules"><p className="text-xs leading-5 text-muted-foreground">Milestone grouping never replaces WorkItem identity. Requester, submitter, accountable owner, assignee, approval, and evidence remain separate facts.</p></Panel><Panel title="Integrity gaps"><p className="text-xs leading-5 text-muted-foreground">{view.work.summary.withoutMilestone} without Milestone · {view.work.summary.withoutBusinessLine} without business line · {view.work.summary.unassigned} unassigned</p></Panel></ContextRail>}>
    <div className="space-y-5">
      <section className="grid gap-3 sm:grid-cols-3"><WorkStat label="Open WorkItems" value={String(view.work.summary.active)} detail="company_os.work summary.active" tone="warm" /><WorkStat label="Waiting for approval" value={String(view.work.summary.waitingForApproval)} detail="company_os.work summary.waiting_for_approval" tone="warn" /><WorkStat label="Milestone gaps" value={String(view.work.summary.withoutMilestone)} detail={`${view.work.milestones.length} native Milestones supplied`} tone="quiet" /></section>
      {view.work.summary.total === 0
        ? <section className="rounded-2xl border border-dashed border-border bg-card/60 p-8 text-center"><h2 className="font-semibold">No WorkItems in aggregate truth</h2><p className="mt-2 text-sm text-muted-foreground">The UI does not fall back to a raw first row when company_os.work is empty.</p></section>
        : <section className="grid gap-4 xl:grid-cols-3">{columns.map(([status, workItemIds]) => <BoardColumn key={status} title={humanReadable(status, status)} items={workItemIds.map((id) => itemsById.get(id)).filter((item): item is WorkItemView => Boolean(item))} />)}</section>}
    </div>
  </PageFrame>;
}

function WorkStat({ label, value, detail, tone }: { label: string; value: string; detail: string; tone: "warm" | "warn" | "quiet" }) {
  return <div className={`${tone === "warm" ? "border-primary/25 bg-primary/[0.05]" : tone === "warn" ? "border-status-warn/30 bg-status-warn/[0.05]" : "border-border bg-card/70"} rounded-xl border p-4`}><p className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</p><p className="company-editorial-title mt-2 text-3xl">{value}</p><p className="mt-1 text-xs text-muted-foreground">{detail}</p></div>;
}

function BoardColumn({ title, items }: { title: string; items: WorkItemView[] }) {
  return <section className="min-h-64 rounded-lg border border-border bg-muted/35 p-3"><h2 className="mb-3 text-sm font-semibold">{title}<span className="ml-2 text-muted-foreground">{items.length}</span></h2><div className="space-y-3">{items.map((workItem) => <article key={workItem.id} data-company-os-ref={workItem.id} data-work-item-status={workItem.status} className="rounded-md border border-border bg-card p-3"><StatusTag status={workItem.status} /><h3 className="mt-3 text-sm font-semibold">{workItem.title}</h3><LinkedRecord recordRef={workItem.sourceDocument.id} label={workItem.sourceDocument.label} detail="Source document" /><dl className="mt-4 space-y-2 border-t border-border pt-3 text-sm"><BoardFact label="Requested by" actor={workItem.requestedBy} /><BoardFact label="Submitted by" actor={workItem.submittedBy} /><BoardFact label="Accountable" actor={workItem.accountableOwner} /><BoardFact label="Assignee" actor={workItem.assignees[0]} /><BoardFact label="Contributor" actor={workItem.contributors[0]} /><BoardFact label="Reviewer" actor={workItem.reviewer} /></dl></article>)}</div></section>;
}

function BoardFact({ label, actor }: { label: string; actor?: ActorSummary }) {
  const kind = actorSemanticKind(actor);
  return <div className="grid grid-cols-[6.5rem_minmax(0,1fr)] gap-2"><dt className="text-xs text-muted-foreground">{label}</dt><dd className="min-w-0 break-words text-sm leading-5 text-foreground" data-company-os-ref={actor?.id} data-actor-kind={kind} data-actor-type={kind}>{actorDescriptor(actor)}</dd></div>;
}

export function WorkItemFocus({ data, actionEnabled = false, onTransition }: WorkItemFocusProps) {
  const view = projection(data);
  const { workItem, linkedApproval, linkedCommitment, linkedTypedRecords = [], workAssignmentExecutionChains = [] } = view;
  const approvalTitle = linkedApproval ? humanReadable(linkedApproval.title, "Approval decision") : undefined;
  const [capabilityToken, setCapabilityToken] = useState("");
  const [transitionNote, setTransitionNote] = useState("");
  const [submitting, setSubmitting] = useState<WorkItemTransitionStatus | null>(null);
  const [feedback, setFeedback] = useState<string | null>(null);
  const intents = useRef<Partial<Record<WorkItemTransitionStatus, { id: string; transitionedAt: string }>>>({});
  if (view.work.selection.status !== "resolved") {
    const reason = view.work.selection.status === "not_found"
      ? `Requested WorkItem ${view.work.selection.requestedId ?? "unknown"} is absent from company_os.work.`
      : view.work.selection.status === "empty"
        ? "company_os.work contains no WorkItems."
        : "No WorkItem id was selected.";
    return <PageFrame eyebrow="Work item" title="WorkItem unavailable" description="The detail view fails closed instead of selecting the first WorkItem row."><div role="alert" className="rounded-xl border border-status-warn/35 bg-status-warn/[0.05] p-6"><p className="text-sm font-semibold">{reason}</p><p className="mt-2 text-xs leading-5 text-muted-foreground">Return to Work and choose an explicit WorkItem id from aggregate truth.</p></div></PageFrame>;
  }
  const terminal = ["completed", "cancelled", "archived"].includes(workItem.status);
  const canTransition = actionEnabled && Boolean(onTransition) && Boolean(workItem.transitionContext) && !terminal;
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
      : terminal
        ? `This WorkItem is ${humanReadable(workItem.status, workItem.status)} and cannot be reopened by the V1 transition contract.`
        : !capabilityToken.trim() || !transitionNote.trim()
          ? "Enter the session capability and a durable transition note."
          : undefined;
  const transitionControls = <div className="w-full max-w-lg space-y-2" aria-label="WorkItem transition controls" data-company-os-action-state={canTransition ? "available" : "unavailable"}><div className="grid gap-2 sm:grid-cols-2"><label className="text-xs font-medium text-muted-foreground">Session capability<input data-company-os-action-token type="password" autoComplete="off" value={capabilityToken} onChange={(event) => setCapabilityToken(event.target.value)} disabled={!canTransition} placeholder="Not stored" className="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground disabled:bg-muted" /></label><label className="text-xs font-medium text-muted-foreground">Transition note<input data-company-os-work-note value={transitionNote} onChange={(event) => setTransitionNote(event.target.value)} disabled={!canTransition} placeholder="Required for durable outcome" className="mt-1 h-9 w-full rounded-md border border-input bg-background px-3 text-sm text-foreground disabled:bg-muted" /></label></div><div className="flex flex-wrap gap-2">{targets.map((target) => { const approvalBlocked = target.status === "completed" && Boolean(linkedApproval && linkedApproval.status !== "approved"); const ready = canTransition && Boolean(capabilityToken.trim()) && Boolean(transitionNote.trim()) && !submitting && !approvalBlocked; return <GovernedActionButton key={target.status} label={submitting === target.status ? `${target.label}…` : target.label} reason={approvalBlocked ? "Every linked Approval must be approved before completion." : unavailableReason} disabled={!ready} onClick={() => void transition(target.status)} />; })}</div><p className="max-w-lg text-xs leading-5 text-muted-foreground">{feedback ?? unavailableReason ?? "The server validates lifecycle, responsibility, provenance, policy, scope and idempotency before appending the next WorkItem version."}</p></div>;
  const stateNotice = workItem.status === "completed"
    ? <><strong>Work completed.</strong> The durable result is linked; completion did not create a Payment or accept an execution run.</>
    : workItem.status === "in_review"
      ? <><strong>Result submitted for review.</strong> The accountable owner may complete it after linked approval requirements, if any, are satisfied.</>
      : workItem.status === "in_progress"
        ? <><strong>Work is in progress.</strong> The assignee can submit a durable result or record a blocker.</>
        : workItem.status === "blocked"
          ? <><strong>Work is blocked.</strong> Review its context, owner, and evidence rather than inferring an approval or financial cause.</>
          : workItem.status === "waiting_for_approval"
            ? linkedApproval
              ? <><strong>Waiting for linked approval.</strong> Execution may continue only within the declared policy boundary.</>
              : <><strong>Approval relation missing.</strong> The WorkItem says it is waiting, but no referenced Approval resolves in Store truth.</>
            : <><strong>{humanReadable(workItem.status, workItem.status)}.</strong> This WorkItem is recorded and awaits its next governed lifecycle decision.</>;
  return <div className="h-full min-h-0 overflow-hidden" data-company-os-ref={workItem.id} data-work-item-status={workItem.status}><PageFrame eyebrow="Work item" title={workItem.title} description="A linked business commitment with explicit responsibility, result provenance and governed lifecycle actions." action={transitionControls} context={<ContextRail><StatusTag status={workItem.status} /><Panel title="Source"><LinkedRecord recordRef={workItem.sourceDocument.id} label={workItem.sourceDocument.label} detail="Durable source context" />{linkedTypedRecords.map((record) => <LinkedRecord key={record.id} recordRef={record.id} label={record.label} detail={record.detail} />)}</Panel><Panel title="Last updated"><p className="text-sm">{displayTimestamp(workItem.updatedAt)}</p></Panel>{workItem.outcomeSummary && <Panel title="Latest outcome"><p className="text-sm leading-6">{workItem.outcomeSummary}</p></Panel>}</ContextRail>}>
    <div className="space-y-5">
      <DecisionNotice>{stateNotice}</DecisionNotice>
      <Panel title="Definition"><div className="space-y-4"><div><p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">Objective</p><p className="mt-1 break-words text-sm leading-6">{humanReadable(workItem.objective ?? "", "No objective supplied.")}</p></div>{workItem.description && <div><p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">Description</p><p className="mt-1 break-words text-sm leading-6 text-muted-foreground">{workItem.description}</p></div>}<div className="grid gap-4 md:grid-cols-3"><TextList title="Acceptance" empty="No acceptance criteria supplied." items={workItem.acceptanceCriteria ?? []} /><DetailList title="Context" empty="No context refs linked." items={workItem.contextRefs ?? []} /><DetailList title="Deliverables" empty="No deliverable refs linked." items={workItem.deliverableRefs ?? []} /></div></div></Panel>
      <Panel title="Durable Work truth"><p className="mb-3 text-xs leading-5 text-muted-foreground">These references and the lifecycle state belong to Company Store. Execution delivery, GitHub state, and passing checks do not accept or transition this WorkItem.</p><div className="space-y-1">{view.evidence.length > 0 ? view.evidence.map((evidence) => <LinkedRecord key={evidence.id} recordRef={evidence.id} label={evidence.label} detail={evidence.detail} />) : <p className="text-sm text-muted-foreground">No durable evidence reference is linked.</p>}</div></Panel>
      <Panel title="Computed execution & delivery evidence"><div className="space-y-3" data-work-execution-evidence>{workAssignmentExecutionChains.length > 0 ? workAssignmentExecutionChains.map((chain) => <div key={chain.assignmentId} className="rounded-md border border-border p-3" data-work-execution-chain={chain.assignmentId} data-link-status={chain.linkStatus}><div className="flex flex-wrap items-center gap-2"><strong className="text-sm">{chain.assignmentId}</strong><StatusTag status={chain.assignmentState} /><span className="text-xs text-muted-foreground">{humanReadable(chain.linkStatus, chain.linkStatus)}</span></div><p className="mt-2 text-xs leading-5 text-muted-foreground">{chain.detail}</p>{chain.teamMessage && <p className="mt-2 text-xs">TeamMessage <code>{chain.teamMessage.id}</code> · {humanReadable(chain.teamMessage.deliveryState, chain.teamMessage.deliveryState)}{chain.teamMessage.providerReceiptId ? ` · receipt ${chain.teamMessage.providerReceiptId}` : ""}</p>}{chain.memberRun && <p className="mt-1 text-xs">MemberRun <code>{chain.memberRun.id}</code> · session {chain.memberRun.nativeSessionAvailability}{chain.memberRun.nativeSessionId ? ` (${chain.memberRun.nativeSessionId})` : ""}</p>}{chain.handoffs.map((handoff) => <div key={handoff.id} className="mt-2 rounded border border-border/70 p-2 text-xs" data-handoff-result={handoff.result ?? "unavailable"}><p>Handoff <code>{handoff.id}</code> · {displayTimestamp(handoff.createdAt)} · {humanReadable(handoff.result ?? "", "result unavailable")}</p><DetailList title="Handoff evidence" empty="No Handoff evidence refs." items={handoff.evidenceRefs.map((id) => ({ id, label: id }))} /><details className="mt-2 rounded border border-border/70 bg-muted/20 px-2.5 py-1.5" aria-label={`Full Handoff body ${handoff.id}`} data-handoff-body-disclosure="collapsed-default"><summary className="cursor-pointer select-none font-medium text-muted-foreground hover:text-foreground">Show full Handoff body</summary><div className="mt-2 max-h-96 overflow-auto border-t border-border/60 pt-2"><Markdown source={handoff.body} compact /></div></details></div>)}{chain.externalObservations.map((observation) => <div key={observation.id} className="mt-2 rounded border border-border/70 p-2 text-xs" data-observation-freshness={observation.freshness} data-observation-repository={observation.repository} data-observation-pr={observation.pullRequestNumber}><p>{observation.label} · {humanReadable(observation.state ?? "", "state unavailable")} · {observation.freshness}</p><p className="mt-1 text-muted-foreground">{observation.repository ?? "repository unavailable"}{observation.pullRequestNumber ? ` · PR #${observation.pullRequestNumber}` : ""}{observation.headRef ? ` · ${observation.headRef}` : ""}{observation.baseRef ? ` → ${observation.baseRef}` : ""}</p>{observation.url && <p className="mt-1 break-all">{observation.url}</p>}{observation.headSha && <p className="mt-1">Head <code>{observation.headSha}</code></p>}<p className="mt-1 text-muted-foreground">Observed {observation.observedAt ? displayTimestamp(observation.observedAt) : "unavailable"} · source updated {observation.sourceUpdatedAt ? displayTimestamp(observation.sourceUpdatedAt) : "unavailable"} · source completed {observation.sourceCompletedAt ? displayTimestamp(observation.sourceCompletedAt) : "unavailable"}</p></div>)}</div>) : <p className="text-sm text-muted-foreground" data-link-status="unavailable">No explicitly linked execution chain is available. Names, providers, and timing are never used as fallback identity.</p>}</div></Panel>
      {linkedApproval || linkedCommitment ? <div className="grid gap-5 lg:grid-cols-2">{linkedApproval && <Panel title="Approval decision"><LinkedRecord wrapLabel recordRef={linkedApproval.id} label={approvalTitle ?? "Approval"} detail={`${humanReadable(linkedApproval.status, "Unknown")} · ${actorDescriptor(linkedApproval.requiredApprover)}`} /><p className="mt-3 break-words text-sm leading-6 text-muted-foreground">{humanReadable(linkedApproval.actionSummary, "No approval summary was supplied.")}</p></Panel>}{linkedCommitment && <Panel title="Financial relation"><FinancialRecordCard record={linkedCommitment} /></Panel>}</div> : <Panel title="Governed relations"><p className="text-sm leading-6 text-muted-foreground">No Approval or Finance record is linked to this WorkItem. The page keeps that absence visible instead of borrowing another business record.</p></Panel>}
      <Panel title="Responsibility"><WorkRoleTable workItem={workItem} /></Panel>
    </div>
  </PageFrame></div>;
}

function DetailList({ title, empty, items }: { title: string; empty: string; items: RelatedLink[] }) {
  return <div className="rounded-md border border-border bg-background/50 p-3"><p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{title}</p><div className="mt-2 space-y-1">{items.length > 0 ? items.map((item) => <LinkedRecord key={`${item.id}-${item.label}`} wrapLabel recordRef={item.id} label={item.label} detail={item.detail} />) : <p className="text-xs leading-5 text-muted-foreground">{empty}</p>}</div></div>;
}

function TextList({ title, empty, items }: { title: string; empty: string; items: string[] }) {
  return <div className="rounded-md border border-border bg-background/50 p-3"><p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{title}</p><ul className="mt-2 space-y-1">{items.length > 0 ? items.map((item, index) => <li key={`${title}-${index}`} className="text-xs leading-5 text-foreground">{item}</li>) : <li className="text-xs leading-5 text-muted-foreground">{empty}</li>}</ul></div>;
}

function WorkRoleTable({ workItem }: { workItem: WorkItemView }) {
  return <div className="divide-y divide-border"><RoleLine label="Requested by" actor={workItem.requestedBy} /><RoleLine label="Submitted by" actor={workItem.submittedBy} /><RoleLine label="Accountable owner" actor={workItem.accountableOwner} /><RoleLine label="Assignee" actor={workItem.assignees[0]} /><RoleLine label="Contributor" actor={workItem.contributors[0]} /><RoleLine label="Reviewer" actor={workItem.reviewer} /><RoleLine label="Legal reviewer" actor={workItem.legalReviewer} /><RoleLine label="Approver" actor={workItem.approver} /></div>;
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
      context={<ContextRail><StatusTag status={approval.status} /><Panel title="Expires"><p className="text-sm">{approval.expiresAt ? displayTimestamp(approval.expiresAt) : "No expiry recorded"}</p></Panel><Panel title="Policy"><p className="text-xs leading-5 text-muted-foreground">Human approval for sensitive financial or governed effects</p></Panel></ContextRail>}
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
  const proposedHomes = proposedAgent
    ? view.organization.memberships
      .filter((membership) => membership.actorId === proposedAgent.id)
      .map((membership) => view.organization.units.find((unit) => unit.id === membership.orgUnitId)?.label)
      .filter((label): label is string => Boolean(label))
    : [];
  return <PageFrame eyebrow="Governance proposal" title={proposalTitle} description="A proposal joins document architecture, organization capacity, work and financial controls without creating authority by itself." context={<ContextRail><StatusTag status="awaiting_final_approval" /><Panel title="Proposed by"><ActorPill actor={proposer} /></Panel><Panel title="Recorded organization homes"><p className="text-sm">{proposedHomes.join(", ") || "No OrganizationMembership recorded"}</p><p className="mt-1 text-xs text-muted-foreground">Proposed role · {proposedAgent?.role ?? "No role specified"}</p></Panel></ContextRail>}>
    <div className="space-y-5" data-company-os-ref={view.governanceProposal.id}><DecisionNotice><strong>Awaiting final approval.</strong> The module and proposed role remain governed changes. Human approval is required before any sensitive effect is authorized.</DecisionNotice><Panel title="Impact surfaces"><div className="grid gap-3 md:grid-cols-2"><ImpactSurface label="Business module" link={view.businessModule} /><ImpactSurface label="Application record" link={view.typedApplication} /><ImpactSurface label="Linked work" link={{ id: view.workItem.id, label: view.workItem.title, detail: `Assignee · ${actorDescriptor(proposedAgent)}` }} /><ImpactSurface label="Financial commitment" financialRecord={view.commitment} /></div></Panel><Panel title="Review participants"><div className="divide-y divide-border"><RoleLine label="Accountable owner" actor={view.workItem.accountableOwner} /><RoleLine label="Finance reviewer" actor={view.approval.financeReviewer} /><RoleLine label="Legal reviewer" actor={view.approval.legalReviewer} /></div></Panel><Panel title="Governed actions"><div className="flex flex-wrap gap-2"><GovernedActionButton label="Approve proposal" reason={commandUnavailable} /><GovernedActionButton label="Request changes" reason={commandUnavailable} /><GovernedActionButton label="Reject proposal" reason={commandUnavailable} /></div><p className="mt-3 text-xs leading-5 text-muted-foreground">{commandUnavailable}</p></Panel></div>
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
