import type {
  CompanyOsActorRef,
  CompanyOsDocumentHealthData,
  CompanyOsHomeData,
  CompanyOsHealthFinding,
  CompanyOsLink,
  CompanyOsRelationRepairContext,
  CompanyOsStructuredViewData,
  CompanyOsTemplateOption,
  CompanyOsTemplateRecordPolicy,
  CompanyOsWorkspaceArchive,
  CompanyOsWorkspaceData,
  CompanyOsWorkspaceTreeItem,
} from "./types";

type JsonRecord = Record<string, unknown>;
type Projection = {
  workspace: CompanyOsWorkspaceData;
  moduleView: CompanyOsStructuredViewData;
  home: CompanyOsHomeData;
  health: CompanyOsDocumentHealthData;
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

function queryText(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (value === null || value === undefined) return "";
  try {
    return JSON.stringify(value);
  } catch {
    return "";
  }
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

function isArchived(entry: JsonRecord | undefined): boolean {
  return text(entry?.lifecycle_status, text(entry?.status)).toLowerCase() === "archived";
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
  if (kind === "standing agent" || kind === "agent") return "Agent Membership";
  if (kind === "external") return "External";
  return "Service";
}

function actorLink(actors: JsonRecord[], id: unknown): CompanyOsLink | undefined {
  const actor = record(actors, refId(id));
  return actor
    ? { id: text(actor.id), label: text(actor.display_name, "Unnamed actor"), kind: "actor", actorType: kindForActor(actor) }
    : undefined;
}

function actorRef(actor: JsonRecord | undefined): CompanyOsActorRef | undefined {
  const id = text(actor?.id);
  if (!id || !actor) return undefined;
  const kind = kindForActor(actor);
  return {
    actor_type: kind === "Human" ? "human" : kind === "Agent Membership" ? "agent" : kind === "External" ? "external" : "service",
    actor_id: id,
  };
}

function entityRefs(value: unknown): Array<{ kind: string; id: string }> {
  return Array.isArray(value)
    ? value
        .filter((entry): entry is JsonRecord => Boolean(entry) && typeof entry === "object")
        .map((entry) => ({ kind: text(entry.kind), id: text(entry.id) }))
        .filter((entry) => entry.kind && entry.id)
    : [];
}

function docsDocumentHref(id: string): string | undefined {
  return id ? `?surface=docs&document=${encodeURIComponent(id)}` : undefined;
}

function docsModuleHref(id: string): string | undefined {
  return id ? `?surface=docs&module=${encodeURIComponent(id)}` : undefined;
}

function documentLink(entry: JsonRecord | undefined): CompanyOsLink | undefined {
  const id = text(entry?.id);
  return entry ? { id, label: text(entry.title, "Untitled document"), kind: "document", href: docsDocumentHref(id) } : undefined;
}

function moduleLink(entry: JsonRecord | undefined): CompanyOsLink | undefined {
  const id = text(entry?.id);
  return entry ? { id, label: text(entry.name, "Unnamed module"), kind: "module", href: docsModuleHref(id), meta: text(entry.status) ? humanize(entry.status) : undefined } : undefined;
}

function typedRecordLink(entry: JsonRecord | undefined): CompanyOsLink | undefined {
  return entry
    ? { id: text(entry.id), label: text(field(entry, "display_id"), text(entry.display_name, text(entry.title, "Untitled record"))), kind: "record", meta: text(entry.record_type) || undefined }
    : undefined;
}

function workLink(entry: JsonRecord | undefined): CompanyOsLink | undefined {
  return entry ? { id: text(entry.id), label: text(entry.title, "Untitled work"), kind: "work" } : undefined;
}

function financialRecordLink(entry: JsonRecord | undefined): CompanyOsLink | undefined {
  if (!entry) return undefined;
  const financialType = text(entry.type);
  return {
    id: text(entry.id),
    label: [text(entry.display_name, "Financial record"), text(entry.display_amount)].filter(Boolean).join(" · "),
    kind: "finance",
    financialRecordType: ["commitment", "invoice", "payment", "budget"].includes(financialType)
      ? financialType as CompanyOsLink["financialRecordType"]
      : undefined,
  };
}

function linkEntries(values: Array<CompanyOsLink | undefined>): CompanyOsLink[] {
  return values
    .filter((value): value is CompanyOsLink => Boolean(value?.id))
    .filter((value, index, entries) => entries.findIndex((candidate) => candidate.id === value.id) === index);
}

function contentObject(block: JsonRecord): JsonRecord {
  const content = block.content;
  return content && typeof content === "object" && !Array.isArray(content) ? content as JsonRecord : {};
}

function templateOption(document: JsonRecord, blocks: JsonRecord[]): CompanyOsTemplateOption {
  const blockOrder = strings(document.block_ids);
  const templateId = text(document.id);
  const orderedBlocks = blockOrder
    .map((id) => blocks.find((block) => text(block.id) === id && text(block.document_id, text(block.document_ref)) === templateId))
    .filter((block): block is JsonRecord => Boolean(block));
  return {
    id: templateId,
    label: text(document.title, "Untitled template"),
    kind: "document",
    meta: humanize(text(document.lifecycle_status, "draft")) || "Draft",
    templateBlockIds: orderedBlocks.map((block) => text(block.id)),
    templateBlocks: orderedBlocks.map((block) => ({
      id: text(block.id),
      kind: text(block.kind, "rich_text"),
      content: contentObject(block),
      referencedEntities: entityRefs(block.referenced_entities),
    })),
  };
}

function templateRecordPolicy(module: JsonRecord | undefined, definitionId: string, actorId: string): CompanyOsTemplateRecordPolicy {
  const recordTypes = strings(module?.record_types);
  const relationTypes = items(module?.relation_rules)
    .filter((rule) => text(rule.from_kind) === "document" && text(rule.to_kind) === "typed_record")
    .map((rule) => text(rule.relation_type))
    .filter(Boolean);
  const relationType = relationTypes[0] ?? "source_for";
  return {
    status: recordTypes.length && relationTypes.length ? "declared" : "missing",
    recordTypes,
    relationTypes,
    commandHint: `harness company docs relation link --definition ${definitionId} --from-document <child-document-id> --to-record <typed-record-id> --relation-type ${relationType} --actor ${actorId}`,
  };
}

function humanize(value: unknown): string {
  const raw = text(value);
  if (!raw) return "";
  return raw.replace(/[_-]+/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
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

function relationEndpointIds(relation: JsonRecord): string[] {
  return [
    relation.source_ref,
    relation.target_ref,
    relation.from_ref,
    relation.to_ref,
    relation.left_ref,
    relation.right_ref,
    relation.subject_ref,
    relation.object_ref,
  ].map(refId).filter(Boolean);
}

function hasRelationBetween(relations: JsonRecord[], leftId: string, rightId: string): boolean {
  return relations.some((relation) => {
    const ids = relationEndpointIds(relation);
    return ids.includes(leftId) && ids.includes(rightId);
  });
}

function buildDocumentHealthData({
  fixtureId,
  actors,
  documents,
  sourceDocuments,
  blocks,
  typedRecords,
  relations,
  modules,
  structureLinks,
  pageDefinitions,
}: {
  fixtureId?: string;
  actors: JsonRecord[];
  documents: JsonRecord[];
  /** Unfiltered Documents including archived rows, used only for source resolution. */
  sourceDocuments: JsonRecord[];
  blocks: JsonRecord[];
  typedRecords: JsonRecord[];
  relations: JsonRecord[];
  modules: JsonRecord[];
  structureLinks: CompanyOsLink[];
  pageDefinitions: JsonRecord[];
}): CompanyOsDocumentHealthData {
  const findings: CompanyOsHealthFinding[] = [];
  const documentIds = new Set(documents.map((entry) => text(entry.id)).filter(Boolean));
  const relationDefinition = pageDefinitions.find((definition) => Array.isArray(definition.action_command_refs)
    && definition.action_command_refs.map((value) => text(value)).includes("relation.append"));
  const module = modules.find((entry) => text(entry.id) === text(relationDefinition?.module_id)) ?? modules[0];
  const relationActionPolicyRef = Array.isArray(relationDefinition?.policy_refs)
    ? relationDefinition.policy_refs.map((value) => text(value)).find((value) => value.endsWith(":relation.append"))
    : undefined;
  const governanceActor = actors.find((actor) => /docs|document/i.test(text(actor.display_name)) && /agent/i.test(text(actor.actor_type)));
  const governanceActorRef = actorRef(governanceActor);
  const blockCounts = new Map<string, number>();
  blocks.forEach((block) => {
    const documentId = text(block.document_ref, text(block.document_id));
    if (documentId) blockCounts.set(documentId, (blockCounts.get(documentId) ?? 0) + 1);
  });

  const bySpaceAndTitle = new Map<string, JsonRecord[]>();
  documents.forEach((document) => {
    const title = text(document.title).trim().toLowerCase();
    if (!title) return;
    const key = `${text(document.space_id, text(document.space, "default"))}::${title}`;
    bySpaceAndTitle.set(key, [...(bySpaceAndTitle.get(key) ?? []), document]);
  });

  function recordBelongsToWorkDefinition(entry: JsonRecord | undefined): boolean {
    if (!entry || !module) return false;
    const moduleId = text(module.id);
    return text(entry.module_ref, text(entry.module_id, text(field(entry, "module_ref"), text(field(entry, "module_id"))))) === moduleId;
  }

  function documentBelongsToWorkDefinition(sourceDocument: JsonRecord | undefined, _sourceRecords: JsonRecord[] = []): boolean {
    if (!sourceDocument || !module) return false;
    const documentId = text(sourceDocument.id);
    const rootDocumentId = text(module.root_document_ref, text(module.root_document_id));
    if (rootDocumentId === documentId) return true;
    let cursor: JsonRecord | undefined = sourceDocument;
    const visited = new Set<string>();
    while (cursor) {
      const cursorId = text(cursor.id);
      if (!cursorId || visited.has(cursorId)) return false;
      visited.add(cursorId);
      const parentId = text(cursor.parent_document_id, text(cursor.parent_document_ref, text(cursor.parent_id)));
      if (!parentId) return false;
      if (parentId === rootDocumentId) return true;
      cursor = record(documents, parentId);
    }
    return false;
  }

  function relationRepairContext(sourceDocument: JsonRecord | undefined, sourceRecord: JsonRecord | undefined): CompanyOsRelationRepairContext | undefined {
    const sourceDocumentId = text(sourceDocument?.id);
    const sourceRecordId = text(sourceRecord?.id);
    if (!sourceDocumentId || !sourceRecordId || !sourceRecord || !relationDefinition || !relationActionPolicyRef || !governanceActorRef) return undefined;
    if (!documentBelongsToWorkDefinition(sourceDocument, [sourceRecord]) || !recordBelongsToWorkDefinition(sourceRecord)) return undefined;
    return {
      definitionId: text(relationDefinition.id),
      actionPolicyRef: relationActionPolicyRef,
      relationType: "source_for",
      from: { kind: "document", id: sourceDocumentId },
      to: { kind: "typed_record", id: sourceRecordId },
      provenanceRef: { kind: "document", id: sourceDocumentId },
      requestedBy: governanceActorRef,
      createdBy: governanceActorRef,
    };
  }

  documents.forEach((document) => {
    const documentId = text(document.id);
    const parentId = text(document.parent_document_id, text(document.parent_document_ref, text(document.parent_id)));
    if (parentId && !documentIds.has(parentId)) {
      findings.push({
        id: `orphan-document:${documentId}`,
        kind: "orphan_document",
        severity: "critical",
        title: "Document parent is missing",
        detail: `${text(document.title, documentId)} references parent ${parentId}, but that document is not present in the projection.`,
        subject: documentLink(document),
        recommendedAction: "Route a native TeamWork to Docs Governance, or run a governed Docs action to attach the document to a valid parent.",
        directActionLabel: "Relink parent",
      });
    }
    const blockCount = blockCounts.get(documentId) ?? 0;
    if (blockCount > 50) {
      findings.push({
        id: `oversized-document:${documentId}`,
        kind: "oversized_document",
        severity: "warning",
        title: "Document is becoming oversized",
        detail: `${text(document.title, documentId)} has ${blockCount} blocks. Consider extracting typed records or sub-documents before it becomes hard for agents to maintain.`,
        subject: documentLink(document),
        recommendedAction: "Ask Docs Governance to split this document through native TeamWork and preserve source/result relations.",
      });
    }
  });

  bySpaceAndTitle.forEach((entries) => {
    if (entries.length < 2) return;
    findings.push({
      id: `duplicate-title:${text(entries[0].space_id, text(entries[0].space, "default"))}:${text(entries[0].title).toLowerCase()}`,
      kind: "duplicate_document_title",
      severity: "warning",
      title: "Duplicate document title in one space",
      detail: `${entries.length} documents share the title “${text(entries[0].title, "Untitled document")}”. Agents need a stable naming convention to route work correctly.`,
      subject: documentLink(entries[0]),
      affected: linkEntries(entries.map(documentLink)),
      recommendedAction: "Route cleanup through native TeamWork to rename or merge duplicates; do not delete historical documents without a governed action.",
      directActionLabel: "Rename document",
    });
  });

  typedRecords.forEach((entry) => {
    const recordId = text(entry.id);
    const sourceDocumentId = text(entry.source_document_ref, text(entry.source_document_id));
    if (!sourceDocumentId) {
      findings.push({
        id: `missing-source:${recordId}`,
        kind: "typed_record_missing_source",
        severity: "warning",
        title: "TypedRecord has no source Document",
        detail: `${text(entry.display_name, recordId)} has no source_document_ref. It may be valid, but agents cannot explain where the durable business fact came from.`,
        subject: typedRecordLink(entry),
        recommendedAction: "Link the typed record to its originating Document or mark the module policy that allows source-less records.",
        directActionLabel: "Link source",
      });
      return;
    }
    const sourceDocument = record(sourceDocuments, sourceDocumentId);
    if (!sourceDocument) {
      findings.push({
        id: `missing-source-document:${recordId}`,
        kind: "typed_record_source_document_missing",
        severity: "critical",
        title: "TypedRecord source Document is missing",
        detail: `${text(entry.display_name, recordId)} points to ${sourceDocumentId}, but that Document is not present.`,
        subject: typedRecordLink(entry),
        related: { id: sourceDocumentId, label: sourceDocumentId, kind: "document" },
        recommendedAction: "Restore the source Document or create a governed relation migration that moves this record to a valid source.",
      });
      return;
    }
    if (isArchived(sourceDocument)) {
      const archivedLink = documentLink(sourceDocument);
      findings.push({
        id: `archived-source-document:${recordId}`,
        kind: "typed_record_source_document_archived",
        severity: "warning",
        title: "TypedRecord source Document is archived history",
        detail: `${text(entry.display_name, recordId)} points to ${text(sourceDocument.title, sourceDocumentId)}, which is archived. The source stays readable for provenance but is no longer active company memory.`,
        subject: typedRecordLink(entry),
        related: archivedLink ? { ...archivedLink, meta: "archived" } : undefined,
        recommendedAction: "The source Document is explicit archived history; keep it read-only or route a successor source through a governed Docs action.",
      });
    }
    if (!hasRelationBetween(relations, sourceDocumentId, recordId)) {
      findings.push({
        id: `missing-doc-record-relation:${recordId}`,
        kind: "missing_document_record_relation",
        severity: "warning",
        title: "Source Document and TypedRecord lack an explicit Relation",
        detail: `${text(entry.display_name, recordId)} has source_document_ref=${sourceDocumentId}, but no durable Relation links the two objects for navigation and audits.`,
        subject: typedRecordLink(entry),
        related: documentLink(sourceDocument),
        recommendedAction: "Run the Docs relation link command or dispatch a governed Docs action to create the explicit Document ↔ TypedRecord relation.",
        directActionLabel: "Link relation",
        relationRepairContext: relationRepairContext(sourceDocument, entry),
      });
    }
  });

  modules.forEach((entry) => {
    const rootDocumentId = text(entry.root_document_ref, text(entry.root_document_id));
    if (rootDocumentId && documentIds.has(rootDocumentId)) return;
    findings.push({
      id: `missing-module-root:${text(entry.id)}`,
      kind: "business_module_missing_root_document",
      severity: rootDocumentId ? "critical" : "warning",
      title: "BusinessModule has no valid root Document",
      detail: rootDocumentId
        ? `${text(entry.name, "Unnamed module")} points to root document ${rootDocumentId}, but it is missing.`
        : `${text(entry.name, "Unnamed module")} does not declare a root_document_ref.`,
      subject: moduleLink(entry),
      related: rootDocumentId ? { id: rootDocumentId, label: rootDocumentId, kind: "document" } : undefined,
      recommendedAction: "Create or attach a root Document before agents add new records into this module.",
      directActionLabel: "Attach root Document",
    });
  });

  const critical = findings.filter((finding) => finding.severity === "critical").length;
  const warning = findings.filter((finding) => finding.severity === "warning").length;
  const cleanupQueue: CompanyOsDocumentHealthData["cleanupQueue"] = findings
    .filter((finding) => ["duplicate_document_title", "oversized_document", "orphan_document", "business_module_missing_root_document", "typed_record_source_document_missing"].includes(finding.kind))
    .map((finding) => {
      const operation = finding.kind === "duplicate_document_title"
        ? "merge"
        : finding.kind === "oversized_document"
          ? "split"
          : "migrate";
      return {
        id: `cleanup:${finding.id}`,
        operation,
        label: operation === "merge"
          ? "Review merge or rename"
          : operation === "split"
            ? "Plan document split"
            : "Plan structure migration",
        detail: `${finding.title}. Route through native TeamWork so affected owners can preserve provenance and rollback context.`,
        findingId: finding.id,
        subject: finding.subject,
        route: "team_work" as const,
      };
    });

  return {
    fixtureId,
    title: "Docs structure health",
    description: "Native health review for company memory: Documents, Blocks, TypedRecords, Relations, Views, and BusinessModules remain explicit and governed.",
    status: findings.length ? "issues" : "pass",
    counts: {
      documents: documents.length,
      blocks: blocks.length,
      typedRecords: typedRecords.length,
      relations: relations.length,
      businessModules: modules.length,
      findings: findings.length,
      critical,
      warning,
    },
    findings,
    cleanupQueue,
    governanceAgent: actorLink(actors, governanceActor?.id),
    structureLinks: linkEntries([...structureLinks, ...documents.slice(0, 8).map(documentLink)]).filter((link, index, values) => values.findIndex((candidate) => candidate.id === link.id) === index),
    actionHints: [
      { id: "health-cli", label: "Run health audit", command: "harness company docs health", tone: "primary" },
      { id: "relation-cli", label: "Link durable truth", command: "harness company docs relation link", tone: "neutral" },
      { id: "corrective-work", label: "Route corrective work", command: "harness team-run work create --team-run-id <id> --title <text> --completion-criteria <text>", tone: "warning" },
      { id: "docs-action", label: "Dispatch Docs action", command: "Browser Action: relation.append", disabledReason: relationDefinition && relationActionPolicyRef ? undefined : "Direct mutation requires Docs action policy and idempotency." },
    ],
  };
}

/**
 * The single lifecycle predicate the default document tree applies.  It is stated
 * as an exclusion, not an `active` allow-list: governed Documents legitimately stay
 * `draft` because no draft -> active transition exists in the Docs command surface,
 * so an active-only tree would render the operating hierarchy empty.
 */
const DEFAULT_TREE_FILTER = 'lifecycle_status != "archived"';

function documentTitleOrder(left: JsonRecord, right: JsonRecord): number {
  return text(left.title, text(left.id)).localeCompare(text(right.title, text(right.id)), undefined, { numeric: true, sensitivity: "base" });
}

/**
 * Builds a real document hierarchy from Document.parent_document_id.  Space is only
 * the root grouping: a Document nests under its parent wherever that parent lives,
 * including across spaces.  A Document whose parent is absent from `entries` — because
 * it is missing, or because the caller's lifecycle filter removed it — anchors its own
 * root instead of disappearing, and a parent cycle anchors rather than recursing.
 */
function buildDocumentSpaceTree(
  entries: JsonRecord[],
  options: { selectedDocumentId?: string; meta?: string; idPrefix?: string } = {},
): CompanyOsWorkspaceTreeItem[] {
  const prefix = options.idPrefix ?? "";
  const byId = new Map<string, JsonRecord>();
  entries.forEach((entry) => {
    const id = text(entry.id);
    if (id) byId.set(id, entry);
  });

  const isRoot = (entry: JsonRecord): boolean => {
    let cursor = text(entry.parent_document_id);
    if (!cursor) return true;
    const seen = new Set([text(entry.id)]);
    while (cursor) {
      if (seen.has(cursor)) return true;
      const parent = byId.get(cursor);
      if (!parent) return true;
      seen.add(cursor);
      cursor = text(parent.parent_document_id);
    }
    return false;
  };

  const roots = entries.filter(isRoot);
  const rootIds = new Set(roots.map((entry) => text(entry.id)));
  const childrenByParent = new Map<string, JsonRecord[]>();
  entries.forEach((entry) => {
    if (rootIds.has(text(entry.id))) return;
    const parentId = text(entry.parent_document_id);
    childrenByParent.set(parentId, [...(childrenByParent.get(parentId) ?? []), entry]);
  });

  const node = (entry: JsonRecord): CompanyOsWorkspaceTreeItem => {
    const documentId = text(entry.id);
    const children = [...(childrenByParent.get(documentId) ?? [])].sort(documentTitleOrder).map(node);
    return {
      id: `${prefix}${documentId}`,
      ref: documentId,
      kind: "document",
      label: text(entry.title, "Untitled document"),
      href: docsDocumentHref(documentId),
      selected: Boolean(documentId) && options.selectedDocumentId === documentId,
      ...(options.meta ? { meta: options.meta } : {}),
      ...(children.length ? { children } : {}),
    };
  };

  const rootsBySpace = new Map<string, JsonRecord[]>();
  roots.forEach((entry) => {
    const space = text(entry.space, text(entry.space_id, "Unassigned"));
    rootsBySpace.set(space, [...(rootsBySpace.get(space) ?? []), entry]);
  });
  return [...rootsBySpace]
    .sort(([left], [right]) => left.localeCompare(right, undefined, { numeric: true, sensitivity: "base" }))
    .map(([space, spaceRoots]) => {
      const sorted = [...spaceRoots].sort(documentTitleOrder);
      return {
        id: `${prefix}space:${space}`,
        kind: "space" as const,
        label: space,
        href: docsDocumentHref(text(sorted[0]?.id)),
        children: sorted.map(node),
      };
    });
}

/**
 * Converts an arbitrary Company OS read projection into Docs presentation data.
 * It performs no store access, persistence, status inference, or fallback data
 * fabrication. Empty projections deliberately remain empty.
 */
export function adaptCompanyOsDocsProjection(input: unknown, selected: { documentId?: string; moduleId?: string } = {}): Projection {
  const root = (input && typeof input === "object" ? input : {}) as JsonRecord;
  const actors = items(root.actors);
  const allDocuments = items(root.documents);
  const documents = allDocuments.filter((entry) => !isArchived(entry));
  const activeDocumentIds = new Set(documents.map((entry) => text(entry.id)).filter(Boolean));
  const typedRecords = items(root.typed_records);
  const workAggregate = root.work && typeof root.work === "object" && !Array.isArray(root.work) ? root.work as JsonRecord : {};
  const works = items(workAggregate.works).length ? items(workAggregate.works) : items(root.works);
  const financialRecords = items(root.financial_records);
  const approvals = items(root.approvals);
  const allModules = items(root.business_modules);
  const modules = allModules.filter((entry) => {
    if (isArchived(entry)) return false;
    const rootDocumentRef = text(entry.root_document_ref, text(entry.root_document_id));
    return !rootDocumentRef || activeDocumentIds.size === 0 || activeDocumentIds.has(rootDocumentRef);
  });
  const relations = items(root.relations);
  const blocks = items(root.blocks);
  const views = items(root.views);
  const pageDefinitions = items(root.custom_page_definitions);
  const proposals = [
    ...items(root.governance_proposals),
    ...typedRecords.filter((entry) => text(entry.record_type).toLowerCase() === "governance_proposal"),
  ];
  const fixtureId = text(root.fixture_id) || undefined;

  const workspaceRefs = refs(root, "docs-workspace");
  const focusRefs = refs(root, "document-focus");
  const moduleRefs = refs(root, "business-module-focus");
  const homeRefs = refs(root, "home");
  const workspaceDocument = firstReferenced(documents, workspaceRefs) ?? documents[0];
  const explicitlySelectedDocument = selected.documentId
    ? record(allDocuments, selected.documentId)
    : undefined;
  const focusDocument = explicitlySelectedDocument
    ?? firstReferenced(documents, focusRefs)
    ?? workspaceDocument
    ?? documents[0];
  const templateDocuments = documents.filter((entry) => text(entry.kind).toLowerCase() === "template");
  const templateLinks = templateDocuments.map((document) => templateOption(document, blocks));
  const work = firstReferenced(works, distinct([...focusRefs, ...moduleRefs])) ?? works[0];
  const workSourceDocument = workspaceDocument;
  const application = typedRecords.find((entry) => ["trademarkapplication", "trademark_application"].includes(text(entry.record_type).toLowerCase()))
    ?? typedRecords.find((entry) => text(entry.source_document_ref) === text(workSourceDocument?.id ?? workspaceDocument?.id))
    ?? firstReferenced(typedRecords, distinct([...workspaceRefs, ...focusRefs, ...moduleRefs]))
    ?? typedRecords[0];
  const financial = financialRecords.find((entry) => text(entry.work_ref) === text(work?.id))
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

  const sourceLinkBase = documentLink(workSourceDocument ?? workspaceDocument);
  // An archived Work source stays visible and navigable as explicit archived
  // history instead of silently falling back to another Document.
  const sourceLink = sourceLinkBase && workSourceDocument && isArchived(workSourceDocument)
    ? { ...sourceLinkBase, meta: "Archived history" }
    : sourceLinkBase;
  const applicationLink = application
    ? { id: text(application.id), label: text(field(application, "display_id"), text(application.display_name, text(application.title, "Untitled record"))), kind: "record" as const }
    : undefined;
  const selectedWorkLink = workLink(work);
  const financialType = text(financial?.type);
  const financeLink = financialRecordLink(financial);
  const decisionTitle = approvalTitle(approval, financial, work);
  const decisionSummary = approvalSummary(approval, decisionTitle, financial, work);
  const approvalLink = approval ? { id: text(approval.id), label: decisionTitle, kind: "approval" as const, href: `?surface=approvals&approval=${encodeURIComponent(text(approval.id))}` } : undefined;
  const proposalLink = proposal ? { id: text(proposal.id), label: text(field(proposal, "title"), text(proposal.title, "Structure proposal")), kind: "module" as const } : undefined;
  const selectedModuleLink = moduleLink(module);
  const focusDocumentId = text(focusDocument?.id);
  const focusTypedRecords = focusDocumentId
    ? typedRecords.filter((entry) => text(entry.source_document_ref, text(entry.source_document_id)) === focusDocumentId
      || hasRelationBetween(relations, focusDocumentId, text(entry.id)))
    : [];
  const focusActorRefs = refs(root, "document-focus");
  const moduleActorRefs = refs(root, "business-module-focus");
  const homeActorRefs = refs(root, "home");
  const referencedActorIds = items(focusDocument?.reference_refs)
    .filter((reference) => text(reference.kind) === "actor")
    .map((reference) => text(reference.id));
  const focusActors = linkEntries(distinct([...focusActorRefs, refId(focusDocument?.owner_ref), ...referencedActorIds]).map((id) => actorLink(actors, id)));
  const moduleActors = linkEntries(distinct(moduleActorRefs).map((id) => actorLink(actors, id)));
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

  const docsBySpace = new Map<string, JsonRecord[]>();
  documents.forEach((entry) => {
    const space = text(entry.space, text(entry.space_id, "Unassigned"));
    docsBySpace.set(space, [...(docsBySpace.get(space) ?? []), entry]);
  });
  // The default tree is the real parent_document_id hierarchy under DEFAULT_TREE_FILTER;
  // space contributes root grouping only, so a child never re-appears as a sibling of
  // its own parent just because they share a space_id.
  const workspaceTree: CompanyOsWorkspaceData["tree"] = buildDocumentSpaceTree(documents, { selectedDocumentId: selected.documentId });
  // Track what the tree actually placed. A module can pass the navigability filter and
  // still find no home — its space may contain no root Document once the hierarchy is
  // real — and such a module must surface in the archive rather than silently vanish.
  const placedModuleIds = new Set<string>();
  for (const candidate of modules) {
    const moduleId = text(candidate.id);
    const rootDocument = record(documents, candidate.root_document_ref ?? candidate.root_document_id);
    if (!rootDocument && selected.documentId) continue;
    const moduleSpace = text(rootDocument?.space, text(rootDocument?.space_id, text(workspaceDocument?.space, text(workspaceDocument?.space_id))));
    const parent = workspaceTree.find((entry) => entry.label === moduleSpace) ?? (!rootDocument ? workspaceTree[0] : undefined);
    if (!moduleId || !parent) continue;
    parent.children?.push({ id: moduleId, ref: moduleId, kind: "module", label: text(candidate.name, "Unnamed module"), href: docsModuleHref(moduleId), selected: selected.moduleId === moduleId, meta: humanize(candidate.status) || undefined });
    placedModuleIds.add(moduleId);
  }

  // The archive is the exact complement of the default tree: every Document the
  // `documents` predicate removed, plus every BusinessModule the tree did not place.
  // Conservation is the invariant — a module appears in exactly one of the two views —
  // so the withheld set is derived from what was placed, never from the filter alone.
  const archivedDocuments = allDocuments.filter((entry) => isArchived(entry));
  const withheldModules = allModules.filter((entry) => !placedModuleIds.has(text(entry.id)));
  const withholdReason = (entry: JsonRecord): string => {
    if (isArchived(entry)) return "Module is archived";
    const rootRef = text(entry.root_document_ref, text(entry.root_document_id));
    if (!rootRef) return "No root Document is declared";
    const rootDocument = record(allDocuments, rootRef);
    if (!rootDocument) return "Root Document is missing from this projection";
    if (isArchived(rootDocument)) return "Root Document is archived";
    return "No navigable space holds this module";
  };
  const workspaceArchive: CompanyOsWorkspaceArchive | undefined = allDocuments.length || allModules.length
    ? {
      defaultFilter: DEFAULT_TREE_FILTER,
      tree: buildDocumentSpaceTree(archivedDocuments, { selectedDocumentId: selected.documentId, meta: "Archived", idPrefix: "archive:" }),
      documentIds: archivedDocuments.map((entry) => text(entry.id)).filter(Boolean),
      modules: linkEntries(withheldModules.map((entry) => {
        const link = moduleLink(entry);
        return link ? { ...link, meta: withholdReason(entry) } : undefined;
      })),
      countLabel: `${archivedDocuments.length} archived page${archivedDocuments.length === 1 ? "" : "s"}`,
    }
    : undefined;

  const structureLinks = linkEntries([selectedModuleLink, proposalLink, sourceLink, applicationLink, financeLink]);
  const documentDefinition = pageDefinitions.find((definition) => Array.isArray(definition.action_command_refs)
    && definition.action_command_refs.map((value) => text(value)).includes("document.append")
    && definition.action_command_refs.map((value) => text(value)).includes("block.append")
    && text(definition.module_id) === text(module?.id))
    ?? pageDefinitions.find((definition) => Array.isArray(definition.action_command_refs)
      && definition.action_command_refs.map((value) => text(value)).includes("document.append")
      && definition.action_command_refs.map((value) => text(value)).includes("block.append"));
  const typedRecordPolicyRef = Array.isArray(documentDefinition?.policy_refs)
    ? documentDefinition.policy_refs.map((value) => text(value)).find((value) => value.endsWith(":typed_record.append"))
    : undefined;
  const viewPolicyRef = Array.isArray(documentDefinition?.policy_refs)
    ? documentDefinition.policy_refs.map((value) => text(value)).find((value) => value.endsWith(":view.append"))
    : undefined;
  const moduleRelationPolicyRef = Array.isArray(documentDefinition?.policy_refs)
    ? documentDefinition.policy_refs.map((value) => text(value)).find((value) => value.endsWith(":relation.append"))
    : undefined;
  const actorPermissions = (actor: JsonRecord) => strings(actor.permission_policy_refs).concat(strings((actor.actor as JsonRecord | undefined)?.permission_policy_refs));
  const isWritableAgent = (actor: JsonRecord) => kindForActor(actor) === "Agent Membership" && actorPermissions(actor).includes("company.records.write");
  const isDocsGovernanceAgent = (actor: JsonRecord) => /docs|document/i.test([text(actor.id), text(actor.display_name), text((actor.actor as JsonRecord | undefined)?.display_name)].filter(Boolean).join(" "));
  const documentAuthoringAgent = actors.find((actor) => isWritableAgent(actor) && isDocsGovernanceAgent(actor))
    ?? actors.find(isWritableAgent);
  const documentAuthoringActor = actorRef(documentAuthoringAgent)
    ?? actorRef(actors.find((actor) => actorPermissions(actor).includes("company.records.write")));
  const primaryDefinitionForPolicy = text(documentDefinition?.id, "<custom-page-definition-id>");
  const actorForPolicy = documentAuthoringActor?.actor_id ?? "<agent-or-human-id>";
  const templatePolicy = templateRecordPolicy(module, primaryDefinitionForPolicy, actorForPolicy);
  const moduleAuthoringSourceDocumentId = text(module?.root_document_ref, text(module?.root_document_id, text(focusDocument?.id)));
  const moduleAuthoring = module && moduleAuthoringSourceDocumentId && documentDefinition && typedRecordPolicyRef && viewPolicyRef && moduleRelationPolicyRef && documentAuthoringActor
    ? {
        definitionId: text(documentDefinition.id),
        moduleId: text(module.id),
        sourceDocumentId: moduleAuthoringSourceDocumentId,
        typedRecordPolicyRef,
        relationPolicyRef: moduleRelationPolicyRef,
        viewPolicyRef,
        requestedBy: documentAuthoringActor,
      }
    : undefined;
  const health = buildDocumentHealthData({
    fixtureId,
    actors,
    documents,
    sourceDocuments: allDocuments,
    blocks,
    typedRecords,
    relations,
    modules,
    structureLinks,
    pageDefinitions,
  });
  const primaryDocumentId = text(workspaceDocument?.id, text(focusDocument?.id, "<doc-id>"));
  const primaryModuleId = text(module?.id, "<module-id>");
  const primaryDefinitionId = text(pageDefinitions[0]?.id, "<custom-page-definition-id>");
  const primaryViewId = text((views.find((view) => text(view.module_id) === primaryModuleId) ?? views[0])?.id, "<view-id>");
  const governanceAuthority = text(
    actors.find((actor) => {
      const actorRecord = actor.actor;
      return (
        text(actor.actor_type) === "human" &&
        actorRecord &&
        typeof actorRecord === "object" &&
        !Array.isArray(actorRecord) &&
        Array.isArray((actorRecord as JsonRecord).permission_policy_refs) &&
        ((actorRecord as JsonRecord).permission_policy_refs as unknown[]).includes("company_os.admin")
      );
    })?.id,
    "<human-admin-id>",
  );
  const governanceCommands = [
    {
      id: "module-create",
      label: "Create BusinessModule",
      command: `harness company docs module create --root-document ${primaryDocumentId} --name <module-name> --purpose <purpose> --authority ${governanceAuthority} --record-type <type> --relation-rule-json '{"relation_type":"source_for","from_kind":"document","to_kind":"typed_record","required":true,"cross_module":false}'`,
      scope: "governance" as const,
      disabledReason: governanceAuthority === "<human-admin-id>" ? "Requires a Human company_os.admin authority." : undefined,
    },
    {
      id: "page-definition-create",
      label: "Install page definition",
      command: `harness company docs page-definition create --module ${primaryModuleId} --fallback-view ${primaryViewId} --purpose <purpose> --authority ${governanceAuthority}`,
      scope: "governance" as const,
      disabledReason: primaryModuleId === "<module-id>" || primaryViewId === "<view-id>" ? "Requires an existing BusinessModule and fallback View." : undefined,
    },
    {
      id: "document-create",
      label: "Create child document",
      command: `harness company docs document create --definition ${primaryDefinitionId} --parent-document ${primaryDocumentId} --title <title> [--template <template-document-id> --instantiate-template] --actor <agent-or-human-id>`,
      scope: "module_action" as const,
      disabledReason: primaryDefinitionId === "<custom-page-definition-id>" ? "Requires a CustomPageDefinition that declares document.append." : undefined,
    },
    {
      id: "template-create",
      label: "Create reusable template",
      command: `harness company docs template create --definition ${primaryDefinitionId} --parent-document ${primaryDocumentId} --title <template-title> [--from-document <source-doc-id>] --actor <agent-or-human-id>`,
      scope: "module_action" as const,
      disabledReason: primaryDefinitionId === "<custom-page-definition-id>" ? "Requires a CustomPageDefinition that declares document.append and block.append." : undefined,
    },
    {
      id: "template-status",
      label: "Set template lifecycle",
      command: `harness company docs template status --definition ${primaryDefinitionId} --template <template-document-id> --status active|paused|archived --actor <agent-or-human-id>`,
      scope: "module_action" as const,
      disabledReason: primaryDefinitionId === "<custom-page-definition-id>" ? "Requires a CustomPageDefinition that declares document.append." : undefined,
    },
    {
      id: "block-append",
      label: "Append structured block",
      command: `harness company docs block append --definition ${primaryDefinitionId} --document ${primaryDocumentId} --kind callout --content-json <json> --actor <agent-or-human-id>`,
      scope: "module_action" as const,
      disabledReason: primaryDefinitionId === "<custom-page-definition-id>" ? "Requires a CustomPageDefinition that declares block.append." : undefined,
    },
    {
      id: "block-reorder",
      label: "Reorder document blocks",
      command: `harness company docs block reorder --definition ${primaryDefinitionId} --document ${primaryDocumentId} --block-order <block-id-2,block-id-1> --actor <agent-or-human-id>`,
      scope: "module_action" as const,
      disabledReason: primaryDefinitionId === "<custom-page-definition-id>" ? "Requires a CustomPageDefinition that declares document.append." : undefined,
    },
    {
      id: "typed-record-append",
      label: "Create typed record",
      command: `harness company docs typed-record append --definition ${primaryDefinitionId} --module ${primaryModuleId} --source-document ${primaryDocumentId} --record-type <type> --title <title> --actor <agent-or-human-id>`,
      scope: "module_action" as const,
      disabledReason: primaryDefinitionId === "<custom-page-definition-id>" || primaryModuleId === "<module-id>" ? "Requires a scoped CustomPageDefinition and BusinessModule." : undefined,
    },
    {
      id: "view-create",
      label: "Create standard view",
      command: `harness company docs view create --definition ${primaryDefinitionId} --module ${primaryModuleId} --title <title> --source-kind typed_record --actor <agent-or-human-id>`,
      scope: "module_action" as const,
      disabledReason: primaryDefinitionId === "<custom-page-definition-id>" || primaryModuleId === "<module-id>" ? "Requires a scoped CustomPageDefinition and BusinessModule." : undefined,
    },
    {
      id: "relation-link",
      label: "Link document to record",
      command: `harness company docs relation link --definition ${primaryDefinitionId} --from-document ${primaryDocumentId} --to-record <typed-record-id> --actor <agent-or-human-id>`,
      scope: "module_action" as const,
      disabledReason: primaryDefinitionId === "<custom-page-definition-id>" ? "Requires a CustomPageDefinition that declares relation.append." : undefined,
    },
  ];
  const moduleTypedRecords = typedRecords.filter((entry) => {
    const moduleRef = text(entry.module_id) || text(entry.module_ref);
    return moduleRef && moduleRef === text(module?.id);
  });
  const moduleRecords: CompanyOsStructuredViewData["records"] = moduleTypedRecords.length
    ? moduleTypedRecords.map((entry) => {
        const sourceDocument = record(documents, entry.source_document_ref);
        const relatedWork = works.find((candidate) => relations.some((relation) => {
          const endpointIds = relationEndpointIds(relation);
          return endpointIds.includes(text(candidate.id)) && endpointIds.includes(text(entry.id));
        }));
        const relatedFinancial = financialRecords.find((candidate) => text(candidate.business_record_ref) === text(entry.id))
          ?? financialRecords.find((candidate) => text(candidate.work_ref) === text(relatedWork?.id));
        const relatedApproval = approvals.find((candidate) => strings(candidate.subject_refs).some((ref) => ref === text(relatedWork?.id) || ref === text(relatedFinancial?.id)))
          ?? approvals.find((candidate) => text(candidate.subject_ref) === text(relatedWork?.id) || text(candidate.subject_ref) === text(relatedFinancial?.id));
        return {
          id: text(entry.id),
          title: text(field(entry, "display_id"), text(entry.display_name, text(entry.title, "Untitled record"))),
          type: text(entry.record_type, "TypedRecord"),
          status: text(entry.lifecycle_status, text(field(entry, "status"))) || undefined,
          group: humanize(text(entry.record_type, "TypedRecord")),
          date: text(entry.updated_at, text(entry.created_at)) || undefined,
          links: linkEntries([
            documentLink(sourceDocument),
            relatedWork ? { id: text(relatedWork.id), label: text(relatedWork.title, "Linked work"), kind: "work" as const } : undefined,
            relatedApproval ? { id: text(relatedApproval.id), label: approvalTitle(relatedApproval, relatedFinancial, relatedWork), kind: "approval" as const } : undefined,
            relatedFinancial ? {
              id: text(relatedFinancial.id),
              label: [text(relatedFinancial.display_name, "Financial record"), text(relatedFinancial.display_amount)].filter(Boolean).join(" · "),
              kind: "finance" as const,
              financialRecordType: ["commitment", "invoice", "payment", "budget"].includes(text(relatedFinancial.type))
                ? text(relatedFinancial.type) as CompanyOsLink["financialRecordType"]
                : undefined,
            } : undefined,
          ]),
        };
      })
    : work ? [{
        id: text(work.id),
        title: text(work.title, "Untitled work"),
        type: "TeamWork",
        status: text(work.phase) || undefined,
        group: text(work.condition) || undefined,
        date: text(work.updated_at) || undefined,
        links: linkEntries([sourceLink, applicationLink, approvalLink, financeLink]),
      }] : [];
  const moduleConnectedLinks = moduleRecords
    .flatMap((entry) => entry.links ?? [])
    .filter((link, index, values) => values.findIndex((candidate) => candidate.id === link.id) === index);
  const moduleNativeView = views.find((entry) => text(entry.module_id) === text(module?.id))
    ?? views.find((entry) => strings(module?.default_view_refs).includes(text(entry.id)))
    ?? views[0];
  const moduleViewSourceKinds = strings(moduleNativeView?.source_kinds);
  const moduleViewQueryRecord = moduleNativeView?.query && typeof moduleNativeView.query === "object" && !Array.isArray(moduleNativeView.query)
    ? moduleNativeView.query as JsonRecord
    : undefined;
  const moduleViewQuery = moduleViewQueryRecord
    ? Object.entries(moduleViewQueryRecord)
        .map(([key, value]) => `${key}: ${Array.isArray(value) ? value.map((entry) => queryText(entry)).filter(Boolean).join(", ") : queryText(value)}`)
        .filter((entry) => !entry.endsWith(": "))
        .join("; ")
    : "";
  const moduleViewFilters = Array.isArray(moduleViewQueryRecord?.filters)
    ? (moduleViewQueryRecord.filters as unknown[])
        .filter((entry): entry is JsonRecord => Boolean(entry) && typeof entry === "object" && !Array.isArray(entry))
        .map((entry) => ({ field: text(entry.field), value: queryText(entry.value) }))
        .filter((entry) => entry.field && entry.value)
    : [];
  const moduleViewMode = ["table", "board", "timeline"].includes(text(moduleNativeView?.mode))
    ? text(moduleNativeView?.mode) as "table" | "board" | "timeline"
    : "table";

  return {
    workspace: {
      fixtureId,
      title: "Operating workspace",
      description: documents.length ? "Documents, typed records, and connected operating context." : "No company documents are supplied by this projection.",
      rootSelected: true,
      tree: workspaceTree,
      archive: workspaceArchive,
      spaces: [...docsBySpace].map(([space, entries]) => ({ id: `space:${space}`, name: space, href: docsDocumentHref(text(entries[0]?.id)), countLabel: `${entries.length} page${entries.length === 1 ? "" : "s"}` })),
      recentlyUpdated: linkEntries(documents.map(documentLink)),
      templates: templateLinks,
      templateRecordPolicy: templatePolicy,
      databases: typedRecords.map((entry) => ({ id: text(entry.id), label: text(entry.record_type, "Typed record"), kind: "record" as const })),
      maintainers: linkEntries([
        actorLink(actors, proposal?.proposed_by_ref ?? proposal?.proposed_by),
        ...moduleActors.filter((actor) => actor.actorType === "Agent Membership"),
      ]).filter((actor, index, values) => values.findIndex((candidate) => candidate.id === actor.id) === index),
      structureNotes: [
        ...(module ? [{ label: "Module state", value: humanize(module.status) || "Reported", tone: /proposed|pending/i.test(text(module.status)) ? "warning" as const : "neutral" as const }] : []),
        { label: "Docs health", value: health.status === "pass" ? "Passing" : `${health.counts.findings} findings`, tone: health.status === "pass" ? "neutral" as const : "warning" as const },
      ],
      structureLinks,
      suggestions: linkEntries([sourceLink, applicationLink, selectedWorkLink, approvalLink, financeLink]),
      proposal: proposalLink,
      authoringCommands: governanceCommands,
    },
    moduleView: {
      fixtureId,
      id: module ? text(module.id) : undefined,
      title: module ? text(module.name, "Unnamed module") : "No module selected",
      description: module ? "Native TypedRecords in this BusinessModule, with linked Documents, Work, Approvals, and Finance shown as references." : "No business module records are supplied.",
      provenance: {
        moduleId: text(module?.id) || undefined,
        moduleLabel: text(module?.name) || undefined,
        viewId: text(moduleNativeView?.id) || undefined,
        viewTitle: text(moduleNativeView?.title) || undefined,
        sourceKinds: moduleViewSourceKinds.length ? moduleViewSourceKinds : ["typed_record"],
        querySummary: moduleViewQuery || (module ? `module_id: ${text(module.id)}` : "No module scope supplied"),
        recordCount: moduleRecords.length,
      },
      configuration: {
        mode: moduleViewMode,
        sourceKinds: moduleViewSourceKinds.length ? moduleViewSourceKinds : ["typed_record"],
        filters: moduleViewFilters,
        groupBy: text(moduleViewQueryRecord?.group_by) || undefined,
        sortBy: text(moduleViewQueryRecord?.sort_by) || undefined,
        query: moduleViewQueryRecord,
      },
      records: moduleRecords,
      columns: [
        { id: "title", label: "Record", cell: (entry) => entry.title },
        { id: "status", label: "Status", cell: (entry) => entry.status ?? "—" },
        { id: "links", label: "Connected", cell: (entry) => entry.links?.map((link) => link.label).join(", ") ?? "—" },
      ],
      availableViews: ["table", "board", "timeline"],
      sourceLinks: linkEntries([sourceLink, applicationLink, proposalLink, ...moduleActors]),
      resultLinks: linkEntries([selectedWorkLink, financeLink, ...moduleConnectedLinks.filter((link) => ["work", "approval", "finance"].includes(link.kind ?? ""))]),
      authoring: moduleAuthoring,
      fallback: { label: "Open standard record view", description: "The standard record view remains available if a custom module page is unavailable." },
    },
    home: {
      fixtureId,
      title: "Company home",
      subtitle: "Company context supplied by the current projection.",
      decisionRequired: approvalLink,
      decisionSummary,
      decisionActor: decisionActor ? { id: decisionActor.id, name: decisionActor.label, kind: decisionActor.actorType === "Human" ? "human" : decisionActor.actorType === "Agent Membership" ? "agent" : decisionActor.actorType === "External" ? "external" : "service" } : undefined,
      decisionRequester,
      decisionCollaborators,
      changes: linkEntries([sourceLink, applicationLink, selectedWorkLink, financeLink]),
      workSummary: work ? [{ id: text(work.id), label: "Open work", value: "1", detail: text(work.title) }] : [],
      financeSummary: financeLink ? [{ id: financeLink.id, label: text(financial?.display_name, "Financial record"), value: text(financial?.display_amount, "—"), detail: financialType ? `Record type: ${financialType}` : undefined, financialRecordType: financeLink.financialRecordType }] : [],
    },
    health,
  };
}

/** @deprecated Use adaptCompanyOsDocsProjection for fixture and live projections alike. */
export const adaptTrademarkDocsFixture = adaptCompanyOsDocsProjection;
