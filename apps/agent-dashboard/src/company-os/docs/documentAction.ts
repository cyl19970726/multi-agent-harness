import type { CompanyOsDocsActionCommand, CompanyOsDocumentPageData, CompanyOsStructuredViewData, CompanyOsTemplateOption } from "./types";

function slug(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64) || "document";
}

function commandId(prefix: string): string {
  return `${prefix}-${crypto.randomUUID()}`;
}

export function buildDocsChildDocumentCommand(params: {
  document: CompanyOsDocumentPageData;
  title: string;
  templateRef?: string | null;
  commandId?: string;
  createdAt: string;
}): CompanyOsDocsActionCommand {
  const context = params.document.authoring;
  if (!context) throw new Error("This document does not expose a governed document.append authoring contract");
  const title = params.title.trim();
  if (!title) throw new Error("A child document title is required");
  const id = params.commandId ?? commandId("action-browser-docs-document");
  const documentId = `document-browser-${slug(title)}-${id.slice(-8)}`;
  const record = {
    id: documentId,
    space_id: context.spaceId,
    parent_document_id: context.documentId,
    title,
    kind: "page",
    lifecycle_status: "draft",
    block_ids: [],
    template_ref: params.templateRef?.trim() || null,
    permission_policy_refs: [...context.permissionPolicyRefs],
    reference_refs: [],
    created_by: { ...context.requestedBy },
    updated_by: { ...context.requestedBy },
    created_at: params.createdAt,
    updated_at: params.createdAt,
  };
  return {
    id,
    command_name: "document.append",
    subject_ref: { kind: "document", id: context.documentId },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record },
    required_permission: "company.records.write",
    policy_ref: context.documentPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${id}:policy-authorized`],
    requested_at: params.createdAt,
    completed_at: null,
  };
}

export function buildDocsInstantiateTemplateBlockCommands(params: {
  parentDocument: CompanyOsDocumentPageData;
  childDocumentCommand: CompanyOsDocsActionCommand;
  template: CompanyOsTemplateOption;
  commandId?: string;
  createdAt: string;
}): CompanyOsDocsActionCommand[] {
  const context = params.parentDocument.authoring;
  if (!context) throw new Error("This document does not expose a governed template Block instantiation contract");
  if (params.childDocumentCommand.command_name !== "document.append") throw new Error("Template Block instantiation must start from a governed child document.append ActionCommand");
  const childRecord = params.childDocumentCommand.payload.record;
  const childDocumentId = typeof childRecord.id === "string" ? childRecord.id : "";
  if (!childDocumentId) throw new Error("Child Document record id is required before copying template Blocks");
  if (!params.template.templateBlocks.length) return [];
  const baseId = params.commandId ?? commandId("action-browser-docs-template");
  const copiedBlockIds: string[] = [];
  const commands: CompanyOsDocsActionCommand[] = [];
  params.template.templateBlocks.forEach((templateBlock, index) => {
    const copiedBlockId = `block-browser-template-${slug(childDocumentId)}-${index + 1}-${slug(templateBlock.id)}`;
    copiedBlockIds.push(copiedBlockId);
    const blockCommandId = `${baseId}-block-${index + 1}`;
    commands.push({
      id: blockCommandId,
      command_name: "block.append",
      subject_ref: { kind: "document", id: childDocumentId },
      requested_by: { ...context.requestedBy },
      payload: {
        definition_id: context.definitionId,
        record: {
          id: copiedBlockId,
          document_id: childDocumentId,
          kind: templateBlock.kind,
          position: index,
          content: { ...templateBlock.content },
          referenced_entities: templateBlock.referencedEntities.map((ref) => ({ ...ref })),
          created_by: { ...context.requestedBy },
          updated_by: { ...context.requestedBy },
          created_at: params.createdAt,
          updated_at: params.createdAt,
        },
      },
      required_permission: "company.records.write",
      policy_ref: context.blockPolicyRef,
      risk_tier: "r1",
      requires_human_approval: false,
      approval_refs: [],
      status: "requested",
      audit_event_refs: [`${blockCommandId}:policy-authorized`],
      requested_at: params.createdAt,
      completed_at: null,
    });
    const documentCommandId = `${baseId}-document-${index + 1}`;
    commands.push({
      id: documentCommandId,
      command_name: "document.append",
      subject_ref: { kind: "document", id: childDocumentId },
      requested_by: { ...context.requestedBy },
      payload: {
        definition_id: context.definitionId,
        record: {
          ...childRecord,
          block_ids: [...copiedBlockIds],
          updated_by: { ...context.requestedBy },
          updated_at: params.createdAt,
        },
      },
      required_permission: "company.records.write",
      policy_ref: context.documentPolicyRef,
      risk_tier: "r1",
      requires_human_approval: false,
      approval_refs: [],
      status: "requested",
      audit_event_refs: [`${documentCommandId}:policy-authorized`],
      requested_at: params.createdAt,
      completed_at: null,
    });
  });
  return commands;
}

export function buildDocsAppendBlockCommands(params: {
  document: CompanyOsDocumentPageData;
  text: string;
  blockKind?: "rich_text" | "heading" | "callout" | "table";
  calloutTitle?: string;
  commandId?: string;
  createdAt: string;
}): [CompanyOsDocsActionCommand, CompanyOsDocsActionCommand] {
  const context = params.document.authoring;
  if (!context) throw new Error("This document does not expose a governed block.append authoring contract");
  const text = params.text.trim();
  if (!text) throw new Error("Block text is required");
  const blockKind = params.blockKind ?? "rich_text";
  const baseId = params.commandId ?? commandId("action-browser-docs-block");
  const blockId = `block-browser-${slug(text)}-${baseId.slice(-8)}`;
  const tableColumns = text.split("\n")[0]?.split("|").map((entry) => entry.trim()).filter(Boolean) ?? [];
  const content = blockKind === "table"
    ? {
        columns: tableColumns.length ? tableColumns : ["Column"],
        rows: text.split("\n").slice(1).filter(Boolean).map((line) => line.split("|").map((entry) => entry.trim())),
      }
    : blockKind === "callout"
      ? { title: params.calloutTitle?.trim() || "Note", text, tone: "neutral" }
      : { text };
  const blockRecord = {
    id: blockId,
    document_id: context.documentId,
    kind: blockKind,
    position: context.blockIds.length,
    content,
    referenced_entities: [],
    created_by: { ...context.requestedBy },
    updated_by: { ...context.requestedBy },
    created_at: params.createdAt,
    updated_at: params.createdAt,
  };
  const blockCommand: CompanyOsDocsActionCommand = {
    id: baseId,
    command_name: "block.append",
    subject_ref: { kind: "document", id: context.documentId },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record: blockRecord },
    required_permission: "company.records.write",
    policy_ref: context.blockPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${baseId}:policy-authorized`],
    requested_at: params.createdAt,
    completed_at: null,
  };
  const documentUpdateId = `${baseId}-document-update`;
  const documentRecord = {
    id: context.documentId,
    space_id: context.spaceId,
    parent_document_id: context.parentDocumentId,
    title: params.document.title,
    kind: context.documentKind,
    lifecycle_status: context.lifecycleStatus,
    block_ids: [...context.blockIds, blockId],
    template_ref: context.templateRef ?? null,
    permission_policy_refs: [...context.permissionPolicyRefs],
    reference_refs: context.referenceRefs.map((ref) => ({ ...ref })),
    created_by: { ...context.createdBy },
    updated_by: { ...context.requestedBy },
    created_at: context.createdAt,
    updated_at: params.createdAt,
  };
  const documentCommand: CompanyOsDocsActionCommand = {
    id: documentUpdateId,
    command_name: "document.append",
    subject_ref: { kind: "document", id: context.documentId },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record: documentRecord },
    required_permission: "company.records.write",
    policy_ref: context.documentPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${documentUpdateId}:policy-authorized`],
    requested_at: params.createdAt,
    completed_at: null,
  };
  return [blockCommand, documentCommand];
}

export function buildDocsReorderBlocksCommand(params: {
  document: CompanyOsDocumentPageData;
  blockIds: string[];
  commandId?: string;
  updatedAt: string;
}): CompanyOsDocsActionCommand {
  const context = params.document.authoring;
  if (!context) throw new Error("This document does not expose a governed document.append authoring contract");
  const existing = [...context.blockIds];
  const next = params.blockIds.map((id) => id.trim()).filter(Boolean);
  if (next.length !== existing.length || new Set(next).size !== next.length || existing.some((id) => !next.includes(id))) {
    throw new Error("Block reorder must preserve exactly the existing Document.block_ids set");
  }
  const id = params.commandId ?? commandId("action-browser-docs-block-reorder");
  const documentRecord = {
    id: context.documentId,
    space_id: context.spaceId,
    parent_document_id: context.parentDocumentId,
    title: params.document.title,
    kind: context.documentKind,
    lifecycle_status: context.lifecycleStatus,
    block_ids: next,
    template_ref: context.templateRef ?? null,
    permission_policy_refs: [...context.permissionPolicyRefs],
    reference_refs: context.referenceRefs.map((ref) => ({ ...ref })),
    created_by: { ...context.createdBy },
    updated_by: { ...context.requestedBy },
    created_at: context.createdAt,
    updated_at: params.updatedAt,
  };
  return {
    id,
    command_name: "document.append",
    subject_ref: { kind: "document", id: context.documentId },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record: documentRecord },
    required_permission: "company.records.write",
    policy_ref: context.documentPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${id}:policy-authorized`],
    requested_at: params.updatedAt,
    completed_at: null,
  };
}

