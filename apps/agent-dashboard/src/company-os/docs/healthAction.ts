import type { CompanyOsHealthFinding, CompanyOsRelationRepairCommand } from "./types";

function slug(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 72) || "docs-health-finding";
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
