import type {
  ActorAvailability,
  ActorKind,
  ActorSummary,
  StandingExecutionAssignment,
  AssignmentView,
  CanonicalActorRef,
  CanonicalEntityRef,
  ApprovalView,
  FinancialRecordView,
  OrganizationIntegrityFinding,
  OrganizationMembershipView,
  RelatedLink,
  StandingLinkConflict,
  TrademarkOperationsProjection,
  WorkAggregateView,
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

function numeric(value: unknown, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function object(value: unknown): JsonRecord {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as JsonRecord
    : {};
}

function stringListMap(value: unknown): Record<string, string[]> {
  return Object.fromEntries(
    Object.entries(object(value)).map(([key, entries]) => [key, stringArray(entries)]),
  );
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.map((item) => text(item)).filter(Boolean) : [];
}

function distinct<T>(values: T[]): T[] {
  return [...new Set(values)];
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

function relatedEntityLinks(value: unknown): RelatedLink[] {
  return Array.isArray(value)
    ? value
        .flatMap((entry) => {
          const ref = canonicalEntityRef(entry);
          return ref ? [{ id: ref.id, label: ref.id, detail: ref.kind }] : [];
        })
    : [];
}

function entityRefs(value: unknown): CanonicalEntityRef[] {
  return Array.isArray(value)
    ? value.map(canonicalEntityRef).filter((entry): entry is CanonicalEntityRef => Boolean(entry))
    : [];
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

function membershipRole(value: unknown): OrganizationMembershipView["membershipRole"] {
  switch (text(value)) {
    case "lead": case "member": case "advisor": case "observer": case "external_partner":
      return text(value) as OrganizationMembershipView["membershipRole"];
    default:
      return undefined;
  }
}

function workStatus(value: unknown): WorkItemView["status"] {
  switch (text(value)) {
    case "draft": case "submitted": case "triaged": case "accepted":
    case "waiting_for_approval": case "in_progress": case "in_review":
    case "completed": case "blocked": case "cancelled": case "archived":
      return text(value) as WorkItemView["status"];
    default: return "draft";
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
    actionSummary: `Authorize the ${params.commitment.amount} ${noun}; the requested governed effect remains blocked until approval.`,
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
  objective: "Prepare the CN trademark filing package and stop for Human approval before legal or financial effect.",
  description: "Collect source materials, legal review, filing evidence, and finance context into one governed WorkItem.",
  acceptanceCriteria: ["Filing package reviewed", "Human approval recorded", "Filing receipt or blocker evidence linked"],
  contextRefs: [trademarkSource],
  deliverableRefs: [{ id: "evidence-filing-package", label: "Filing package evidence", detail: "evidence" }],
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
  objective: "Keep trademark source documents, records, and views navigable for agents and human reviewers.",
  acceptanceCriteria: ["Source document linked", "Structured record relation visible"],
  contextRefs: [trademarkSource],
  deliverableRefs: [],
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
    units: [
      { id: "org-company", label: "Company", actorIds: [], policyRefs: [] },
      { id: "org-brand-ip", label: "Brand & IP", parentId: "org-company", humanLeadActorId: "actor-human-brand-owner", agentLeadActorId: "actor-agent-ip-lead", actorIds: ["actor-human-brand-owner", "actor-agent-ip-lead", "actor-agent-trademark", "actor-external-lawyer"], policyRefs: [] },
      { id: "org-content-operations", label: "Content Operations", parentId: "org-company", actorIds: ["actor-agent-content-strategy", "actor-agent-analytics"], policyRefs: [] },
      { id: "org-finance", label: "Finance", parentId: "org-company", actorIds: ["actor-agent-finance"], policyRefs: [] },
      { id: "org-governance", label: "Governance", parentId: "org-company", actorIds: ["actor-agent-document-architecture", "actor-agent-organization-governance"], policyRefs: [] },
    ],
    memberships: [
      { id: "membership-brand-owner", orgUnitId: "org-brand-ip", actorId: "actor-human-brand-owner", membershipRole: "lead", authorityPolicyRefs: [] },
      { id: "membership-ip-lead", orgUnitId: "org-brand-ip", actorId: "actor-agent-ip-lead", membershipRole: "lead", authorityPolicyRefs: [] },
      { id: "membership-trademark", orgUnitId: "org-brand-ip", actorId: "actor-agent-trademark", membershipRole: "member", authorityPolicyRefs: [] },
      { id: "membership-lawyer", orgUnitId: "org-brand-ip", actorId: "actor-external-lawyer", membershipRole: "external_partner", authorityPolicyRefs: [] },
      { id: "membership-content-strategy", orgUnitId: "org-content-operations", actorId: "actor-agent-content-strategy", membershipRole: "member", authorityPolicyRefs: [] },
      { id: "membership-analytics", orgUnitId: "org-content-operations", actorId: "actor-agent-analytics", membershipRole: "member", authorityPolicyRefs: [] },
      { id: "membership-finance", orgUnitId: "org-finance", actorId: "actor-agent-finance", membershipRole: "member", authorityPolicyRefs: [] },
      { id: "membership-docs", orgUnitId: "org-governance", actorId: "actor-agent-document-architecture", membershipRole: "member", authorityPolicyRefs: [] },
      { id: "membership-org-governance", orgUnitId: "org-governance", actorId: "actor-agent-organization-governance", membershipRole: "member", authorityPolicyRefs: [] },
    ],
    rootUnitIds: ["org-company"],
    unplacedUnitIds: [],
    unassignedActorIds: [],
    integrityFindings: [],
  },
  sourceDocument: trademarkSource,
  contentPlanDocument: { id: "document-brand-a-content-operating-plan", label: "Brand A · Content operating plan", detail: "Content Operations" },
  typedApplication: { id: "trademark-application-cn-2026-018", label: "Trademark application CN-2026-018", detail: "Typed application record · filing preparation" },
  workItem: trademarkWorkItem,
  workItems: [trademarkWorkItem, documentArchitectureWorkItem],
  work: {
    provenance: "legacy_raw_records",
    summary: { total: 2, active: 2, completed: 0, blocked: 0, waitingForApproval: 1, unassigned: 0, withoutMilestone: 2, withoutBusinessLine: 2 },
    board: { waiting_for_approval: [trademarkWorkItem.id], in_progress: [documentArchitectureWorkItem.id] },
    businessLines: { unclassified: [trademarkWorkItem.id, documentArchitectureWorkItem.id] },
    workTypes: { general: [trademarkWorkItem.id, documentArchitectureWorkItem.id] },
    milestones: [],
    workload: [],
    selection: { requestedId: trademarkWorkItem.id, status: "resolved" },
  },
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
      executionAgentMemberRef: text(actor.execution_agent_member_ref) || undefined,
    };
  }
  const actor = (id: unknown): ActorSummary => actorById[refId(id)] ?? {
    id: refId(id) || "unresolved-actor", name: "Unresolved actor", kind: "service", role: "Unresolved role",
  };
  const optionalActor = (id: unknown): ActorSummary | undefined => {
    const resolved = refId(id);
    return resolved ? actor(resolved) : undefined;
  };

  const documents = records(root.documents);
  const typedRecords = records(root.typed_records);
  const hasWorkAggregate = Object.prototype.hasOwnProperty.call(root, "work");
  const workAggregateRecord = object(root.work);
  const workRecords = hasWorkAggregate
    ? records(workAggregateRecord.work_items)
    : records(root.work_items);
  const assignmentRecords = records(root.assignments);
  const standingAssignmentRecords = records(root.standing_assignments);
  const executionChainRecords = records(root.work_assignment_execution_chains);
  const financeRecords = records(root.financial_records);
  const approvalRecords = records(root.approvals);
  const pageDefinitions = records(root.custom_page_definitions);
  const evidenceRecords = records(root.evidence);
  const proposalRecords = [
    ...records(root.governance_proposals),
    ...typedRecords.filter((item) => text(item.record_type).toLowerCase() === "governance_proposal"),
  ];
  const moduleRecords = records(root.business_modules);
  const standingAssignments: StandingExecutionAssignment[] = standingAssignmentRecords.map((record): StandingExecutionAssignment => ({
    id: text(record.id),
    agentMemberId: text(record.agent_member_id),
    sourceKind: text(record.source_kind) === "agent_team_participation"
      ? "agent_team_participation"
      : "agent_team_assignment",
    sourceRef: text(record.source_ref) || undefined,
    missionId: text(record.mission_id) || undefined,
    waveId: text(record.wave_id) || undefined,
    teamRunId: text(record.team_run_id),
    memberRunId: text(record.member_run_id),
    title: text(record.title, "Agent Team assignment"),
    role: text(record.role, "member"),
    status: text(record.status, "unknown"),
    assignedAt: text(record.assigned_at),
    lastActivityAt: text(record.last_activity_at) || undefined,
    correlationId: text(record.correlation_id) || undefined,
    nativeSession: record.native_session && typeof record.native_session === "object"
      ? record.native_session as StandingExecutionAssignment["nativeSession"]
      : undefined,
  })).filter((assignment) => assignment.agentMemberId && assignment.teamRunId && assignment.memberRunId);
  const standingAssignmentConflictRecords = records(root.standing_assignment_conflicts);
  const standingAssignmentConflicts: StandingLinkConflict[] = standingAssignmentConflictRecords.map((record): StandingLinkConflict => ({
    id: text(record.id),
    kind: text(record.kind),
    severity: text(record.severity, "error"),
    agentMemberId: text(record.agent_member_id),
    standingAgentIds: stringArray(record.standing_agent_ids),
    affectedMemberRunIds: stringArray(record.affected_member_run_ids),
    detail: text(record.detail),
    resolutionHint: text(record.resolution_hint) || undefined,
  })).filter((conflict) => conflict.id && conflict.agentMemberId && conflict.standingAgentIds.length > 0);
  const workAssignmentExecutionChains = executionChainRecords.map((record) => ({
      assignmentId: text(record.assignment_id),
      workItemId: text(record.work_item_id),
      assignmentState: text(record.assignment_state, "unknown"),
      correlationId: text(record.correlation_id),
      linkStatus: text(record.link_status, "unavailable") as "linked" | "mismatch" | "unavailable",
      detail: text(record.detail, "Execution evidence is unavailable."),
      teamMessage: record.team_message && typeof record.team_message === "object" ? {
        id: text((record.team_message as Record<string, unknown>).id),
        deliveryState: text((record.team_message as Record<string, unknown>).delivery_state, "unavailable"),
        providerReceiptId: text((record.team_message as Record<string, unknown>).provider_receipt_id) || undefined,
      } : undefined,
      memberRun: record.member_run && typeof record.member_run === "object" ? {
        id: text((record.member_run as Record<string, unknown>).id),
        status: text((record.member_run as Record<string, unknown>).status, "unknown"),
        nativeSessionId: text((record.member_run as Record<string, unknown>).native_session_id) || undefined,
        nativeSessionAvailability: text((record.member_run as Record<string, unknown>).native_session_availability, "unavailable"),
      } : undefined,
      handoffs: records(record.handoffs).map((handoff) => ({
        id: text(handoff.id), result: text(handoff.result) || undefined,
        body: text(handoff.body), createdAt: text(handoff.created_at),
        evidenceRefs: stringArray(handoff.evidence_refs),
      })),
      externalObservations: records(record.external_observations).map((observation) => ({
        id: text(observation.id),
        kind: text(observation.kind) === "check" ? "check" as const : "pull_request" as const,
        label: text(observation.label, text(observation.id)),
        repository: text(observation.repository) || undefined,
        pullRequestNumber: text(observation.pull_request_number) || undefined,
        headRef: text(observation.head_ref) || undefined,
        url: text(observation.url) || undefined,
        headSha: text(observation.head_sha) || undefined,
        baseRef: text(observation.base_ref) || undefined,
        state: text(observation.state) || undefined,
        observedAt: text(observation.observed_at) || undefined,
        sourceUpdatedAt: text(observation.source_updated_at) || undefined,
        sourceCompletedAt: text(observation.source_completed_at) || undefined,
        freshness: text(observation.freshness, "unavailable") as "fresh" | "stale" | "unavailable",
      })),
    }));
  const metrics = [
    ...records(root.explicit_metrics),
    ...typedRecords.filter((item) => text(item.record_type).toLowerCase() === "metric_observation"),
  ];
  const requestedWorkItemId = options.workItemId
    ?? (text(root.fixture_id) ? "workitem-trademark-filing-brand-a" : undefined);
  const workRecord = requestedWorkItemId ? find(workRecords, requestedWorkItemId) ?? {} : {};
  const sourceDocument = find(documents, text(workRecord.source_document_ref)) ?? {};
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
    id: text(workRecord.id, requestedWorkItemId ?? "unresolved-work-item"),
    title: text(workRecord.title, requestedWorkItemId ? "WorkItem not found" : "No WorkItem selected"),
    objective: text(workRecord.objective) || undefined,
    description: text(workRecord.description) || undefined,
    acceptanceCriteria: stringArray(workRecord.acceptance_criteria),
    contextRefs: relatedEntityLinks(workRecord.context_refs),
    deliverableRefs: relatedEntityLinks(workRecord.deliverable_refs),
    status: workStatus(workRecord.status),
    sourceDocument: source,
    requestedBy: optionalActor(workRecord.requested_by_ref ?? workRecord.requested_by),
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
      objective: text(record.objective) || undefined,
      description: text(record.description) || undefined,
      acceptanceCriteria: stringArray(record.acceptance_criteria),
      contextRefs: relatedEntityLinks(record.context_refs),
      deliverableRefs: relatedEntityLinks(record.deliverable_refs),
      status: workStatus(record.status),
      sourceDocument: asRef(recordSource?.id ?? record.source_document_ref, recordSource?.title ?? record.source_document_ref, recordSource?.space ?? recordSource?.space_id),
      requestedBy: optionalActor(record.requested_by_ref ?? record.requested_by),
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
  const aggregateSummary = object(workAggregateRecord.summary);
  const legacyActive = workItems.filter((item) => !["draft", "completed", "cancelled", "archived"].includes(item.status));
  const legacyBoard = workItems.reduce<Record<string, string[]>>((board, item) => {
    (board[item.status] ??= []).push(item.id);
    return board;
  }, {});
  const legacyWorkTypes = workRecords.reduce<Record<string, string[]>>((types, record) => {
    const type = text(record.work_type, "general");
    (types[type] ??= []).push(text(record.id));
    return types;
  }, {});
  const legacyBusinessLines = workRecords.reduce<Record<string, string[]>>((lines, record) => {
    const line = text(record.business_module_ref, "unclassified");
    (lines[line] ??= []).push(text(record.id));
    return lines;
  }, {});
  const aggregateMilestones = records(workAggregateRecord.milestones).map((entry) => {
    const milestone = object(entry.milestone);
    return {
      id: text(milestone.id, text(entry.id)),
      title: text(milestone.title, "Untitled milestone"),
      status: text(milestone.status, "planned"),
      total: numeric(entry.total_work_items),
      completed: numeric(entry.completed_work_items),
      blocked: numeric(entry.blocked_work_items),
      waitingForApproval: numeric(entry.waiting_for_approval_work_items),
      progressPercent: numeric(entry.progress_percent),
    };
  }).filter((milestone) => milestone.id);
  const work: WorkAggregateView = {
    provenance: hasWorkAggregate ? "company_os.work" : "legacy_raw_records",
    summary: {
      total: hasWorkAggregate ? numeric(aggregateSummary.total) : workItems.length,
      active: hasWorkAggregate ? numeric(aggregateSummary.active) : legacyActive.length,
      completed: hasWorkAggregate ? numeric(aggregateSummary.completed) : workItems.filter((item) => item.status === "completed").length,
      blocked: hasWorkAggregate ? numeric(aggregateSummary.blocked) : workItems.filter((item) => item.status === "blocked").length,
      waitingForApproval: hasWorkAggregate ? numeric(aggregateSummary.waiting_for_approval) : workItems.filter((item) => item.status === "waiting_for_approval").length,
      unassigned: hasWorkAggregate ? numeric(aggregateSummary.unassigned) : workItems.filter((item) => item.assignees.length === 0).length,
      withoutMilestone: hasWorkAggregate ? numeric(aggregateSummary.without_milestone) : workRecords.filter((item) => !text(item.milestone_ref)).length,
      withoutBusinessLine: hasWorkAggregate ? numeric(aggregateSummary.without_business_line) : workRecords.filter((item) => !text(item.business_module_ref)).length,
    },
    board: hasWorkAggregate ? stringListMap(workAggregateRecord.board) : legacyBoard,
    businessLines: hasWorkAggregate ? stringListMap(workAggregateRecord.business_lines) : legacyBusinessLines,
    workTypes: hasWorkAggregate ? stringListMap(workAggregateRecord.work_types) : legacyWorkTypes,
    milestones: hasWorkAggregate ? aggregateMilestones : [],
    workload: hasWorkAggregate
      ? records(workAggregateRecord.workload).map((entry) => ({
          actorId: refId(entry.actor),
          accountableCount: numeric(entry.accountable_count),
          assignedCount: numeric(entry.assigned_count),
          activeCount: numeric(entry.active_count),
          workItemIds: stringArray(entry.work_item_refs),
        })).filter((entry) => entry.actorId)
      : [],
    selection: {
      requestedId: requestedWorkItemId,
      status: workRecords.length === 0
        ? "empty"
        : !requestedWorkItemId
          ? "not_requested"
          : text(workRecord.id) === requestedWorkItemId
            ? "resolved"
            : "not_found",
    },
  };
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
  const linkedApprovalIds = stringArray(workRecord.approval_refs);
  const linkedApproval = linkedApprovalIds.includes(approval.id) ? approval : undefined;
  const linkedEntityRefs = [
    ...entityRefs(workRecord.context_refs),
    ...entityRefs(workRecord.deliverable_refs),
  ];
  const linkedTypedRecordIds = distinct([
    ...stringArray(workRecord.source_record_refs),
    ...stringArray(workRecord.result_record_refs),
    ...linkedEntityRefs.filter((entry) => entry.kind === "typed_record").map((entry) => entry.id),
  ]);
  const linkedTypedRecords = linkedTypedRecordIds
    .map((id) => find(typedRecords, id))
    .filter((entry): entry is JsonRecord => Boolean(entry))
    .map((entry) => asRef(
      entry.id,
      field(entry, "display_id") ?? entry.display_name ?? entry.title ?? entry.id,
      text(entry.record_type, "Typed record"),
    ));
  const linkedFinancialIds = distinct([
    ...linkedEntityRefs.filter((entry) => entry.kind === "financial_record").map((entry) => entry.id),
    ...(linkedApproval && subjectRef?.kind === "financial_record" ? [subjectRef.id] : []),
  ]);
  const linkedCommitment = linkedFinancialIds.includes(commitment.id) ? commitment : undefined;

  const organizationMemberships: OrganizationMembershipView[] = memberships.map((membership, index) => {
    const orgUnitId = text(field(membership, "org_unit_id"));
    const actorId = text(field(membership, "actor_id")) || refId(field(membership, "actor_ref"));
    return {
      id: text(membership.id, `membership:${orgUnitId}:${actorId}:${index}`),
      organizationId: text(field(membership, "organization_id")) || undefined,
      orgUnitId,
      actorId,
      membershipRole: membershipRole(field(membership, "membership_role")),
      titleOrFunction: text(field(membership, "title_or_function"), text(field(membership, "role_label"))) || undefined,
      status: text(field(membership, "status")) || undefined,
      startsAt: text(field(membership, "starts_at")) || undefined,
      endsAt: text(field(membership, "ends_at")) || undefined,
      authorityPolicyRefs: stringArray(field(membership, "authority_policy_refs")),
    };
  }).filter((membership) => membership.orgUnitId && membership.actorId);
  const organizationIntegrityFindings: OrganizationIntegrityFinding[] = [];
  const seenUnitIds = new Set<string>();
  const organizationUnits = units.flatMap((unit) => {
    const id = text(unit.id);
    if (!id) return [];
    if (seenUnitIds.has(id)) {
      organizationIntegrityFindings.push({
        id: `duplicate-unit:${id}`,
        kind: "duplicate_unit",
        severity: "error",
        detail: `OrgUnit ${id} appears more than once in the projection.`,
        unitIds: [id],
        actorIds: [],
      });
      return [];
    }
    seenUnitIds.add(id);
    const unitMemberships = organizationMemberships.filter((membership) => membership.orgUnitId === id);
    const liveParentId = text(field(unit, "parent_unit_id"));
    const legacyFixtureParentId = text(root.fixture_id) ? text(field(unit, "parent_id")) : "";
    return [{
      id,
      label: text(field(unit, "name"), "Unresolved unit"),
      detail: text(field(unit, "purpose")) || undefined,
      organizationId: text(field(unit, "organization_id")) || undefined,
      purpose: text(field(unit, "purpose")) || undefined,
      status: text(field(unit, "status")) || undefined,
      parentId: liveParentId || legacyFixtureParentId || undefined,
      humanLeadActorId: refId(field(unit, "human_lead_actor_ref")) || undefined,
      agentLeadActorId: refId(field(unit, "agent_lead_actor_ref")) || undefined,
      actorIds: distinct(unitMemberships.map((membership) => membership.actorId)),
      policyRefs: stringArray(field(unit, "policy_refs")),
      documentSpaceRef: text(field(unit, "document_space_ref")) || undefined,
    }];
  });
  const unitById = new Map(organizationUnits.map((unit) => [unit.id, unit]));
  if (organizationUnits.length === 0) {
    organizationIntegrityFindings.push({
      id: "empty-organization",
      kind: "empty_organization",
      severity: "warning",
      detail: "No OrgUnit records are present in Store truth.",
      unitIds: [],
      actorIds: [],
    });
  }
  for (const unit of organizationUnits) {
    if (unit.parentId && !unitById.has(unit.parentId)) {
      organizationIntegrityFindings.push({
        id: `orphan-parent:${unit.id}`,
        kind: "orphan_parent",
        severity: "error",
        detail: `OrgUnit ${unit.id} references missing parent ${unit.parentId}.`,
        unitIds: [unit.id, unit.parentId],
        actorIds: [],
      });
    }
    const humanLead = unit.humanLeadActorId ? actorById[unit.humanLeadActorId] : undefined;
    if (unit.humanLeadActorId && humanLead?.kind !== "human") {
      organizationIntegrityFindings.push({
        id: `invalid-human-lead:${unit.id}`,
        kind: "invalid_human_lead",
        severity: "error",
        detail: `OrgUnit ${unit.id} human lead does not resolve to a Human actor.`,
        unitIds: [unit.id],
        actorIds: [unit.humanLeadActorId],
      });
    }
    const agentLead = unit.agentLeadActorId ? actorById[unit.agentLeadActorId] : undefined;
    if (unit.agentLeadActorId && agentLead?.kind !== "standing_agent") {
      organizationIntegrityFindings.push({
        id: `invalid-agent-lead:${unit.id}`,
        kind: "invalid_agent_lead",
        severity: "error",
        detail: `OrgUnit ${unit.id} agent lead does not resolve to a Standing Agent actor.`,
        unitIds: [unit.id],
        actorIds: [unit.agentLeadActorId],
      });
    }
  }
  const seenMembershipPairs = new Set<string>();
  for (const membership of organizationMemberships) {
    if (!unitById.has(membership.orgUnitId)) {
      organizationIntegrityFindings.push({
        id: `unknown-membership-unit:${membership.id}`,
        kind: "unknown_membership_unit",
        severity: "error",
        detail: `Membership ${membership.id} references missing OrgUnit ${membership.orgUnitId}.`,
        unitIds: [membership.orgUnitId],
        actorIds: [membership.actorId],
      });
    }
    if (!actorById[membership.actorId]) {
      organizationIntegrityFindings.push({
        id: `unknown-membership-actor:${membership.id}`,
        kind: "unknown_membership_actor",
        severity: "error",
        detail: `Membership ${membership.id} references missing actor ${membership.actorId}.`,
        unitIds: [membership.orgUnitId],
        actorIds: [membership.actorId],
      });
    }
    const pair = `${membership.orgUnitId}:${membership.actorId}`;
    if (seenMembershipPairs.has(pair)) {
      organizationIntegrityFindings.push({
        id: `duplicate-membership:${membership.id}`,
        kind: "duplicate_membership",
        severity: "warning",
        detail: `Actor ${membership.actorId} has more than one membership in OrgUnit ${membership.orgUnitId}.`,
        unitIds: [membership.orgUnitId],
        actorIds: [membership.actorId],
      });
    }
    seenMembershipPairs.add(pair);
  }
  const rootUnitIds = organizationUnits.filter((unit) => !unit.parentId).map((unit) => unit.id);
  const reached = new Set<string>();
  const visit = (unitId: string, ancestry: string[]) => {
    if (ancestry.includes(unitId)) {
      const cycle = [...ancestry.slice(ancestry.indexOf(unitId)), unitId];
      const id = `parent-cycle:${cycle.join(":")}`;
      if (!organizationIntegrityFindings.some((finding) => finding.id === id)) {
        organizationIntegrityFindings.push({
          id,
          kind: "parent_cycle",
          severity: "error",
          detail: `OrgUnit parent cycle detected: ${cycle.join(" → ")}.`,
          unitIds: cycle,
          actorIds: [],
        });
      }
      return;
    }
    if (reached.has(unitId)) return;
    reached.add(unitId);
    for (const child of organizationUnits.filter((candidate) => candidate.parentId === unitId)) {
      visit(child.id, [...ancestry, unitId]);
    }
  };
  for (const rootId of rootUnitIds) visit(rootId, []);
  for (const unit of organizationUnits) {
    if (!reached.has(unit.id)) visit(unit.id, []);
  }
  const structurallyPlaced = new Set<string>();
  const markPlaced = (unitId: string) => {
    if (structurallyPlaced.has(unitId)) return;
    structurallyPlaced.add(unitId);
    for (const child of organizationUnits.filter((candidate) => candidate.parentId === unitId)) markPlaced(child.id);
  };
  for (const rootId of rootUnitIds) markPlaced(rootId);
  const unplacedUnitIds = organizationUnits.filter((unit) => !structurallyPlaced.has(unit.id)).map((unit) => unit.id);
  const assignedActorIds = new Set(organizationMemberships.map((membership) => membership.actorId));
  const unassignedActorIds = Object.keys(actorById).filter((actorId) => !assignedActorIds.has(actorId));
  for (const actorId of unassignedActorIds) {
    organizationIntegrityFindings.push({
      id: `unassigned-actor:${actorId}`,
      kind: "unassigned_actor",
      severity: "info",
      detail: `Actor ${actorId} has no OrganizationMembership.`,
      unitIds: [],
      actorIds: [actorId],
    });
  }

  return {
    fixtureId: text(root.fixture_id) || undefined,
    actors: actorById,
    actorList: Object.values(actorById),
    standingAssignments,
    standingAssignmentConflicts,
    organization: {
      units: organizationUnits,
      memberships: organizationMemberships,
      rootUnitIds,
      unplacedUnitIds,
      unassignedActorIds,
      integrityFindings: organizationIntegrityFindings,
    },
    sourceDocument: source,
    contentPlanDocument: asRef(contentPlan.id, contentPlan.title, contentPlan.space ?? contentPlan.space_id),
    typedApplication: asRef(application.id, field(application, "display_id") ? `Trademark application ${text(field(application, "display_id"))}` : application.display_name ?? application.title, "Typed application record · filing preparation"),
    linkedTypedRecords,
    linkedApproval,
    linkedCommitment,
    workItem,
    workItems,
    work,
    assignments,
    workAssignmentExecutionChains: workAssignmentExecutionChains.filter((chain) => chain.workItemId === workItem.id),
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
