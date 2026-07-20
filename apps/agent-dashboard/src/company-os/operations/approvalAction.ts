import type {
  ApprovalDecision,
  ApprovalDecisionCommand,
  ApprovalView,
} from "./types";

export function buildApprovalDecisionCommand(params: {
  approval: ApprovalView;
  decision: ApprovalDecision;
  note: string;
  commandId: string;
  decidedAt: string;
}): ApprovalDecisionCommand {
  const context = params.approval.decisionContext;
  if (!context) throw new Error("The Store projection does not expose a governed approval action contract");
  const note = params.note.trim();
  if (!note) throw new Error("A durable decision note is required");
  const decider = context.requiredApproverRefs[0];
  if (!decider || decider.actor_type !== "human") {
    throw new Error("The approval does not name a Human approver");
  }
  const auditId = `${params.commandId}:policy-authorized`;
  return {
    id: params.commandId,
    command_name: "approval.decide",
    subject_ref: { kind: "approval", id: params.approval.id },
    requested_by: { ...decider },
    payload: {
      definition_id: context.definitionId,
      record: {
        id: params.approval.id,
        subject_ref: { ...context.recordSubjectRef },
        action_summary: context.rawActionSummary,
        requested_by: { ...context.requestedBy },
        required_approver_refs: context.requiredApproverRefs.map((actor) => ({ ...actor })),
        required_actor_type: context.requiredActorType ?? null,
        policy_ref: context.recordPolicyRef,
        status: params.decision,
        decided_by: [{ ...decider }],
        decision_note: note,
        evidence_refs: [...context.evidenceRefs],
        requested_at: context.requestedAt,
        decided_at: params.decidedAt,
        expires_at: context.expiresAt ?? null,
      },
    },
    required_permission: "company.approve",
    policy_ref: context.actionPolicyRef,
    risk_tier: "r2",
    requires_human_approval: false,
    approval_refs: [],
    status: "requested",
    audit_event_refs: [auditId],
    requested_at: params.decidedAt,
    completed_at: null,
  };
}