export function buildDocsTypedRecordCommand(params: {
  view: CompanyOsStructuredViewData;
  title: string;
  recordType: string;
  commandId?: string;
  createdAt: string;
}): CompanyOsDocsActionCommand {
  const context = params.view.authoring;
  if (!context) throw new Error("This module view does not expose a governed typed_record.append authoring contract");
  const title = params.title.trim();
  const recordType = params.recordType.trim();
  if (!title) throw new Error("A TypedRecord title is required");
  if (!recordType) throw new Error("A TypedRecord type is required");
  const id = params.commandId ?? commandId("action-browser-docs-typed-record");
  const recordId = `typed-record-browser-${slug(title)}-${id.slice(-8)}`;
  const record = {
    id: recordId,
    module_id: context.moduleId,
    record_type: recordType,
    title,
    fields: {},
    lifecycle_status: "draft",
    source_document_ref: context.sourceDocumentId,
    created_by: { ...context.requestedBy },
    updated_by: { ...context.requestedBy },
    created_at: params.createdAt,
    updated_at: params.createdAt,
  };
  return {
    id,
    command_name: "typed_record.append",
    subject_ref: { kind: "document", id: context.sourceDocumentId },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record },
    required_permission: "company.records.write",
    policy_ref: context.typedRecordPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${id}:policy-authorized`],
    requested_at: params.createdAt,
    completed_at: null,
  };
}

export function buildDocsViewCommand(params: {
  view: CompanyOsStructuredViewData;
  title: string;
  mode?: "table" | "board" | "timeline";
  sourceKinds?: string[];
  query?: Record<string, unknown>;
  commandId?: string;
  createdAt: string;
}): CompanyOsDocsActionCommand {
  const context = params.view.authoring;
  if (!context) throw new Error("This module view does not expose a governed view.append authoring contract");
  const title = params.title.trim();
  if (!title) throw new Error("A View title is required");
  const id = params.commandId ?? commandId("action-browser-docs-view");
  const viewId = `view-browser-${slug(title)}-${id.slice(-8)}`;
  const sourceKinds = params.sourceKinds?.map((entry) => entry.trim()).filter(Boolean);
  const record = {
    id: viewId,
    module_id: context.moduleId,
    title,
    mode: params.mode ?? "table",
    source_kinds: sourceKinds?.length ? sourceKinds : ["typed_record"],
    query: params.query ?? {},
    owner: { ...context.requestedBy },
    policy_refs: ["company.records.write"],
    created_at: params.createdAt,
    updated_at: params.createdAt,
  };
  return {
    id,
    command_name: "view.append",
    subject_ref: { kind: "business_module", id: context.moduleId },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record },
    required_permission: "company.records.write",
    policy_ref: context.viewPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${id}:policy-authorized`],
    requested_at: params.createdAt,
    completed_at: null,
  };
}

