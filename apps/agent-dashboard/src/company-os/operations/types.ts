import type { ReactNode } from "react";

/**
 * These presentation contracts intentionally preserve Company OS boundaries.
 * An actor is not a provider session, and a MemberRun is not an organization
 * member. Containers can adapt API records into these small view models.
 */
export type ActorKind = "human" | "standing_agent" | "external" | "service";
export type ActorAvailability = "available" | "away" | "unavailable";

export interface ActorSummary {
  id: string;
  name: string;
  kind: ActorKind;
  role: string;
  unit?: string;
  /** Only render when it is explicitly reported by an organization record. */
  availability?: ActorAvailability;
  /** Organization role state, not a provider or runtime state. */
  organizationRoleState?: "proposed" | "active" | "paused";
  membershipRole?: "lead" | "member" | "advisor" | "observer" | "external_partner";
  responsibilitySummary?: string;
  systemPromptRef?: string;
  toolRefs?: string[];
  skillRefs?: string[];
  maintainedDocumentRefs?: string[];
  acceptedWorkTypeRefs?: string[];
  permissionPolicyRefs?: string[];
  escalationPolicyRef?: string;
}

export interface RelatedLink {
  id: string;
  label: string;
  detail?: string;
}

export interface OrganizationUnitView extends RelatedLink {
  parentId?: string;
  humanLeadActorId?: string;
  agentLeadActorId?: string;
  actorIds: string[];
}

export interface WorkItemView {
  id: string;
  title: string;
  status: "waiting_for_approval" | "in_progress" | "in_review" | "completed" | "blocked";
  sourceDocument: RelatedLink;
  requestedBy: ActorSummary;
  submittedBy: ActorSummary;
  accountableOwner: ActorSummary;
  assignees: ActorSummary[];
  contributors: ActorSummary[];
  reviewer?: ActorSummary;
  legalReviewer?: ActorSummary;
  approver?: ActorSummary;
  outcomeSummary?: string;
  updatedAt: string;
  /** Present only when Store truth declares the governed lifecycle Action. */
  transitionContext?: WorkItemTransitionContext;
}

export interface AssignmentView {
  id: string;
  workItemId: string;
  recipient: ActorSummary;
  sender: ActorSummary;
  assignedRole: string;
  scope: string;
  deliveryState: "pending" | "delivered" | "acknowledged" | "failed" | "cancelled";
  correlationId: string;
  deliveryEvidenceRef?: string;
  assignedAt: string;
}

export type WorkItemTransitionStatus = "in_progress" | "blocked" | "in_review" | "completed";

export interface WorkItemTransitionContext {
  definitionId: string;
  actionPolicyRef: string;
  record: Record<string, unknown>;
  accountableOwner: CanonicalActorRef;
  assignees: CanonicalActorRef[];
  reviewer?: CanonicalActorRef;
}

export interface WorkItemTransitionCommand {
  id: string;
  command_name: "work_item.transition";
  subject_ref: CanonicalEntityRef;
  requested_by: CanonicalActorRef;
  payload: { definition_id: string; record: Record<string, unknown> };
  required_permission: "company.work.execute";
  policy_ref: string;
  risk_tier: "r2";
  requires_human_approval: false;
  approval_refs: [];
  status: "requested";
  audit_event_refs: string[];
  requested_at: string;
  completed_at: null;
}

export interface FinancialRecordView {
  id: string;
  label: string;
  type: "budget" | "commitment" | "invoice" | "payment" | "refund";
  amount: string;
  status: "pending_approval" | "approved" | "settled";
  sourceDocument: RelatedLink;
  /** Optional business/cost context; absence is rendered as unknown, never guessed. */
  costContext?: RelatedLink;
  accountableOwner: ActorSummary;
}

export interface ApprovalView {
  id: string;
  title: string;
  actionSummary: string;
  status: "requested" | "approved" | "rejected" | "expired";
  requestedBy: ActorSummary;
  requiredApprover: ActorSummary;
  financeReviewer?: ActorSummary;
  legalReviewer?: ActorSummary;
  expiresAt?: string;
  /** Present only when the Store projection supplies a complete governed action contract. */
  decisionContext?: ApprovalDecisionContext;
}

export interface CanonicalActorRef {
  actor_type: "human" | "agent" | "external" | "service";
  actor_id: string;
}

export interface CanonicalEntityRef {
  kind: string;
  id: string;
}

export interface ApprovalDecisionContext {
  definitionId: string;
  actionPolicyRef: string;
  recordSubjectRef: CanonicalEntityRef;
  requestedBy: CanonicalActorRef;
  requiredApproverRefs: CanonicalActorRef[];
  requiredActorType?: string;
  recordPolicyRef: string;
  rawActionSummary: string;
  evidenceRefs: string[];
  requestedAt: string;
  expiresAt?: string;
}

export type ApprovalDecision = "approved" | "rejected";

export interface ApprovalDecisionCommand {
  id: string;
  command_name: "approval.decide";
  subject_ref: CanonicalEntityRef;
  requested_by: CanonicalActorRef;
  payload: {
    definition_id: string;
    record: Record<string, unknown>;
  };
  required_permission: "company.approve";
  policy_ref: string;
  risk_tier: "r2";
  requires_human_approval: false;
  approval_refs: [];
  status: "requested";
  audit_event_refs: string[];
  requested_at: string;
  completed_at: null;
}

/**
 * The single read-only view model consumed by all operations pages.  It is an
 * adapter output, not a second store: callers pass the already-resolved Company
 * OS projection from the same source used by Docs.
 */
export interface TrademarkOperationsProjection {
  fixtureId?: string;
  actors: Record<string, ActorSummary>;
  actorList: ActorSummary[];
  organization: {
    company: RelatedLink;
    brandUnit: RelatedLink;
    units: OrganizationUnitView[];
  };
  sourceDocument: RelatedLink;
  contentPlanDocument: RelatedLink;
  typedApplication: RelatedLink;
  workItem: WorkItemView;
  workItems?: WorkItemView[];
  assignments?: AssignmentView[];
  commitment: FinancialRecordView;
  approval: ApprovalView;
  evidence: RelatedLink[];
  governanceProposal: RelatedLink & { proposedById?: string };
  businessModule: RelatedLink;
  julySpendMetric: RelatedLink;
  julySpendAmount: string;
}

export interface PageFrameProps {
  eyebrow: string;
  title: string;
  description?: string;
  action?: ReactNode;
  children: ReactNode;
  context?: ReactNode;
  /** Dense is opt-in for spatially structured pages such as Organization. */
  dense?: boolean;
}
