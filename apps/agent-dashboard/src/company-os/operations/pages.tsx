import type { ReactNode } from "react";
import { Bot, Building2, FileText, Landmark, Plus, Scale, Send, Tag } from "lucide-react";

import {
  ActorPill, ContextRail, DecisionNotice,
  FinancialRecordCard, GovernedActionButton, LinkedRecord, PageFrame, Panel, PolicyNote, RoleLine, StatusTag,
} from "./components";
import { prototypeTrademarkOperationsProjection } from "./fixture";
import type { ActorSummary, TrademarkOperationsProjection, WorkItemView } from "./types";

type OperationsPageProps = { data?: TrademarkOperationsProjection };

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
  const otherGovernanceActors = governanceActors.filter((actor) => actor.id !== governanceLead?.id);
  const secondaryUnits = view.organization.units.filter((unit) => unit.id !== view.organization.company.id && unit.id !== brandUnit.id && unit.id !== governanceUnit?.id);

  return <PageFrame dense eyebrow="Organization" title="Organization" description="People and Standing Agents working as one company. Membership, authority, and external scope are explicit company facts." action={<div className="flex flex-wrap gap-2"><button type="button" disabled title="A governed organization action requires an approved proposal." className="inline-flex min-h-10 cursor-not-allowed items-center gap-2 rounded-md border border-border bg-card px-3 py-2 text-sm font-medium text-muted-foreground"><Bot className="size-4" />Propose agent</button><button type="button" disabled title="A governed organization action requires an approved proposal." className="inline-flex min-h-10 cursor-not-allowed items-center gap-2 rounded-md border border-border bg-card px-3 py-2 text-sm font-medium text-muted-foreground"><Plus className="size-4" />Create org unit</button></div>} context={<ContextRail label="Governance proposal"><PolicyNote>Organization changes are proposed and reviewed. This read-only view cannot grant authority, legal access, or financial permissions.</PolicyNote><LinkedRecord wrapLabel recordRef={view.governanceProposal.id} label={view.governanceProposal.label} detail={view.governanceProposal.detail} icon={<Scale className="size-4" />} /></ContextRail>}>
    <div className="space-y-5">
      <section aria-label="Organization tree" className="rounded-lg border border-border bg-card p-3 sm:p-4">
        <div className="mx-auto max-w-4xl">
          <OrganizationNode icon={<Building2 className="size-5" />} label={view.organization.company.label} recordRef={view.organization.company.id} className="mx-auto max-w-[17rem]" />
          <div className="mx-auto h-3 w-px bg-border" aria-hidden />
          <div className="ml-0 border-l border-border pl-3 sm:ml-24 sm:pl-8">
            <OrganizationNode icon={<Tag className="size-5" />} label={brandUnit.label} recordRef={brandUnit.id} className="max-w-[17rem]" />
            <div className="ml-5 mt-2 border-l border-border pl-4 sm:ml-10 sm:pl-7">
              {brandActors.length > 0 ? brandActors.map((actor) => <OrganizationMember key={actor.id} actor={actor} linkedDocument={actor.id === view.workItem.assignees[0]?.id ? view.sourceDocument : undefined} />) : <p className="py-4 text-sm text-muted-foreground">No explicitly projected members in this unit.</p>}
            </div>
          </div>
        </div>
      </section>

      {governanceUnit && <section aria-label={`${governanceUnit.label} members`} className="rounded-lg border border-border bg-card p-4 sm:p-6"><div className="mb-3 flex items-center gap-2 text-sm font-semibold"><Landmark className="size-4 text-primary" />{governanceUnit.label}</div>{governanceLead ? <OrganizationMember actor={governanceLead} availabilityNote /> : <p className="text-sm text-muted-foreground">No explicitly projected governance member.</p>}{otherGovernanceActors.length > 0 && <details className="mt-4 border-t border-border pt-3"><summary className="cursor-pointer text-xs font-medium text-muted-foreground">Other explicit governance members ({otherGovernanceActors.length})</summary><div className="mt-3 space-y-3">{otherGovernanceActors.map((actor) => <OrganizationMember key={actor.id} actor={actor} />)}</div></details>}</section>}

      {secondaryUnits.length > 0 && <details className="rounded-lg border border-border bg-card p-4"><summary className="cursor-pointer text-sm font-medium">Other explicit organization units ({secondaryUnits.length})</summary><div className="mt-4 grid gap-3 sm:grid-cols-2">{secondaryUnits.map((unit) => <div key={unit.id} data-company-os-ref={unit.id} className="rounded-md border border-border p-3"><p className="text-sm font-medium">{unit.label}</p><p className="mt-1 text-xs text-muted-foreground">{membersForUnit(view, unit.id).length} explicitly projected member(s)</p></div>)}</div></details>}
    </div>
  </PageFrame>;
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

