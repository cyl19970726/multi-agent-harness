import type {
  ActorAvailability,
  ActorKind,
  ActorSummary,
  AssignmentView,
  CanonicalActorRef,
  CanonicalEntityRef,
  ApprovalView,
  FinancialRecordView,
  RelatedLink,
  TrademarkOperationsProjection,
  WorkItemView,
} from "./types";

type JsonRecord = Record<string, unknown>;

function records(value: unknown): JsonRecord[] {
  return Array.isArray(value)
    ? value
        .filter((item): item is JsonRecord => Boolean(item) && typeof item === "object")
        .map((item) => {
          const nested = item.record;
          return nested && typeof nested === "object" && !Array.isArray(nested)
            ? { ...(nested as JsonRecord), ...item }
            : item;
        })
    : [];
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map((item) => text(item)).filter(Boolean) : [];
}

function refId(value: unknown): string {
  if (typeof value === "string") return value;
  if (!value || typeof value !== "object" || Array.isArray(value)) return "";
  const ref = value as JsonRecord;
  return text(ref.actor_id) || text(ref.id);
}

function canonicalActorRef(value: unknown): CanonicalActorRef | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const candidate = value as JsonRecord;
  const actorId = text(candidate.actor_id);
  const actorType = text(candidate.actor_type);
  if (!actorId || !new Set(["human", "agent", "external", "service"]).has(actorType)) return undefined;
  return { actor_type: actorType as CanonicalActorRef["actor_type"], actor_id: actorId };
}

function canonicalEntityRef(value: unknown): CanonicalEntityRef | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const candidate = value as JsonRecord;
  const id = text(candidate.id);
  const kind = text(candidate.kind);
  return id && kind ? { id, kind } : undefined;
}

function field(record: JsonRecord | undefined, key: string): unknown {
  if (!record) return undefined;
  if (record[key] !== undefined) return record[key];
  const fields = record.fields;
  return fields && typeof fields === "object" && !Array.isArray(fields)
    ? (fields as JsonRecord)[key]
    : undefined;
}

function find(items: JsonRecord[], id: string): JsonRecord | undefined {
  return items.find((item) => text(item.id) === id);
}

function pick(items: JsonRecord[], preferredId: string): JsonRecord {
  return find(items, preferredId) ?? items[0] ?? {};
}

function actorKind(value: unknown): ActorKind {
  switch (text(value).toLowerCase().replace(/ /g, "_")) {
    case "human": return "human";
    case "standing_agent": case "agent": return "standing_agent";
    case "external": return "external";
    case "service": return "service";
    default: return "service";
  }
}

function workStatus(value: unknown): WorkItemView["status"] {
  switch (text(value)) {
    case "waiting_for_approval": case "in_progress": case "in_review": case "completed": case "blocked": return text(value) as WorkItemView["status"];
    default: return "in_progress";
  }
}

function financialType(value: unknown): FinancialRecordView["type"] {
  switch (text(value)) {
    case "budget": case "commitment": case "invoice": case "payment": case "refund": return text(value) as FinancialRecordView["type"];
    default: return "commitment";
  }
}

function financialStatus(value: unknown): FinancialRecordView["status"] {
  switch (text(value)) {
    case "pending_approval": case "approved": case "settled": return text(value) as FinancialRecordView["status"];
    default: return "pending_approval";
  }
}

function approvalStatus(value: unknown): ApprovalView["status"] {
  switch (text(value)) {
    case "requested": case "approved": case "rejected": case "expired": return text(value) as ApprovalView["status"];
    default: return "requested";
  }
}

function asRef(id: unknown, label: unknown, detail?: unknown): RelatedLink {
  return { id: text(id), label: text(label, "Unresolved record"), detail: text(detail) || undefined };
}

