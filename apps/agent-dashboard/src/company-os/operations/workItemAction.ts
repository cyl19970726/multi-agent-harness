import type {
  WorkItemTransitionCommand,
  WorkItemTransitionStatus,
  WorkItemView,
} from "./types";

export function buildWorkItemTransitionCommand(params: {
  workItem: WorkItemView;
  targetStatus: WorkItemTransitionStatus;
  note: string;
  commandId: string;
  transitionedAt: string;
}): WorkItemTransitionCommand {
  const context = params.workItem.transitionContext;
  if (!context) throw new Error("The Store projection does not expose a governed WorkItem transition contract");
  const note = params.note.trim();
  if (!note) throw new Error("A durable transition note is required");
  const requestedBy = params.targetStatus === "completed"
    ? context.accountableOwner
    : context.assignees[0] ?? context.accountableOwner;
  const record = {
    ...context.record,
    status: params.targetStatus,
    outcome_summary: note,
    updated_at: params.transitionedAt,
    completed_at: params.targetStatus === "completed" ? params.transitionedAt : null,
  };
  return {
    id: params.commandId,
    command_name: "work_item.transition",
    subject_ref: { kind: "work_item", id: params.workItem.id },
    requested_by: { ...requestedBy },
    payload: { definition_id: context.definitionId, record },
    required_permission: "company.work.execute",
    policy_ref: context.actionPolicyRef,
    risk_tier: "r2",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [`${params.commandId}:policy-authorized`],
    requested_at: params.transitionedAt,
    completed_at: null,
  };
}