export function StandingAgentFocus({ data }: OperationsPageProps) {
  const view = projection(data);
  const actor = actorOr(view, "actor-agent-document-architecture", view.actorList[0] ?? view.workItem.submittedBy);
  const authoredProposal = view.governanceProposal.proposedById === actor.id && !view.governanceProposal.id.startsWith("unresolved")
    ? view.governanceProposal
    : undefined;
  return <PageFrame eyebrow="Standing Agent" title={actor.name} description="A durable organization role with documented collaboration. Execution attempts and hidden reasoning do not define membership." context={<ContextRail label="Organization context"><Panel title="Status"><ActorPill actor={actor} />{actor.availability === "available" ? <p className="mt-3 inline-flex items-center gap-2 rounded-md border border-status-good/30 bg-status-good/10 px-2 py-1 text-xs font-medium text-status-good"><span className="size-2 rounded-full bg-status-good" />Available · explicitly reported</p> : <p className="mt-3 text-xs leading-5 text-muted-foreground">Availability has not been explicitly reported.</p>}</Panel><Panel title="Membership"><p className="text-sm">{actor.unit ?? "No organization unit linked"}</p><p className="mt-1 text-xs text-muted-foreground">{actor.role}</p></Panel><Panel title="Related structure"><LinkedRecord wrapLabel recordRef={view.businessModule.id} label={humanReadable(view.businessModule.label, "Business module")} detail={view.businessModule.detail} /></Panel><Panel title="Authority boundary"><p className="text-xs leading-5 text-muted-foreground">This role may propose document structure. Authority changes remain governed company actions.</p></Panel></ContextRail>}>
    <section aria-label="Standing Agent collaboration" className="flex min-h-[34rem] flex-col overflow-hidden rounded-lg border border-border bg-card"><header className="flex items-center justify-between gap-3 border-b border-border px-4 py-3"><div><h2 className="text-sm font-semibold">Collaboration</h2><p className="mt-1 text-xs text-muted-foreground">Documented updates only · no hidden reasoning or runtime transcript</p></div>{actor.availability === "available" && <span className="inline-flex items-center gap-2 text-xs font-medium text-status-good"><span className="size-2 rounded-full bg-status-good" />Available</span>}</header><div className="min-h-0 flex-1 space-y-4 p-4">{authoredProposal ? <article className="max-w-3xl rounded-lg border border-border bg-background p-4" data-company-os-ref={authoredProposal.id}><div className="flex items-start justify-between gap-3"><ActorPill actor={actor} compact /><span className="text-xs text-muted-foreground">Documented proposal</span></div><div className="mt-3"><LinkedRecord wrapLabel recordRef={authoredProposal.id} label={humanReadable(authoredProposal.label, "Governance proposal")} detail={authoredProposal.detail} /><p className="mt-2 text-sm leading-6 text-muted-foreground">Proposed company structure for a governed review. The linked module remains related structure, not a second authored activity.</p></div></article> : <p className="max-w-xl rounded-md border border-dashed border-border p-4 text-sm leading-6 text-muted-foreground">No documented collaboration entry is linked to this Standing Agent in the current projection.</p>}</div><form className="border-t border-border p-3" aria-label="Message composer"><label className="sr-only" htmlFor="standing-agent-message">Message {actor.name}</label><div className="flex items-end gap-2"><textarea id="standing-agent-message" disabled rows={2} placeholder={`Message ${actor.name}…`} aria-describedby="standing-agent-message-reason" className="min-h-12 flex-1 resize-none rounded-md border border-input bg-muted px-3 py-2 text-sm text-muted-foreground" /><button type="submit" disabled title={commandUnavailable} aria-label={`Send message. Unavailable: ${commandUnavailable}`} className="grid size-10 shrink-0 cursor-not-allowed place-items-center rounded-md bg-muted text-muted-foreground"><Send className="size-4" /></button></div><p id="standing-agent-message-reason" className="mt-2 text-xs leading-5 text-muted-foreground">{commandUnavailable}</p></form></section>
  </PageFrame>;
}