function humanizeEvidenceLabel(value: unknown): string {
  const raw = text(value);
  if (!raw) return "Evidence";
  if (!raw.startsWith("evidence-")) return raw;
  const words = raw
    .replace(/^evidence-/, "")
    .replace(/-(?:[a-z]{2}-)?\d{4}-\d+$/i, "")
    .split("-")
    .filter(Boolean)
    .join(" ");
  if (words.toLowerCase() === "legal review") return "Lawyer review";
  return words.replace(/\b\w/g, (letter) => letter.toUpperCase()) || "Evidence";
}

function financialBusinessLabel(record: JsonRecord): string {
  const displayName = text(record.display_name).trim();
  if (displayName && !/^financial (?:record|commitment)$/i.test(displayName)) return displayName;
  const id = text(record.id)
    .replace(/^financial-(?:budget|commitment|invoice|payment|refund)-/i, "")
    .replace(/-(?:[a-z]{2}-)?\d{4}-\d+$/i, "");
  const words = id.split("-").filter(Boolean).join(" ");
  return words ? words.replace(/^\w/, (letter) => letter.toUpperCase()) : (displayName || "Financial record");
}

function isInternalCommand(value: string): boolean {
  return /\b[a-z]+(?:[._-][a-z]+)+\b/i.test(value);
}

function approvalPresentation(params: {
  title: string;
  summary: string;
  commitment: FinancialRecordView;
}): Pick<ApprovalView, "title" | "actionSummary"> {
  const rawTitle = params.title.trim();
  const rawSummary = params.summary.trim();
  if (!isInternalCommand(rawTitle) && !isInternalCommand(rawSummary)) {
    return { title: rawTitle || "Approval", actionSummary: rawSummary };
  }
  const noun = params.commitment.label.trim().toLowerCase() || "commitment";
  return {
    title: `Approve ${noun}`,
    actionSummary: `Authorize the ${params.commitment.amount} ${noun}; legal submission remains blocked until approval.`,
  };
}

/** Shared V1 fixture adapted to typed UI props. No page may invent new facts. */
export const companyOsActors = {
  brandOwner: { id: "actor-human-brand-owner", name: "Brand Owner", kind: "human", role: "Business owner", unit: "Brand & IP" },
  trademarkAgent: { id: "actor-agent-trademark", name: "Trademark Agent", kind: "standing_agent", role: "Proposed trademark role", unit: "Brand & IP", organizationRoleState: "proposed" },
  financeAgent: { id: "actor-agent-finance", name: "Finance Agent", kind: "standing_agent", role: "Financial review", unit: "Finance" },
  externalLawyer: { id: "actor-external-lawyer", name: "External Lawyer", kind: "external", role: "Matter-specific legal support", unit: "Brand & IP" },
  documentArchitecture: {
    id: "actor-agent-document-architecture",
    name: "Document Architecture Agent",
    kind: "standing_agent",
    role: "Document architecture",
    unit: "Governance",
    availability: "available",
    membershipRole: "member",
    responsibilitySummary: "Maintains company knowledge structure and routes durable results back into Docs.",
    systemPromptRef: "document-agent-prompt-docs-governance",
    toolRefs: ["tool-docs-write", "tool-record-query"],
    skillRefs: ["skill-document-governance"],
    maintainedDocumentRefs: ["document-company-operating-manual", "document-trademark-application-cn-2026-018"],
    acceptedWorkTypeRefs: ["work-type-document-governance"],
    permissionPolicyRefs: ["policy-docs-governance"],
    escalationPolicyRef: "policy-governance-escalation",
  },
  ipLead: { id: "actor-agent-ip-lead", name: "IP Lead Agent", kind: "standing_agent", role: "IP lead", unit: "Brand & IP" },
  organizationGovernance: { id: "actor-agent-organization-governance", name: "Organization Governance Agent", kind: "standing_agent", role: "Organization governance", unit: "Governance" },
  contentStrategy: { id: "actor-agent-content-strategy", name: "Content Strategy Agent", kind: "standing_agent", role: "Strategy partner", unit: "Content Operations" },
  analytics: { id: "actor-agent-analytics", name: "Analytics Agent", kind: "standing_agent", role: "Analytics", unit: "Content Operations" },
} as const satisfies Record<string, ActorSummary>;