export function buildDocsRelationCommand(params: {
  view: CompanyOsStructuredViewData;
  typedRecordId: string;
  commandId?: string;
  createdAt: string;
}): CompanyOsDocsActionCommand {
  const context = params.view.authoring;
  if (!context) throw new Error("This module view does not expose a governed relation.append authoring contract");
  const typedRecordId = params.typedRecordId.trim();
  if (!typedRecordId) throw new Error("A TypedRecord id is required");
  const id = params.commandId ?? commandId("action-browser-docs-relation");
  const relationId = `relation-browser-${slug(context.sourceDocumentId)}-${slug(typedRecordId)}-${id.slice(-8)}`;
  const record = {
    id: relationId,
    from_ref: { kind: "document", id: context.sourceDocumentId },
    relation_type: "source_for",
    to_ref: { kind: "typed_record", id: typedRecordId },
    provenance_ref: { kind: "document", id: context.sourceDocumentId },
    created_by: { ...context.requestedBy },
    created_at: params.createdAt,
  };
  return {
    id,
    command_name: "relation.append",
    subject_ref: { kind: "document", id: context.sourceDocumentId },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record },
    required_permission: "company.records.write",
    policy_ref: context.relationPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${id}:policy-authorized`],
    requested_at: params.createdAt,
    completed_at: null,
  };
}
