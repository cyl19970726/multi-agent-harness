import type { ReactNode } from "react";

/**
 * These presentation contracts intentionally preserve Company OS boundaries.
 * An actor is not a provider session, and a MemberRun is not an organization
 * member. Containers can adapt API records into these small view models.
 */
export type ActorKind = "human" | "agent_membership" | "external" | "service";
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
  /** Maintained Documents resolved to title plus lifecycle; archived stays navigable history. */
  maintainedDocuments?: RelatedLink[];
  acceptedWorkTypeRefs?: string[];
  permissionPolicyRefs?: string[];
  escalationPolicyRef?: string;
  executionAgentMemberRef?: string;
}

export interface RelatedLink {
  id: string;
  label: string;
  detail?: string;
  /** Explicit lifecycle of the resolved record; archived stays navigable history. */
  lifecycle?: "archived" | "missing";
}

export interface OrganizationUnitView extends RelatedLink {
  parentId?: string;
  organizationId?: string;
  purpose?: string;
  status?: string;
  humanLeadActorId?: string;
  agentLeadActorId?: string;
  actorIds: string[];
  policyRefs: string[];
  documentSpaceRef?: string;
}

export interface OrganizationMembershipView {
  id: string;
  organizationId?: string;
  orgUnitId: string;
  actorId: string;
  membershipRole?: "lead" | "member" | "advisor" | "observer" | "external_partner";
  titleOrFunction?: string;
  status?: string;
  startsAt?: string;
  endsAt?: string;
  authorityPolicyRefs: string[];
}

export interface OrganizationIntegrityFinding {
  id: string;
  kind:
    | "empty_organization"
    | "duplicate_unit"
    | "orphan_parent"
    | "parent_cycle"
    | "unknown_membership_unit"
    | "unknown_membership_actor"
    | "duplicate_membership"
    | "invalid_human_lead"
    | "invalid_agent_lead"
    | "unassigned_actor";
  severity: "info" | "warning" | "error";
  detail: string;
  unitIds: string[];
  actorIds: string[];
}

/** Read-only execution participation joined by explicit MemberRun.agent_member_id. */
export interface AgentMemberExecutionAssignment {
  id: string;
  agentMemberId: string;
  sourceKind: "agent_team_work" | "agent_team_participation";
  sourceRef?: string;
  workId?: string;
  missionId?: string;
  waveId?: string;
  teamRunId: string;
  memberRunId: string;
  title: string;
  role: string;
  status: string;
  assignedAt: string;
  lastActivityAt?: string;
  nativeSession?: {
    provider?: string;
    execution_mode?: string;
    native_session_id?: string;
    availability?: string;
  };
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
    units: OrganizationUnitView[];
    memberships: OrganizationMembershipView[];
    rootUnitIds: string[];
    unplacedUnitIds: string[];
    unassignedActorIds: string[];
    integrityFindings: OrganizationIntegrityFinding[];
  };
  sourceDocument: RelatedLink;
  contentPlanDocument: RelatedLink;
  typedApplication: RelatedLink;
  linkedWork?: RelatedLink & {
    phase?: "open" | "active" | "review" | "closed";
    condition?: "normal" | "blocked" | "on_hold";
    resolution?: "accepted" | "cancelled" | "failed";
  };
  /** Relations explicitly recorded against the selected TeamWork. */
  linkedTypedRecords?: RelatedLink[];
  linkedApproval?: ApprovalView;
  linkedCommitment?: FinancialRecordView;
  membershipProjections?: AgentMemberExecutionAssignment[];
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