export const trademarkSource = {
  id: "document-trademark-application-cn-2026-018",
  label: "Trademark application CN-2026-018",
  detail: "Brand & IP",
} as const;

export const trademarkWorkItem: WorkItemView = {
  id: "workitem-trademark-filing-brand-a",
  title: "Trademark filing for Brand A",
  status: "waiting_for_approval",
  sourceDocument: trademarkSource,
  requestedBy: companyOsActors.brandOwner,
  submittedBy: companyOsActors.trademarkAgent,
  accountableOwner: companyOsActors.brandOwner,
  assignees: [companyOsActors.trademarkAgent],
  contributors: [companyOsActors.externalLawyer],
  reviewer: companyOsActors.financeAgent,
  legalReviewer: companyOsActors.externalLawyer,
  approver: companyOsActors.brandOwner,
  updatedAt: "20 Jul 2026 · 09:10",
};

export const documentArchitectureWorkItem: WorkItemView = {
  id: "workitem-organize-trademark-knowledge",
  title: "Organize trademark filing knowledge",
  status: "in_progress",
  sourceDocument: trademarkSource,
  requestedBy: companyOsActors.ipLead,
  submittedBy: companyOsActors.documentArchitecture,
  accountableOwner: companyOsActors.ipLead,
  assignees: [companyOsActors.documentArchitecture],
  contributors: [],
  updatedAt: "2026-07-20T09:21:00+08:00",
};

export const trademarkAssignment: AssignmentView = {
  id: "assignment-trademark-agent",
  workItemId: trademarkWorkItem.id,
  recipient: companyOsActors.trademarkAgent,
  sender: companyOsActors.ipLead,
  assignedRole: "Filing owner",
  scope: "Prepare the CN trademark filing package and return durable evidence.",
  deliveryState: "delivered",
  correlationId: "corr-trademark-018",
  deliveryEvidenceRef: "evidence-assignment-delivered",
  assignedAt: "2026-07-20T09:05:00+08:00",
};

export const documentArchitectureAssignment: AssignmentView = {
  id: "assignment-document-architecture",
  workItemId: documentArchitectureWorkItem.id,
  recipient: companyOsActors.documentArchitecture,
  sender: companyOsActors.ipLead,
  assignedRole: "Knowledge architecture owner",
  scope: "Organize trademark filing guidance and return a durable structure proposal to Docs.",
  deliveryState: "delivered",
  correlationId: "corr-document-architecture",
  deliveryEvidenceRef: "evidence-document-assignment-delivered",
  assignedAt: "2026-07-20T09:02:00+08:00",
};

export const trademarkCommitment: FinancialRecordView = {
  id: "financial-commitment-trademark-filing-fee-cn-2026-018",
  label: "Trademark filing fee",
  type: "commitment",
  amount: "¥3,000",
  status: "pending_approval",
  sourceDocument: trademarkSource,
  costContext: { id: "brand-brand-a", label: "Brand A" },
  accountableOwner: companyOsActors.brandOwner,
};

export const trademarkApproval: ApprovalView = {
  id: "approval-trademark-filing-fee-cn-2026-018",
  title: "Approve trademark filing fee",
  actionSummary: "Authorize a ¥3,000 commitment and legal submission for Trademark application CN-2026-018.",
  status: "requested",
  requestedBy: companyOsActors.trademarkAgent,
  requiredApprover: companyOsActors.brandOwner,
  financeReviewer: companyOsActors.financeAgent,
  legalReviewer: companyOsActors.externalLawyer,
  expiresAt: "31 Jul 2026 · 18:00",
};

