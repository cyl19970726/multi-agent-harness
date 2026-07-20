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
  updatedAt: string;
}

export interface FinancialRecordView {
  id: string;
  label: string;
  type: "budget" | "commitment" | "invoice" | "payment" | "refund";
  amount: string;
  status: "pending_approval" | "approved" | "settled";
  sourceDocument: RelatedLink;
  /** Optional accounting/project context; absence is rendered as unknown, never guessed. */
  project?: RelatedLink;
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
