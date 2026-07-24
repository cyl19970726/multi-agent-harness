import type { CompanyOsCorrectiveWorkCommand, CompanyOsHealthFinding, CompanyOsRelationRepairCommand } from "./types";

function slug(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 72) || "docs-health-finding";
}

export function buildDocsHealthCorrectiveWorkCommand(params: {
  finding: CompanyOsHealthFinding;
  note: string;
  commandId: string;
  createdAt: string;
}): CompanyOsCorrectiveWorkCommand {
  const context = params.finding.correctiveWorkContext;
  if (!context) throw new Error("The selected finding does not expose a governed corrective WorkItem contract");
  const note = params.note.trim();
  if (!note) throw new Error("A durable corrective-work note is required");
  const workItemId = `work-docs-health-${slug(params.finding.id)}-${params.commandId.slice(-8)}`;
  const record = {
    id: workItemId,
    title: `Fix Docs health: ${params.finding.title}`,
    objective: `${params.finding.recommendedAction}\n\nFinding detail: ${params.finding.detail}\n\nOperator note: ${note}`,
    status: "submitted",
    source_document_ref: context.sourceDocument.id,
    source_record_refs: [...context.sourceRecordRefs],
    milestone_ref: null,
    work_type: "governance",
    business_module_ref: context.businessModuleRef ?? null,
    result_document_ref: null,
    result_record_refs: [],
    submitted_by: { ...context.submittedBy },
    requested_by: { ...context.requestedBy },
    accountable_owner: { ...context.accountableOwner },
    assignees: context.assignees.map((actor) => ({ ...actor })),
    contributors: [],
    reviewer: context.reviewer ? { ...context.reviewer } : null,
    approver: null,
    execution_mode: "direct",
    execution_refs: [],
    approval_refs: [],
    evidence_refs: [],
    artifact_refs: [],
    outcome_summary: null,
    due_at: null,
    priority: params.finding.severity === "critical" ? "high" : "medium",
    risk_level: "governance",
    created_at: params.createdAt,
    updated_at: params.createdAt,
    completed_at: null,
  };
  return {
    id: params.commandId,
    command_name: "work_item.append",
    subject_ref: { kind: "document", id: context.sourceDocument.id },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record },
    required_permission: "company.records.write",
    policy_ref: context.actionPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${params.commandId}:policy-authorized`],
    requested_at: params.createdAt,
    completed_at: null,
  };
}

export function buildDocsHealthRelationRepairCommand(params: {
  finding: CompanyOsHealthFinding;
  note: string;
  commandId: string;
  createdAt: string;
}): CompanyOsRelationRepairCommand {
  const context = params.finding.relationRepairContext;
  if (!context) throw new Error("The selected finding does not expose a governed relation.append contract");
  if (!params.note.trim()) throw new Error("A durable Docs action note is required");
  const relationId = `relation-docs-health-${slug(params.finding.id)}-${params.commandId.slice(-8)}`;
  const record = {
    id: relationId,
    from_ref: { ...context.from },
    relation_type: context.relationType,
    to_ref: { ...context.to },
    provenance_ref: context.provenanceRef ? { ...context.provenanceRef } : null,
    created_by: { ...context.createdBy },
    created_at: params.createdAt,
  };
  return {
    id: params.commandId,
    command_name: "relation.append",
    subject_ref: { ...context.from },
    requested_by: { ...context.requestedBy },
    payload: { definition_id: context.definitionId, record },
    required_permission: "company.records.write",
    policy_ref: context.actionPolicyRef,
    risk_tier: "r1",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${params.commandId}:policy-authorized`],
    requested_at: params.createdAt,
    completed_at: null,
  };
}
