import type {
  CustomPageDefinition,
  CustomPagePackageManifest,
  CustomPageRenderer,
  PageRuntimeCapabilities,
} from "./types";

export interface TrademarkApplicationSummary {
  id: string;
  title: string;
  status: string;
  nextMilestone?: string;
}

export interface TrademarkWorkSummary {
  id: string;
  title: string;
  assignee: string;
  status: string;
}

export interface TrademarkApprovalSummary {
  id: string;
  title: string;
  status: string;
  approver: string;
}

export interface TrademarkFinancialRecord {
  id: string;
  kind: "Commitment" | "Invoice" | "Payment" | "Refund";
  amount: number;
  currency: string;
  status: string;
}

export interface TrademarkParticipantSummary {
  id: string;
  name: string;
  kind: "human" | "standing_agent" | "external" | "service";
  responsibility: string;
}

export interface TrademarkModuleProps {
  pageTitle: string;
  focusApplicationId?: string;
}

export interface TrademarkModuleComposition {
  title: string;
  sourceApplicationId?: string;
  sections: readonly {
    id: "pipeline" | "work" | "approvals" | "finance" | "participants";
    title: string;
    items: readonly unknown[];
  }[];
  finance: {
    currency: string;
    committedAmount: number;
    paymentAmount: number;
    paymentRecordIds: readonly string[];
  };
}

async function renderTrademarkModule(
  props: Readonly<TrademarkModuleProps>,
  capabilities: PageRuntimeCapabilities,
): Promise<TrademarkModuleComposition> {
  const queryParams = props.focusApplicationId
    ? { applicationId: props.focusApplicationId }
    : {};
  const [applications, work, approvals, financialRecords, participants] = await Promise.all([
    capabilities.queries.query<TrademarkApplicationSummary[]>("trademark.applications", queryParams),
    capabilities.queries.query<TrademarkWorkSummary[]>("trademark.work", queryParams),
    capabilities.queries.query<TrademarkApprovalSummary[]>("trademark.approvals", queryParams),
    capabilities.queries.query<TrademarkFinancialRecord[]>("trademark.finance", queryParams),
    capabilities.queries.query<TrademarkParticipantSummary[]>("trademark.participants", queryParams),
  ]);

  const commitments = financialRecords.filter((record) => record.kind === "Commitment");
  const payments = financialRecords.filter((record) => record.kind === "Payment");
  const currency = financialRecords[0]?.currency ?? "CNY";

  return {
    title: props.pageTitle,
    sourceApplicationId: props.focusApplicationId,
    sections: [
      { id: "pipeline", title: "Application pipeline", items: applications },
      { id: "work", title: "Current work", items: work },
      { id: "approvals", title: "Needs you", items: approvals },
      { id: "finance", title: "Finance", items: financialRecords },
      { id: "participants", title: "Participants", items: participants },
    ],
    finance: {
      currency,
      committedAmount: commitments.reduce((sum, record) => sum + record.amount, 0),
      paymentAmount: payments.reduce((sum, record) => sum + record.amount, 0),
      paymentRecordIds: payments.map((record) => record.id),
    },
  };
}

export const trademarkModuleRenderer: CustomPageRenderer<
  TrademarkModuleProps,
  TrademarkModuleComposition
> = Object.freeze({ render: renderTrademarkModule });

export const trademarkModuleDefinition: CustomPageDefinition = Object.freeze({
  id: "company-os.trademark.module-home",
  version: "1.0.0",
  purpose: "Show the legal, work, approval, finance, and participant state of trademark operations",
  primaryQuestion: "Which application needs a decision or legal action, and what cost is committed?",
  ownerActorId: "human:brand-owner",
  moduleId: "module:trademark-management",
  fixtureId: "company-os-trademark-v1",
  componentVersion: "company-os-ui@1",
  package: { id: "company-os.trademark.module-home.react", version: "1.0.0" },
  queries: [
    { name: "trademark.applications", viewId: "view:trademark-applications", recordTypes: ["TrademarkApplication"] },
    { name: "trademark.work", viewId: "view:trademark-work", recordTypes: ["WorkItem"] },
    { name: "trademark.approvals", viewId: "view:trademark-approvals", recordTypes: ["Approval"] },
    { name: "trademark.finance", viewId: "view:trademark-finance", recordTypes: ["FinancialRecord"] },
    { name: "trademark.participants", viewId: "view:trademark-participants", recordTypes: ["ActorRef"] },
  ],
  actions: [
    {
      name: "trademark.application.create",
      sensitive: false,
      humanApproval: "not_required",
      allowedEffectKinds: ["TrademarkApplication", "AuditEvent"],
    },
    {
      name: "finance.commitment.request",
      sensitive: false,
      humanApproval: "not_required",
      allowedEffectKinds: ["FinancialCommitment", "Approval", "AuditEvent"],
    },
    {
      name: "finance.commitment.authorize",
      sensitive: true,
      humanApproval: "required",
      allowedEffectKinds: ["FinancialCommitment", "AuditEvent"],
    },
    {
      name: "document.open",
      sensitive: false,
      humanApproval: "not_required",
      allowedEffectKinds: [],
    },
  ],
  fallback: {
    title: "Trademark Management — standard views",
    owningDocumentId: "document:trademark-management",
    viewIds: [
      "view:trademark-applications",
      "view:trademark-work",
      "view:trademark-approvals",
      "view:trademark-finance",
    ],
    nextActions: ["document.open", "approval.request"],
  },
} satisfies CustomPageDefinition);

export const trademarkModulePackage: CustomPagePackageManifest = Object.freeze({
  id: "company-os.trademark.module-home.react",
  definitionId: "company-os.trademark.module-home",
  version: "1.0.0",
  format: "react-component",
  entryPoint: "trademarkModuleRenderer",
  integrity: "sha256-fixture-company-os-trademark-v1",
  capabilities: {
    queries: [
      "trademark.applications",
      "trademark.work",
      "trademark.approvals",
      "trademark.finance",
      "trademark.participants",
    ],
    actions: [
      "trademark.application.create",
      "finance.commitment.request",
      "finance.commitment.authorize",
      "document.open",
    ],
    components: ["PageHeader", "DataTable", "ApprovalPanel", "FinanceSummary", "ActorList"],
  },
} satisfies CustomPagePackageManifest);
