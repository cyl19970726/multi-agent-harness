import type { CompanyOsDocsActionCommand, CompanyOsStructuredViewData } from "./types";

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
