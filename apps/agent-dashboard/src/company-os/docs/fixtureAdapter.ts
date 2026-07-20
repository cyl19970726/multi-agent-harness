import type {
  CompanyOsDocumentPageData,
  CompanyOsHomeData,
  CompanyOsLink,
  CompanyOsStructuredViewData,
  CompanyOsWorkspaceData,
} from "./types";

type JsonRecord = Record<string, unknown>;
type Projection = {
  workspace: CompanyOsWorkspaceData;
  document: CompanyOsDocumentPageData;
  moduleView: CompanyOsStructuredViewData;
  home: CompanyOsHomeData;
};

function items(value: unknown): JsonRecord[] {
  return Array.isArray(value)
    ? value
        .filter((entry): entry is JsonRecord => Boolean(entry) && typeof entry === "object")
        .map((entry) => {
          const nested = entry.record;
          return nested && typeof nested === "object" && !Array.isArray(nested)
            ? { ...(nested as JsonRecord), ...entry }
            : entry;
        })
    : [];
}

function strings(value: unknown): string[] {
  return Array.isArray(value) ? value.map((entry) => text(entry)).filter(Boolean) : [];
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function refId(value: unknown): string {
  if (typeof value === "string") return value;
  if (!value || typeof value !== "object" || Array.isArray(value)) return "";
  const ref = value as JsonRecord;
  return text(ref.actor_id) || text(ref.id);
}

function field(entry: JsonRecord | undefined, key: string): unknown {
  if (!entry) return undefined;
  if (entry[key] !== undefined) return entry[key];
  const fields = entry.fields;
  return fields && typeof fields === "object" && !Array.isArray(fields)
    ? (fields as JsonRecord)[key]
    : undefined;
}

function record(records: JsonRecord[], id: unknown): JsonRecord | undefined {
  const resolved = text(id);
  return resolved ? records.find((entry) => entry.id === resolved) : undefined;
}

function refs(root: JsonRecord, page: string): string[] {
  const slices = root.page_slices;
  if (!slices || typeof slices !== "object") return [];
  const slice = (slices as JsonRecord)[page];
  return slice && typeof slice === "object"
    ? strings((slice as JsonRecord).required_refs)
    : [];
}

function firstReferenced(records: JsonRecord[], ids: string[]): JsonRecord | undefined {
  return ids.map((id) => record(records, id)).find(Boolean);
}

function distinct<T>(values: T[]): T[] {
  return [...new Set(values)];
}

function kindForActor(actor: JsonRecord): CompanyOsLink["actorType"] {
  const kind = text(actor.actor_type).toLowerCase();
  if (kind === "human") return "Human";
  if (kind === "standing agent" || kind === "agent") return "Standing Agent";
  if (kind === "external") return "External";
  return "Service";
}

function actorLink(actors: JsonRecord[], id: unknown): CompanyOsLink | undefined {
  const actor = record(actors, refId(id));
  return actor
    ? { id: text(actor.id), label: text(actor.display_name, "Unnamed actor"), kind: "actor", actorType: kindForActor(actor) }
    : undefined;
}

function documentLink(entry: JsonRecord | undefined): CompanyOsLink | undefined {
  return entry ? { id: text(entry.id), label: text(entry.title, "Untitled document"), kind: "document" } : undefined;
}

function linkEntries(values: Array<CompanyOsLink | undefined>): CompanyOsLink[] {
  return values.filter((value): value is CompanyOsLink => Boolean(value?.id));
}

function humanize(value: unknown): string {
  const raw = text(value);
  if (!raw) return "";
  return raw.replace(/[_-]+/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function humanTimestamp(value: unknown): string | undefined {
  const raw = text(value);
  const parsed = raw.startsWith("unix-ms:") ? Number(raw.slice("unix-ms:".length)) : Date.parse(raw);
  if (!raw || !Number.isFinite(parsed)) return raw || undefined;
  return new Intl.DateTimeFormat("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(parsed);
}

function facts(root: JsonRecord, page: string): string[] {
  const slices = root.page_slices;
  if (!slices || typeof slices !== "object") return [];
  const slice = (slices as JsonRecord)[page];
  return slice && typeof slice === "object" ? strings((slice as JsonRecord).required_facts) : [];
}

function isReadableSentence(value: unknown): boolean {
  const raw = text(value).trim();
  return Boolean(raw)
    && !/^[a-z][a-z0-9]*(?:[._:-][a-z0-9-]+)+$/i.test(raw)
    && !/(?:^|\s)[a-z][a-z0-9]*(?:[._][a-z0-9-]+)+(?:\s|$)/i.test(raw);
}

function approvalTitle(approval: JsonRecord | undefined, financial: JsonRecord | undefined, work: JsonRecord | undefined): string {
  const candidate = text(approval?.title).trim();
  if (isReadableSentence(candidate)) return candidate;
  const financialName = text(financial?.display_name).trim();
  if (financialName) return `Review ${financialName}`;
  const workTitle = text(work?.title).trim();
  return workTitle ? `Review ${workTitle}` : "Review approval";
}

function approvalSummary(
  approval: JsonRecord | undefined,
  title: string,
  financial: JsonRecord | undefined,
  work: JsonRecord | undefined,
): string | undefined {
  const candidate = text(approval?.action_summary).trim();
  const normalizedTitle = title.toLocaleLowerCase().replace(/[^a-z0-9]+/g, "");
  const normalizedCandidate = candidate.toLocaleLowerCase().replace(/[^a-z0-9]+/g, "");
  if (isReadableSentence(candidate) && normalizedCandidate !== normalizedTitle) return candidate;
  const financialName = text(financial?.display_name).trim();
  const workTitle = text(work?.title).trim();
  if (financialName && workTitle) return `Review ${financialName} before ${workTitle} can continue.`;
  if (financialName) return `Review the requested ${financialName}.`;
  if (workTitle) return `Review the decision needed for ${workTitle}.`;
  return undefined;
}

/**
 * Converts an arbitrary Company OS read projection into Docs presentation data.
 * It performs no store access, persistence, status inference, or fallback data
 * fabrication. Empty projections deliberately remain empty.
 */
export function adaptCompanyOsDocsProjection(input: unknown, selected: { documentId?: string; moduleId?: string } = {}): Projection {
  const root = (input && typeof input === "object" ? input : {}) as JsonRecord;
  const actors = items(root.actors);
  const documents = items(root.documents);
  const typedRecords = items(root.typed_records);
  const workItems = items(root.work_items);
  const financialRecords = items(root.financial_records);
  const approvals = items(root.approvals);
  const modules = items(root.business_modules);
  const proposals = [
    ...items(root.governance_proposals),
    ...typedRecords.filter((entry) => text(entry.record_type).toLowerCase() === "governance_proposal"),
  ];
  const fixtureId = text(root.fixture_id) || undefined;

  const workspaceRefs = refs(root, "docs-workspace");
  const focusRefs = refs(root, "document-focus");
  const moduleRefs = refs(root, "business-module-focus");
  const homeRefs = refs(root, "home");
  const focusFacts = facts(root, "document-focus");
  const workspaceDocument = firstReferenced(documents, workspaceRefs) ?? documents[0];
  const focusDocument = record(documents, selected.documentId) ?? firstReferenced(documents, focusRefs) ?? workspaceDocument ?? documents[0];
  const work = workItems.find((entry) => text(entry.source_document_ref) === text(workspaceDocument?.id))
    ?? firstReferenced(workItems, distinct([...focusRefs, ...moduleRefs]))
    ?? workItems[0];
  const workSourceDocument = record(documents, work?.source_document_ref);
  const application = typedRecords.find((entry) => ["trademarkapplication", "trademark_application"].includes(text(entry.record_type).toLowerCase()))
    ?? typedRecords.find((entry) => text(entry.source_document_ref) === text(workSourceDocument?.id ?? workspaceDocument?.id))
    ?? firstReferenced(typedRecords, distinct([...workspaceRefs, ...focusRefs, ...moduleRefs]))
    ?? typedRecords[0];
  const financial = financialRecords.find((entry) => text(entry.work_item_ref) === text(work?.id))
    ?? financialRecords.find((entry) => text(entry.business_record_ref) === text(application?.id))
    ?? firstReferenced(financialRecords, distinct([...workspaceRefs, ...focusRefs, ...moduleRefs]))
    ?? financialRecords[0];
  const approval = approvals.find((entry) => strings(entry.subject_refs).some((ref) => ref === text(work?.id)))
    ?? firstReferenced(approvals, distinct([...homeRefs, ...focusRefs, ...moduleRefs]))
    ?? approvals[0];
  const module = record(modules, selected.moduleId) ?? modules.find((entry) => text(entry.id) === text(application?.module_ref ?? application?.module_id))
    ?? firstReferenced(modules, distinct([...workspaceRefs, ...moduleRefs]))
    ?? modules[0];
  const proposal = proposals.find((entry) => text(field(entry, "module_ref") ?? entry.module_id) === text(module?.id))
    ?? firstReferenced(proposals, distinct([...workspaceRefs, ...moduleRefs]))
    ?? proposals[0];

  const sourceLink = documentLink(workSourceDocument ?? workspaceDocument);
  const focusLink = documentLink(focusDocument);
  const applicationLink = application
    ? { id: text(application.id), label: text(field(application, "display_id"), text(application.display_name, text(application.title, "Untitled record"))), kind: "record" as const }
    : undefined;
  const workLink = work ? { id: text(work.id), label: text(work.title, "Untitled work"), kind: "work" as const } : undefined;
  const financialType = text(financial?.type);
  const financeLink = financial
    ? {
        id: text(financial.id),
        label: [text(financial.display_name, "Financial record"), text(financial.display_amount)].filter(Boolean).join(" · "),
        kind: "finance" as const,
        financialRecordType: ["commitment", "invoice", "payment", "budget"].includes(financialType)
          ? financialType as CompanyOsLink["financialRecordType"]
          : undefined,
      }
    : undefined;
  const decisionTitle = approvalTitle(approval, financial, work);
  const decisionSummary = approvalSummary(approval, decisionTitle, financial, work);
  const approvalLink = approval ? { id: text(approval.id), label: decisionTitle, kind: "approval" as const, href: `?surface=approvals&approval=${encodeURIComponent(text(approval.id))}` } : undefined;
  const proposalLink = proposal ? { id: text(proposal.id), label: text(field(proposal, "title"), text(proposal.title, "Structure proposal")), kind: "module" as const } : undefined;
  const moduleLink = module ? { id: text(module.id), label: text(module.name, "Unnamed module"), kind: "module" as const, meta: text(module.status) ? humanize(module.status) : undefined } : undefined;

  const focusActorRefs = refs(root, "document-focus");
  const moduleActorRefs = refs(root, "business-module-focus");
  const homeActorRefs = refs(root, "home");
  const workActorRefs = [
    work?.requested_by_ref ?? work?.requested_by, work?.submitted_by_ref ?? work?.submitted_by, work?.accountable_owner_ref ?? work?.accountable_owner,
    ...(Array.isArray(work?.assignee_refs) ? work.assignee_refs : Array.isArray(work?.assignees) ? work.assignees : []),
    ...(Array.isArray(work?.contributor_refs) ? work.contributor_refs : Array.isArray(work?.contributors) ? work.contributors : []),
    work?.reviewer_ref ?? work?.reviewer, work?.legal_reviewer_ref, work?.approver_ref ?? work?.approver,
  ].map(refId).filter(Boolean);
  const referencedActorIds = items(focusDocument?.reference_refs)
    .filter((reference) => text(reference.kind) === "actor")
    .map((reference) => text(reference.id));
  const focusActors = linkEntries(distinct([...focusActorRefs, refId(focusDocument?.owner_ref), ...referencedActorIds, ...workActorRefs]).map((id) => actorLink(actors, id)));
  const moduleActors = linkEntries(distinct([...moduleActorRefs, ...workActorRefs]).map((id) => actorLink(actors, id)));
  const owner = actorLink(actors, focusDocument?.owner_ref ?? focusDocument?.created_by) ?? actorLink(actors, work?.accountable_owner_ref ?? work?.accountable_owner);
  const decisionActor = actorLink(actors, approval?.accountable_owner_ref ?? work?.approver_ref ?? work?.approver);
  const decisionRequester = actorLink(actors, approval?.requested_by_ref ?? approval?.requested_by ?? work?.requested_by_ref ?? work?.requested_by);
  const decisionCollaborators = linkEntries(distinct([
    ...homeActorRefs,
    ...focusActors.map((actor) => actor.id),
    refId(approval?.finance_reviewer_ref),
    refId(approval?.legal_reviewer_ref),
    refId(work?.submitted_by_ref ?? work?.submitted_by),
    refId(work?.reviewer_ref ?? work?.reviewer),
    refId(work?.legal_reviewer_ref),
    ...(Array.isArray(work?.contributor_refs) ? work.contributor_refs.map(refId) : []),
  ]).filter((id) => id && id !== decisionActor?.id && id !== decisionRequester?.id).map((id) => actorLink(actors, id)));
  const strategyPartner = focusActors.find((actor) => /strategy/i.test(actor.label));
  const rawDocumentStatus = text(field(focusDocument, "status"));
  const documentStatus = rawDocumentStatus
    ? humanize(rawDocumentStatus)
    : focusFacts.some((fact) => /on track/i.test(fact)) ? "On track" : "";
  const nextReviewAt = approval?.expires_at ?? work?.updated_at;
  const reportedMetrics = items(root.explicit_metrics);

  const docsBySpace = new Map<string, JsonRecord[]>();
  documents.forEach((entry) => {
    const space = text(entry.space, text(entry.space_id, "Unassigned"));
    docsBySpace.set(space, [...(docsBySpace.get(space) ?? []), entry]);
  });
  const workspaceTree: CompanyOsWorkspaceData["tree"] = [...docsBySpace].map(([space, entries]) => ({
    id: `space:${space}`,
    label: space,
    children: entries.map((entry) => ({ id: text(entry.id), ref: text(entry.id), label: text(entry.title, "Untitled document"), selected: false })),
  }));
  if (module && workspaceTree.length) {
    const parent = workspaceTree.find((entry) => entry.label === text(workspaceDocument?.space, text(workspaceDocument?.space_id))) ?? workspaceTree[0];
    parent.children?.push({ id: text(module.id), ref: text(module.id), label: text(module.name, "Unnamed module"), meta: humanize(module.status) || undefined });
  }

  const documentProperties = [
    owner && { label: "Owner", value: `${owner.label} · ${owner.actorType}`, ref: owner.id, actorType: owner.actorType },
    documentStatus && { label: "Project status", value: documentStatus },
    strategyPartner && { label: "Strategy partner", value: `${strategyPartner.label} · ${strategyPartner.actorType}`, ref: strategyPartner.id, actorType: strategyPartner.actorType },
    ...focusActors.filter((actor) => actor.id !== owner?.id && actor.id !== strategyPartner?.id).slice(0, 2).map((actor) => ({ label: "Participant", value: `${actor.label} · ${actor.actorType}`, ref: actor.id, actorType: actor.actorType })),
  ].filter(Boolean) as NonNullable<CompanyOsDocumentPageData["properties"]>;
  const documentBlocks: CompanyOsDocumentPageData["blocks"] = focusDocument
    ? [
        { id: "what", type: "heading", content: "What this plan coordinates" },
        { id: "what-copy", type: "paragraph", content: `This page keeps ${text(focusDocument.title, "the selected document")} connected to its related operating records.` },
        { id: "why", type: "heading", content: "Why this context matters" },
        { id: "why-copy", type: "paragraph", content: "The document explains the work; approvals, financial records, and execution outcomes remain linked to their authoritative records instead of becoming copied ledger facts here." },
        { id: "next", type: "heading", content: "Strategy and next review" },
        {
          id: "next-callout",
          type: "callout",
          tone: approval || work ? "warning" : "neutral",
          title: nextReviewAt ? `Review by ${humanTimestamp(nextReviewAt)}` : "Review linked work",
          content: work
            ? `${text(work.title, "Linked work")} is currently ${humanize(work.status) || "open"}. Review the linked decision before updating this plan.`
            : "No linked work is supplied for review.",
        },
        ...(work ? [{
          id: "linked-work-table",
          type: "table" as const,
          table: {
            caption: "Linked work",
            columns: ["Work", "Accountable", "Status", "Last updated"],
            rows: [[
              text(work.title, "Untitled work"),
              owner?.label ?? "Not supplied",
              humanize(work.status) || "Not supplied",
              humanTimestamp(work.updated_at) ?? "Not supplied",
            ]],
          },
        }] : []),
        ...(reportedMetrics.length ? [{
          id: "reported-metrics",
          type: "table" as const,
          table: {
            caption: "Reported metrics",
            columns: ["Metric", "Observed", "Value"],
            rows: reportedMetrics.map((metric) => [
              text(metric.label, "Metric"),
              humanTimestamp(metric.observed_at) ?? "Not supplied",
              text(metric.display_amount, text(metric.value, "Not supplied")),
            ]),
          },
        }] : []),
        { id: "linked-work", type: "relations", label: "Linked records", links: linkEntries([workLink, applicationLink, approvalLink, financeLink]) },
      ]
    : [{ id: "empty", type: "paragraph", content: "No rich document blocks are supplied." }];
  const documentActivity = [
    focusDocument?.updated_at && { id: `document:${text(focusDocument.id)}`, label: "Document updated", at: humanTimestamp(focusDocument.updated_at) },
    work?.updated_at && { id: `work:${text(work.id)}`, label: "Linked work updated", detail: text(work.title), at: humanTimestamp(work.updated_at) },
    financial?.updated_at && { id: `financial:${text(financial.id)}`, label: "Financial record updated", detail: text(financial.display_name), at: humanTimestamp(financial.updated_at) },
  ].filter(Boolean) as NonNullable<CompanyOsDocumentPageData["activity"]>;

  return {
    workspace: {
      fixtureId,
      title: "Company workspace",
      description: documents.length ? "Documents, typed records, and connected operating context." : "No company documents are supplied by this projection.",
      rootSelected: true,
      tree: workspaceTree,
      spaces: [...docsBySpace].map(([space, entries]) => ({ id: `space:${space}`, name: space, countLabel: `${entries.length} page${entries.length === 1 ? "" : "s"}` })),
      recentlyUpdated: linkEntries(documents.map(documentLink)),
      templates: [],
      databases: typedRecords.map((entry) => ({ id: text(entry.id), label: text(entry.record_type, "Typed record"), kind: "record" as const })),
      structureNotes: module ? [{ label: "Module state", value: humanize(module.status) || "Reported", tone: /proposed|pending/i.test(text(module.status)) ? "warning" : "neutral" }] : [],
      structureLinks: linkEntries([moduleLink, proposalLink, sourceLink, applicationLink, financeLink]),
      suggestions: linkEntries([sourceLink, applicationLink, workLink, approvalLink, financeLink]),
      proposal: proposalLink,
    },
    document: {
      fixtureId,
      id: focusDocument ? text(focusDocument.id) : undefined,
      title: focusDocument ? text(focusDocument.title, "Untitled document") : "No document selected",
      breadcrumb: focusDocument?.space || focusDocument?.space_id ? [text(focusDocument.space, text(focusDocument.space_id))] : undefined,
      description: focusDocument ? "This document is rendered from the supplied Company OS projection." : "Select a document or provide a document projection to begin.",
      properties: documentProperties,
      blocks: documentBlocks,
      sourceLinks: linkEntries([sourceLink]),
      resultLinks: linkEntries([workLink]),
      connectedRecords: linkEntries([applicationLink, approvalLink, financeLink]),
      activity: documentActivity,
      updatedLabel: focusDocument?.updated_at ? `Last updated ${humanTimestamp(focusDocument.updated_at)}` : undefined,
    },
    moduleView: {
      fixtureId,
      id: module ? text(module.id) : undefined,
      title: module ? text(module.name, "Unnamed module") : "No module selected",
      description: module ? "Linked records supplied by the current projection." : "No business module records are supplied.",
      records: work ? [{ id: text(work.id), title: text(work.title, "Untitled work"), type: "WorkItem", status: text(work.status) || undefined, group: text(work.status) || undefined, date: text(work.updated_at) || undefined, links: linkEntries([sourceLink, applicationLink, approvalLink, financeLink]) }] : [],
      columns: [
        { id: "title", label: "Record", cell: (entry) => entry.title },
        { id: "status", label: "Status", cell: (entry) => entry.status ?? "—" },
        { id: "links", label: "Connected", cell: (entry) => entry.links?.map((link) => link.label).join(", ") ?? "—" },
      ],
      availableViews: ["table", "board", "timeline"],
      sourceLinks: linkEntries([sourceLink, applicationLink, proposalLink, ...moduleActors]),
      resultLinks: linkEntries([workLink, financeLink]),
      fallback: { label: "Open standard record view", description: "The standard record view remains available if a custom module page is unavailable." },
    },
    home: {
      fixtureId,
      title: "Company home",
      subtitle: "Company context supplied by the current projection.",
      decisionRequired: approvalLink,
      decisionSummary,
      decisionActor: decisionActor ? { id: decisionActor.id, name: decisionActor.label, kind: decisionActor.actorType === "Human" ? "human" : decisionActor.actorType === "Standing Agent" ? "agent" : decisionActor.actorType === "External" ? "external" : "service" } : undefined,
      decisionRequester,
      decisionCollaborators,
      changes: linkEntries([sourceLink, applicationLink, workLink, financeLink]),
      workSummary: work ? [{ id: text(work.id), label: "Open work", value: "1", detail: text(work.title) }] : [],
      financeSummary: financeLink ? [{ id: financeLink.id, label: text(financial?.display_name, "Financial record"), value: text(financial?.display_amount, "—"), detail: financialType ? `Record type: ${financialType}` : undefined, financialRecordType: financeLink.financialRecordType }] : [],
    },
  };
}

/** @deprecated Use adaptCompanyOsDocsProjection for fixture and live projections alike. */
export const adaptTrademarkDocsFixture = adaptCompanyOsDocsProjection;