export const prototypeTrademarkOperationsProjection: TrademarkOperationsProjection = {
  fixtureId: "company-os-trademark-v1",
  actors: companyOsActors,
  actorList: Object.values(companyOsActors),
  organization: {
    company: { id: "org-company", label: "Company" },
    brandUnit: { id: "org-brand-ip", label: "Brand & IP" },
    units: [
      { id: "org-company", label: "Company", actorIds: [] },
      { id: "org-brand-ip", label: "Brand & IP", parentId: "org-company", humanLeadActorId: "actor-human-brand-owner", agentLeadActorId: "actor-agent-ip-lead", actorIds: ["actor-human-brand-owner", "actor-agent-ip-lead", "actor-agent-trademark", "actor-external-lawyer"] },
      { id: "org-content-operations", label: "Content Operations", parentId: "org-company", actorIds: ["actor-agent-content-strategy", "actor-agent-analytics"] },
      { id: "org-finance", label: "Finance", parentId: "org-company", actorIds: ["actor-agent-finance"] },
      { id: "org-governance", label: "Governance", parentId: "org-company", actorIds: ["actor-agent-document-architecture", "actor-agent-organization-governance"] },
    ],
  },
  sourceDocument: trademarkSource,
  contentPlanDocument: { id: "document-brand-a-content-operating-plan", label: "Brand A · Content operating plan", detail: "Content Operations" },
  typedApplication: { id: "trademark-application-cn-2026-018", label: "Trademark application CN-2026-018", detail: "Typed application record · filing preparation" },
  workItem: trademarkWorkItem,
  workItems: [trademarkWorkItem, documentArchitectureWorkItem],
  assignments: [trademarkAssignment, documentArchitectureAssignment],
  commitment: trademarkCommitment,
  approval: trademarkApproval,
  evidence: [
    { id: "evidence-trademark-filing-package-cn-2026-018", label: "Trademark filing package", detail: "Submitted by Trademark Agent" },
    { id: "evidence-legal-review-cn-2026-018", label: "Lawyer review", detail: "Submitted by External Lawyer" },
  ],
  governanceProposal: { id: "governance-proposal-trademark-management", label: "Create Trademark Management module", detail: "Awaiting final approval", proposedById: "actor-agent-document-architecture" },
  businessModule: { id: "module-trademark-management", label: "Trademark Management", detail: "Proposed module" },
  julySpendMetric: { id: "metric-july-spend", label: "July spend" },
  julySpendAmount: "¥18,400",
};

/**
 * Adapts a resolved Company OS read projection into operations presentation
 * data. It preserves the input's ids, labels and responsibility relations; it
 * never adds a payment or derives ownership from execution telemetry.
 */
