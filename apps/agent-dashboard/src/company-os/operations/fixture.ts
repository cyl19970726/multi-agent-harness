import type {
  ActorSummary,
  ApprovalView,
  CanonicalActorRef,
  CanonicalEntityRef,
  FinancialRecordView,
  OrganizationIntegrityFinding,
  OrganizationMembershipView,
  OrganizationUnitView,
  RelatedLink,
  AgentMemberExecutionAssignment,
  TrademarkOperationsProjection,
} from "./types";

type JsonRecord = Record<string, unknown>;

function object(value: unknown): JsonRecord {
  return value && typeof value === "object" && !Array.isArray(value) ? value as JsonRecord : {};
}

function records(value: unknown): JsonRecord[] {
  return Array.isArray(value) ? value.map((entry) => {
    const row = object(entry);
    const nested = object(row.record);
    return Object.keys(nested).length ? { ...nested, ...row } : row;
  }) : [];
}

function text(value: unknown, fallback = ""): string {
  return typeof value === "string" && value.trim() ? value : fallback;
}

function strings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string" && Boolean(entry.trim())) : [];
}

function refId(value: unknown): string {
  const ref = object(value);
  return text(ref.actor_id) || text(ref.id) || text(value);
}

function related(id: unknown, label: unknown, detail?: unknown): RelatedLink {
  const resolved = text(id, "unresolved");
  return { id: resolved, label: text(label, resolved), detail: text(detail) || undefined };
}

function actorKind(value: unknown): ActorSummary["kind"] {
  switch (text(value).toLowerCase()) {
    case "human": return "human";
    case "agent":
    case "agent_membership": return "agent_membership";
    case "external": return "external";
    default: return "service";
  }
}

function actorRef(value: unknown): CanonicalActorRef | undefined {
  const ref = object(value);
  const actor_id = text(ref.actor_id);
  const actor_type = text(ref.actor_type) as CanonicalActorRef["actor_type"];
  return actor_id && ["human", "agent", "external", "service"].includes(actor_type)
    ? { actor_type, actor_id }
    : undefined;
}

function entityRef(value: unknown): CanonicalEntityRef | undefined {
  const ref = object(value);
  const kind = text(ref.kind);
  const id = text(ref.id);
  return kind && id ? { kind, id } : undefined;
}

function money(record: JsonRecord): string {
  const amount = object(record.amount);
  const value = text(amount.amount, "—");
  const currency = text(amount.currency);
  return currency === "CNY" ? `¥${value}` : currency === "USD" ? `$${value}` : [value, currency].filter(Boolean).join(" ");
}

function rootOf(source: unknown): JsonRecord {
  const root = object(source);
  const company = object(root.company_os);
  return Object.keys(company).length ? company : root;
}