export function WorkboardPage({ data }: OperationsPageProps) {
  const view = projection(data);
  return <PageFrame eyebrow="Work" title="Workboard" description="WorkItems are durable company commitments. The board groups the same records without reducing responsibility to a chat or run state." context={<ContextRail><Panel title="Board rules"><p className="text-xs leading-5 text-muted-foreground">Ownership, requested-by and approval state remain separate fields. This board does not infer any of them.</p></Panel><LinkedRecord recordRef={view.sourceDocument.id} label={view.sourceDocument.label} detail="Source document" /></ContextRail>}>
    <div className="grid gap-4 lg:grid-cols-[minmax(8rem,0.65fr)_minmax(24rem,1.7fr)_minmax(8rem,0.65fr)]"><BoardColumn title="In progress" items={[]} approval={view.approval} /><BoardColumn title="Waiting for approval" items={view.workItem.status === "waiting_for_approval" ? [view.workItem] : []} approval={view.approval} /><BoardColumn title="Completed" items={view.workItem.status === "completed" ? [view.workItem] : []} approval={view.approval} /></div>
  </PageFrame>;
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

export function ApprovalFocus({ data }: OperationsPageProps) {
  const view = projection(data);
  const { approval, commitment } = view;
  const approvalTitle = humanReadable(approval.title, "Approval decision");
  const decisionControls = <div className="space-y-2" aria-label="Approval decision controls"><div className="flex flex-wrap gap-2"><GovernedActionButton label="Approve" reason={commandUnavailable} /><GovernedActionButton label="Request changes" reason={commandUnavailable} /><GovernedActionButton label="Reject" reason={commandUnavailable} /></div><p className="max-w-sm text-xs leading-5 text-muted-foreground">{commandUnavailable}</p></div>;
  return (
    <PageFrame
      eyebrow="Approval"
      title={approvalTitle}
      description="A formal authorization record. A review, activity event or Agent recommendation cannot substitute for it."
      action={decisionControls}
      context={<ContextRail><StatusTag status={approval.status} /><Panel title="Expires"><p className="text-sm">{approval.expiresAt ? displayTimestamp(approval.expiresAt) : "No expiry recorded"}</p></Panel><Panel title="Policy"><p className="text-xs leading-5 text-muted-foreground">Human approval for financial and legal submission</p></Panel></ContextRail>}
    >
      <div className="space-y-5" data-company-os-ref={approval.id}>
        <DecisionNotice><strong>Human action required.</strong> {actorDescriptor(approval.requiredApprover)} is the required approver; no payment is authorized or recorded by this pending approval.</DecisionNotice>
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
  return <PageFrame eyebrow="Business module · proposed" title={view.businessModule.label} description="A proposed module structure for Brand & IP. This view does not assert that it was created from an approved Module Design." context={<ContextRail><StatusTag status="awaiting_final_approval" /><Panel title="Home unit"><p className="text-sm">{view.organization.brandUnit.label}</p></Panel><Panel title="Owner"><ActorPill actor={view.workItem.accountableOwner} /></Panel></ContextRail>}>
    <div className="space-y-5" data-company-os-ref={view.businessModule.id}><DecisionNotice><strong>Module proposal pending.</strong> The module is awaiting final approval; the Trademark Agent role is proposed rather than active organization capacity.</DecisionNotice><nav aria-label="Module sections" className="flex gap-1 border-b border-border pb-2 text-sm"><button className="rounded-md bg-muted px-3 py-1.5 font-medium" type="button">Overview</button><button className="rounded-md px-3 py-1.5 text-muted-foreground hover:bg-muted" type="button">Applications</button><button className="rounded-md px-3 py-1.5 text-muted-foreground hover:bg-muted" type="button">Work</button><button className="rounded-md px-3 py-1.5 text-muted-foreground hover:bg-muted" type="button">Finance</button></nav><div className="grid gap-5 lg:grid-cols-2"><Panel title="Application"><LinkedRecord recordRef={view.typedApplication.id} label={view.typedApplication.label} detail={view.typedApplication.detail} /></Panel><Panel title="Linked work"><LinkedRecord recordRef={view.workItem.id} label={view.workItem.title} detail={`Assignee · ${view.workItem.assignees[0]?.name ?? "Unassigned"}`} /></Panel><Panel title="Finance"><FinancialRecordCard record={view.commitment} /></Panel><Panel title="Participants"><div className="space-y-3"><ActorPill actor={view.workItem.accountableOwner} />{view.workItem.assignees[0] && <ActorPill actor={view.workItem.assignees[0]} />}{view.workItem.contributors[0] && <ActorPill actor={view.workItem.contributors[0]} />}</div></Panel><Panel title="Governance"><LinkedRecord recordRef={view.governanceProposal.id} label={view.governanceProposal.label} detail={view.governanceProposal.detail} /></Panel></div></div>
  </PageFrame>;
}