export function adaptTrademarkOperationsProjection(projection: unknown, options: { workItemId?: string } = {}): TrademarkOperationsProjection {
  const root = projection && typeof projection === "object" ? projection as JsonRecord : {};
  const actorRecords = records(root.actors);

  const memberships = records((root.organization as JsonRecord | undefined)?.memberships);
  const statuses = records((root.organization as JsonRecord | undefined)?.explicitly_reported_statuses);
  const units = records((root.organization as JsonRecord | undefined)?.org_units);
  const actorById: Record<string, ActorSummary> = {};
  for (const actor of actorRecords) {
    const id = text(actor.id);
    const membership = memberships.find((item) => (text(item.actor_id) || refId(item.actor_ref)) === id);
    const unit = find(units, text(membership?.org_unit_id));
    const reported = statuses.find((item) => text(item.subject_ref) === id && text(item.kind) === "availability");
    const roleState = statuses.find((item) => text(item.subject_ref) === id && text(item.kind) === "organization_role_state");
    actorById[id] = {
      id,
      name: text(actor.display_name, id || "Unresolved actor"),
      kind: actorKind(actor.actor_type),
      role: text(membership?.role_label, text(membership?.title_or_function, text(actor.role, "Organization participant"))),
      unit: text(unit?.name) || undefined,
      availability: (text(reported?.value) || (text(actor.availability) !== "unknown" ? text(actor.availability) : "")) as ActorAvailability || undefined,
      organizationRoleState: text(roleState?.value) === "proposed" ? "proposed" : undefined,
      membershipRole: text(membership?.membership_role) as ActorSummary["membershipRole"] || undefined,
      responsibilitySummary: text(actor.responsibility_summary) || undefined,
      systemPromptRef: text(actor.system_prompt_ref) || undefined,
      toolRefs: stringArray(actor.tool_refs),
      skillRefs: stringArray(actor.skill_refs),
      maintainedDocumentRefs: stringArray(actor.maintained_document_refs),
      acceptedWorkTypeRefs: stringArray(actor.accepted_work_type_refs),
      permissionPolicyRefs: stringArray(actor.permission_policy_refs),
      escalationPolicyRef: text(actor.escalation_policy_ref) || undefined,
    };
  }
  const actor = (id: unknown): ActorSummary => actorById[refId(id)] ?? {
    id: refId(id) || "unresolved-actor", name: "Unresolved actor", kind: "service", role: "Unresolved role",
  };

  const documents = records(root.documents);
  const typedRecords = records(root.typed_records);
  const workRecords = records(root.work_items);
  const assignmentRecords = records(root.assignments);
  const financeRecords = records(root.financial_records);
  const approvalRecords = records(root.approvals);
  const pageDefinitions = records(root.custom_page_definitions);
  const evidenceRecords = records(root.evidence);
  const proposalRecords = [
    ...records(root.governance_proposals),
    ...typedRecords.filter((item) => text(item.record_type).toLowerCase() === "governance_proposal"),
  ];
  const moduleRecords = records(root.business_modules);
  const metrics = [
    ...records(root.explicit_metrics),
    ...typedRecords.filter((item) => text(item.record_type).toLowerCase() === "metric_observation"),
  ];
  const workRecord = pick(workRecords, options.workItemId ?? "workitem-trademark-filing-brand-a");
  const sourceDocument = pick(documents, text(workRecord.source_document_ref, "document-trademark-application-cn-2026-018"));
  const contentPlan = pick(documents, "document-brand-a-content-operating-plan");
  const application = pick(typedRecords, "trademark-application-cn-2026-018");
  const commitmentRecord = financeRecords.find((item) => text(item.type) === "commitment") ?? financeRecords[0] ?? {};
  const approvalRecord = pick(approvalRecords, text((Array.isArray(workRecord.approval_refs) ? workRecord.approval_refs[0] : undefined), "approval-trademark-filing-fee-cn-2026-018"));
  const approvalDefinition = pageDefinitions.find((definition) => Array.isArray(definition.action_command_refs)
    && definition.action_command_refs.includes("approval.decide"));
  const workTransitionDefinition = pageDefinitions.find((definition) => Array.isArray(definition.action_command_refs)
    && definition.action_command_refs.includes("work_item.transition"));
  const proposalRecord = pick(proposalRecords, "governance-proposal-trademark-management");
  const moduleRecord = pick(moduleRecords, "module-trademark-management");
  const metricRecord = pick(metrics, "metric-july-spend");
  const source = asRef(sourceDocument.id, sourceDocument.title, sourceDocument.space ?? sourceDocument.space_id);
  const evidenceIds = Array.isArray(workRecord.evidence_refs) ? workRecord.evidence_refs : [];
  const evidence = evidenceIds.map((id) => {
    const record = find(evidenceRecords, text(id));
    return asRef(record?.id ?? id, humanizeEvidenceLabel(record?.title ?? id), record ? `Submitted by ${actor(record.submitted_by_ref).name}` : undefined);
  });

  const workItem: WorkItemView = {
    id: text(workRecord.id, "unresolved-work-item"),
    title: text(workRecord.title, "Unresolved work"),
    status: workStatus(workRecord.status),
    sourceDocument: source,
    requestedBy: actor(workRecord.requested_by_ref ?? workRecord.requested_by),
    submittedBy: actor(workRecord.submitted_by_ref ?? workRecord.submitted_by),
    accountableOwner: actor(workRecord.accountable_owner_ref ?? workRecord.accountable_owner),
    assignees: Array.isArray(workRecord.assignee_refs) ? workRecord.assignee_refs.map(actor) : Array.isArray(workRecord.assignees) ? workRecord.assignees.map(actor) : [],
    contributors: Array.isArray(workRecord.contributor_refs) ? workRecord.contributor_refs.map(actor) : Array.isArray(workRecord.contributors) ? workRecord.contributors.map(actor) : [],
    reviewer: workRecord.reviewer_ref || workRecord.reviewer ? actor(workRecord.reviewer_ref ?? workRecord.reviewer) : undefined,
    legalReviewer: workRecord.legal_reviewer_ref
      ? actor(workRecord.legal_reviewer_ref)
      : (Array.isArray(workRecord.contributors) ? workRecord.contributors.map(actor).find((entry) => entry.kind === "external") : undefined),
    approver: workRecord.approver_ref || workRecord.approver ? actor(workRecord.approver_ref ?? workRecord.approver) : undefined,
    outcomeSummary: text(workRecord.outcome_summary) || undefined,
    updatedAt: text(workRecord.updated_at),
  };
  const workItems = workRecords.map((record) => {
    if (text(record.id) === workItem.id) return workItem;
    const recordSource = find(documents, text(record.source_document_ref));
    return {
      id: text(record.id, "unresolved-work-item"),
      title: text(record.title, "Unresolved work"),
      status: workStatus(record.status),
      sourceDocument: asRef(recordSource?.id ?? record.source_document_ref, recordSource?.title ?? record.source_document_ref, recordSource?.space ?? recordSource?.space_id),
      requestedBy: actor(record.requested_by_ref ?? record.requested_by),
      submittedBy: actor(record.submitted_by_ref ?? record.submitted_by),
      accountableOwner: actor(record.accountable_owner_ref ?? record.accountable_owner),
      assignees: Array.isArray(record.assignee_refs) ? record.assignee_refs.map(actor) : Array.isArray(record.assignees) ? record.assignees.map(actor) : [],
      contributors: Array.isArray(record.contributor_refs) ? record.contributor_refs.map(actor) : Array.isArray(record.contributors) ? record.contributors.map(actor) : [],
      reviewer: record.reviewer_ref || record.reviewer ? actor(record.reviewer_ref ?? record.reviewer) : undefined,
      approver: record.approver_ref || record.approver ? actor(record.approver_ref ?? record.approver) : undefined,
      outcomeSummary: text(record.outcome_summary) || undefined,
      updatedAt: text(record.updated_at),
    } satisfies WorkItemView;
  });
  const assignments: AssignmentView[] = assignmentRecords.map((record) => ({
    id: text(record.id, "unresolved-assignment"),
    workItemId: text(record.work_item_id),
    recipient: actor(record.recipient),
    sender: actor(record.sender),
    assignedRole: text(record.assigned_role, "Assigned contributor"),
    scope: text(record.scope, "No assignment scope recorded"),
    deliveryState: text(record.delivery_state, "pending") as AssignmentView["deliveryState"],
    correlationId: text(record.correlation_id),
    deliveryEvidenceRef: text(record.delivery_evidence_ref) || undefined,
    assignedAt: text(record.assigned_at),
  }));
  const workAccountableOwner = canonicalActorRef(workRecord.accountable_owner);
  const workAssignees = Array.isArray(workRecord.assignees)
    ? workRecord.assignees.map(canonicalActorRef).filter((value): value is CanonicalActorRef => Boolean(value))
    : [];
  const workReviewer = canonicalActorRef(workRecord.reviewer);
  const workDefinitionId = text(workTransitionDefinition?.id);
  const workActionPolicyRef = Array.isArray(workTransitionDefinition?.policy_refs)
    ? workTransitionDefinition.policy_refs.map((value) => text(value)).find((value) => value.endsWith(":work_item.transition"))
    : undefined;
  if (workAccountableOwner && workAssignees.length > 0 && workDefinitionId && workActionPolicyRef) {
    workItem.transitionContext = {
      definitionId: workDefinitionId,
      actionPolicyRef: workActionPolicyRef,
      record: { ...workRecord },
      accountableOwner: workAccountableOwner,
      assignees: workAssignees,
      reviewer: workReviewer,
    };
  }
  const commitment: FinancialRecordView = {
    id: text(commitmentRecord.id, "unresolved-financial-record"),
    label: financialBusinessLabel(commitmentRecord),
    type: financialType(commitmentRecord.type),
    amount: text(commitmentRecord.display_amount, "—"),
    status: financialStatus(commitmentRecord.status),
    sourceDocument: source,
    costContext: commitmentRecord.cost_context_ref ?? commitmentRecord.milestone_ref ?? commitmentRecord.project_ref
      ? asRef(
        text(commitmentRecord.cost_context_ref ?? commitmentRecord.milestone_ref ?? commitmentRecord.project_ref),
        find(typedRecords, text(commitmentRecord.cost_context_ref ?? commitmentRecord.milestone_ref ?? commitmentRecord.project_ref))?.display_name
          ?? find(typedRecords, text(commitmentRecord.cost_context_ref ?? commitmentRecord.milestone_ref ?? commitmentRecord.project_ref))?.title
          ?? text(commitmentRecord.cost_context_ref ?? commitmentRecord.milestone_ref ?? commitmentRecord.project_ref),
      )
      : text(field(application, "brand"))
        ? asRef(text(application.id), text(field(application, "brand")), "Business context from the linked application")
        : undefined,
    accountableOwner: actor(commitmentRecord.accountable_owner_ref ?? commitmentRecord.accountable_owner),
  };
  const rawApprovalTitle = text(approvalRecord.title, text(approvalRecord.action_summary, "Approval"));
  const rawApprovalSummary = text(approvalRecord.action_summary);
  const approvalCopy = approvalPresentation({ title: rawApprovalTitle, summary: rawApprovalSummary, commitment });
  const approval: ApprovalView = {
    id: text(approvalRecord.id, "unresolved-approval"),
    title: approvalCopy.title,
    actionSummary: approvalCopy.actionSummary,
    status: approvalStatus(approvalRecord.status),
    requestedBy: actor(approvalRecord.requested_by_ref ?? approvalRecord.requested_by),
    requiredApprover: actor(Array.isArray(approvalRecord.required_approver_refs) ? approvalRecord.required_approver_refs[0] : undefined),
    financeReviewer: approvalRecord.finance_reviewer_ref
      ? actor(approvalRecord.finance_reviewer_ref)
      : workRecord.reviewer_ref || workRecord.reviewer ? actor(workRecord.reviewer_ref ?? workRecord.reviewer) : undefined,
    legalReviewer: approvalRecord.legal_reviewer_ref
      ? actor(approvalRecord.legal_reviewer_ref)
      : (Array.isArray(workRecord.contributors) ? workRecord.contributors.map(actor).find((entry) => entry.kind === "external") : undefined),
    expiresAt: text(approvalRecord.expires_at) || undefined,
  };
  const subjectRef = canonicalEntityRef(approvalRecord.subject_ref);
  const requestedByRef = canonicalActorRef(approvalRecord.requested_by);
  const requiredApproverRefs = Array.isArray(approvalRecord.required_approver_refs)
    ? approvalRecord.required_approver_refs.map(canonicalActorRef).filter((value): value is CanonicalActorRef => Boolean(value))
    : [];
  const definitionId = text(approvalDefinition?.id);
  const actionPolicyRef = Array.isArray(approvalDefinition?.policy_refs)
    ? approvalDefinition.policy_refs.map((value) => text(value)).find((value) => value.endsWith(":approval.decide"))
    : undefined;
  if (subjectRef && requestedByRef && requiredApproverRefs.length > 0 && definitionId && actionPolicyRef) {
    approval.decisionContext = {
      definitionId,
      actionPolicyRef,
      recordSubjectRef: subjectRef,
      requestedBy: requestedByRef,
      requiredApproverRefs,
      requiredActorType: text(approvalRecord.required_actor_type) || undefined,
      recordPolicyRef: text(approvalRecord.policy_ref),
      rawActionSummary: text(approvalRecord.action_summary),
      evidenceRefs: Array.isArray(approvalRecord.evidence_refs) ? approvalRecord.evidence_refs.map((value) => text(value)).filter(Boolean) : [],
      requestedAt: text(approvalRecord.requested_at),
      expiresAt: text(approvalRecord.expires_at) || undefined,
    };
  }

  const organizationUnits = units.map((unit) => {
    const unitMemberships = memberships.filter((membership) => text(field(membership, "org_unit_id")) === text(unit.id));
    const actorIds = unitMemberships
      .map((membership) => text(field(membership, "actor_id")) || refId(field(membership, "actor_ref")))
      .filter(Boolean);
    const membershipAgentLead = unitMemberships
      .map((membership) => ({
        actorId: text(field(membership, "actor_id")) || refId(field(membership, "actor_ref")),
        role: text(field(membership, "membership_role")),
      }))
      .find((membership) => membership.role === "lead" && actorById[membership.actorId]?.kind === "standing_agent")?.actorId;
    const legacyRoleAgentLead = actorIds.find((actorId) => actorById[actorId]?.kind === "standing_agent" && /\blead\b/i.test(actorById[actorId]?.role ?? ""));
    return {
      id: text(unit.id),
      label: text(field(unit, "name"), "Unresolved unit"),
      parentId: text(field(unit, "parent_unit_id"), text(field(unit, "parent_id"))) || undefined,
      humanLeadActorId: refId(field(unit, "human_lead_actor_ref")) || undefined,
      agentLeadActorId: refId(field(unit, "agent_lead_actor_ref")) || membershipAgentLead || legacyRoleAgentLead,
      actorIds,
    };
  });
  const companyUnit = pick(units, "org-company");
  const brandUnit = pick(units, "org-brand-ip");

  return {
    fixtureId: text(root.fixture_id) || undefined,
    actors: actorById,
    actorList: Object.values(actorById),
    organization: {
      company: asRef(companyUnit.id, field(companyUnit, "name")),
      brandUnit: asRef(brandUnit.id, field(brandUnit, "name")),
      units: organizationUnits,
    },
    sourceDocument: source,
    contentPlanDocument: asRef(contentPlan.id, contentPlan.title, contentPlan.space ?? contentPlan.space_id),
    typedApplication: asRef(application.id, field(application, "display_id") ? `Trademark application ${text(field(application, "display_id"))}` : application.display_name ?? application.title, "Typed application record · filing preparation"),
    workItem,
    workItems,
    assignments,
    commitment,
    approval,
    evidence,
    governanceProposal: {
      ...asRef(proposalRecord.id, field(proposalRecord, "title") ?? proposalRecord.title, text(field(proposalRecord, "status") ?? proposalRecord.lifecycle_status).replace(/_/g, " ") || undefined),
      proposedById: refId(field(proposalRecord, "proposed_by_ref")) || undefined,
    },
    businessModule: asRef(moduleRecord.id, moduleRecord.name, text(moduleRecord.status).replace(/_/g, " ") || undefined),
    julySpendMetric: asRef(metricRecord.id, field(metricRecord, "label")),
    julySpendAmount: text(field(metricRecord, "display_amount"), "—"),
  };
}