export function adaptTrademarkOperationsProjection(source: unknown): TrademarkOperationsProjection {
  const root = rootOf(source);
  const actorRows = records(root.actors);
  const actors: Record<string, ActorSummary> = {};
  for (const row of actorRows) {
    const id = text(row.id);
    if (!id) continue;
    const raw = object(row.actor);
    const kind = actorKind(row.actor_type);
    actors[id] = {
      id,
      name: text(row.display_name, text(raw.display_name, text(raw.display_name_or_organization, id))),
      kind,
      role: text(raw.title, text(raw.role, "Company participant")),
      availability: text(raw.availability) as ActorSummary["availability"] || undefined,
      responsibilitySummary: text(raw.responsibility_summary) || undefined,
      systemPromptRef: text(raw.system_prompt_ref) || undefined,
      toolRefs: strings(raw.tool_refs),
      skillRefs: strings(raw.skill_refs),
      maintainedDocumentRefs: strings(raw.maintained_document_refs),
      acceptedWorkTypeRefs: strings(raw.accepted_work_type_refs ?? raw.accepted_work_types),
      permissionPolicyRefs: strings(raw.permission_policy_refs),
      escalationPolicyRef: text(raw.escalation_policy_ref) || undefined,
      executionAgentMemberRef: text(raw.agent_member_ref) || undefined,
    };
  }
  const unresolved: ActorSummary = {
    id: "unresolved-actor",
    name: "Unresolved actor",
    kind: "service",
    role: "No actor supplied",
  };
  const resolveActor = (value: unknown): ActorSummary => actors[refId(value)] ?? unresolved;

  const organization = object(root.organization);
  const memberships: OrganizationMembershipView[] = records(organization.memberships).map((row, index) => ({
    id: text(row.id, `membership-${index}`),
    organizationId: text(row.organization_id) || undefined,
    orgUnitId: text(row.org_unit_id),
    actorId: refId(row.actor_ref) || text(row.actor_id),
    membershipRole: text(row.membership_role) as OrganizationMembershipView["membershipRole"] || undefined,
    titleOrFunction: text(row.title_or_function) || undefined,
    status: text(row.status) || undefined,
    startsAt: text(row.starts_at) || undefined,
    endsAt: text(row.ends_at) || undefined,
    authorityPolicyRefs: strings(row.authority_policy_refs),
  })).filter((row) => row.orgUnitId && row.actorId);
  const units: OrganizationUnitView[] = records(organization.org_units).map((row) => ({
    id: text(row.id),
    label: text(row.name, text(row.id, "Unresolved unit")),
    detail: text(row.purpose) || undefined,
    parentId: text(row.parent_unit_id) || undefined,
    organizationId: text(row.organization_id) || undefined,
    purpose: text(row.purpose) || undefined,
    status: text(row.status) || undefined,
    humanLeadActorId: refId(row.human_lead_actor_ref) || undefined,
    agentLeadActorId: refId(row.agent_lead_actor_ref) || undefined,
    actorIds: memberships.filter((membership) => membership.orgUnitId === text(row.id)).map((membership) => membership.actorId),
    policyRefs: strings(row.policy_refs),
    documentSpaceRef: text(row.document_space_ref) || undefined,
  })).filter((unit) => unit.id);
  const unitIds = new Set(units.map((unit) => unit.id));
  const actorIds = new Set(Object.keys(actors));
  const integrityFindings: OrganizationIntegrityFinding[] = [];
  for (const membership of memberships) {
    if (!unitIds.has(membership.orgUnitId)) integrityFindings.push({
      id: `unknown-unit:${membership.id}`, kind: "unknown_membership_unit", severity: "error",
      detail: `Membership ${membership.id} references missing OrgUnit ${membership.orgUnitId}.`,
      unitIds: [membership.orgUnitId], actorIds: [membership.actorId],
    });
    if (!actorIds.has(membership.actorId)) integrityFindings.push({
      id: `unknown-actor:${membership.id}`, kind: "unknown_membership_actor", severity: "error",
      detail: `Membership ${membership.id} references missing actor ${membership.actorId}.`,
      unitIds: [membership.orgUnitId], actorIds: [membership.actorId],
    });
  }
  const assigned = new Set(memberships.map((membership) => membership.actorId));
  const unassignedActorIds = Object.keys(actors).filter((id) => !assigned.has(id));

  const documents = records(root.documents);
  const sourceDocumentRow = documents[0] ?? {};
  const contentPlanRow = documents[1] ?? sourceDocumentRow;
  const typed = records(root.typed_records)[0] ?? {};
  const aggregate = object(root.work);
  const works = records(aggregate.works).length ? records(aggregate.works) : records(root.works);
  const selectedWork = works[0];
  const linkedWork = selectedWork ? {
    id: text(selectedWork.id),
    label: text(selectedWork.title, text(selectedWork.id)),
    detail: [text(selectedWork.phase), text(selectedWork.condition) !== "normal" ? text(selectedWork.condition) : ""].filter(Boolean).join(" · "),
    phase: text(selectedWork.phase) as "open" | "active" | "review" | "closed",
    condition: text(selectedWork.condition) as "normal" | "blocked" | "on_hold",
    resolution: text(selectedWork.resolution) as "accepted" | "cancelled" | "failed" || undefined,
  } : undefined;

  const approvalRow = records(root.approvals)[0] ?? {};
  const requestedBy = resolveActor(approvalRow.requested_by);
  const requiredApprover = resolveActor(records(approvalRow.required_approver_refs)[0] ?? approvalRow.required_approver_refs);
  const approvalDefinition = records(root.custom_page_definitions)
    .find((definition) => strings(definition.action_command_refs).includes("approval.decide"));
  const subject = entityRef(approvalRow.subject_ref);
  const requestedByRef = actorRef(approvalRow.requested_by);
  const requiredRefs = records(approvalRow.required_approver_refs).map(actorRef).filter((ref): ref is CanonicalActorRef => Boolean(ref));
  const approval: ApprovalView = {
    id: text(approvalRow.id, "unresolved-approval"),
    title: text(approvalRow.action_summary, "Approval"),
    actionSummary: text(approvalRow.action_summary, "No approval supplied"),
    status: text(approvalRow.status, "requested") as ApprovalView["status"],
    requestedBy,
    requiredApprover,
    expiresAt: text(approvalRow.expires_at) || undefined,
    decisionContext: approvalDefinition && subject && requestedByRef && requiredRefs.length ? {
      definitionId: text(approvalDefinition.id),
      actionPolicyRef: `${text(approvalDefinition.id)}:approval.decide`,
      recordSubjectRef: subject,
      requestedBy: requestedByRef,
      requiredApproverRefs: requiredRefs,
      requiredActorType: text(approvalRow.required_actor_type) || undefined,
      recordPolicyRef: text(approvalRow.policy_ref),
      rawActionSummary: text(approvalRow.action_summary),
      evidenceRefs: strings(approvalRow.evidence_refs),
      requestedAt: text(approvalRow.requested_at),
      expiresAt: text(approvalRow.expires_at) || undefined,
    } : undefined,
  };

  const financialRows = records(root.financial_records);
  const commitmentRow = financialRows.find((row) => text(row.type) === "commitment") ?? records(root.commitments)[0] ?? {};
  const commitmentRecord = object(commitmentRow.record);
  const finance = Object.keys(commitmentRecord).length ? commitmentRecord : commitmentRow;
  const sourceDocument = related(sourceDocumentRow.id, sourceDocumentRow.title, sourceDocumentRow.space_id);
  const commitment: FinancialRecordView = {
    id: text(finance.id, "unresolved-commitment"),
    label: text(commitmentRow.display_name, "Financial commitment"),
    type: "commitment",
    amount: text(commitmentRow.display_amount, money(finance)),
    status: text(finance.status) === "approved" ? "approved" : text(finance.status) === "fulfilled" ? "settled" : "pending_approval",
    sourceDocument,
    accountableOwner: resolveActor(finance.accountable_owner),
  };

  const membershipProjections: AgentMemberExecutionAssignment[] = records(root.membership_projections).map((row) => ({
    id: text(row.id),
    agentMemberId: text(row.agent_member_id),
    sourceKind: (text(row.source_kind) === "agent_team_work" ? "agent_team_work" : "agent_team_participation") as AgentMemberExecutionAssignment["sourceKind"],
    sourceRef: text(row.source_ref) || undefined,
    workId: text(row.work_id) || undefined,
    missionId: text(row.mission_id) || undefined,
    waveId: text(row.wave_id) || undefined,
    teamRunId: text(row.team_run_id),
    memberRunId: text(row.member_run_id),
    title: text(row.title, "TeamWork"),
    role: text(row.role, "member"),
    status: text(row.status, "unknown"),
    assignedAt: text(row.assigned_at),
    lastActivityAt: text(row.last_activity_at) || undefined,
    nativeSession: object(row.native_session),
  })).filter((row) => row.id && row.agentMemberId && row.teamRunId && row.memberRunId);
  return {
    fixtureId: text(root.fixture_id) || undefined,
    actors,
    actorList: Object.values(actors),
    organization: {
      units,
      memberships,
      rootUnitIds: units.filter((unit) => !unit.parentId).map((unit) => unit.id),
      unplacedUnitIds: units.filter((unit) => unit.parentId && !unitIds.has(unit.parentId)).map((unit) => unit.id),
      unassignedActorIds,
      integrityFindings,
    },
    sourceDocument,
    contentPlanDocument: related(contentPlanRow.id, contentPlanRow.title, contentPlanRow.space_id),
    typedApplication: related(typed.id, typed.title, typed.record_type),
    linkedWork,
    linkedTypedRecords: [],
    linkedApproval: approval,
    linkedCommitment: commitment,
    membershipProjections,
    commitment,
    approval,
    evidence: strings(approvalRow.evidence_refs).map((id) => related(id, id, "Approval evidence")),
    governanceProposal: { ...related("unresolved-proposal", "No governance proposal supplied"), proposedById: undefined },
    businessModule: related(records(root.business_modules)[0]?.id, records(root.business_modules)[0]?.name, records(root.business_modules)[0]?.status),
    julySpendMetric: related("unresolved-metric", "No spend metric supplied"),
    julySpendAmount: "—",
  };
}

export const prototypeTrademarkOperationsProjection = adaptTrademarkOperationsProjection({
  fixture_id: "company-os-prototype-empty",
});
