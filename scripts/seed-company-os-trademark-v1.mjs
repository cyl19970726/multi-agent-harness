#!/usr/bin/env node

/**
 * Deterministic Company OS acceptance seed.
 *
 * The script writes only through the public Company OS HTTP API. In its
 * default orchestration mode it creates an isolated temporary HOME/project,
 * starts the real `harness serve`, seeds the canonical trademark scenario,
 * optionally invokes the live screenshot runner, archives the resulting
 * ledgers/snapshot as evidence, and removes the temporary working directory.
 */

import { spawn, spawnSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixturePath = join(repoRoot, "docs/design/company-os-v1/fixtures/company-os-trademark-v1.json");
const defaultRunId = "company-os-v1-live-acceptance";
const ADMIN_ID = "actor-human-brand-owner";
const NOW = "2026-07-20T09:30:00+08:00";

function argument(name, fallback = "") {
  const index = process.argv.indexOf(name);
  return index === -1 ? fallback : process.argv[index + 1];
}

function flag(name) {
  return process.argv.includes(name);
}

function actorRef(actorType, actorId) {
  return { actor_type: actorType, actor_id: actorId };
}

function actorKind(canonicalType) {
  if (canonicalType === "Human") return "human";
  if (canonicalType === "Standing Agent") return "agent";
  if (canonicalType === "External") return "external";
  throw new Error(`unsupported canonical actor type: ${canonicalType}`);
}

function admin(record) {
  return { mode: "administrative", authority: actorRef("human", ADMIN_ID), record };
}

async function freePort() {
  return await new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close(() => resolvePort(address.port));
    });
  });
}

async function waitFor(url, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {}
    await new Promise((resolveWait) => setTimeout(resolveWait, 80));
  }
  throw new Error(`timed out waiting for ${url}`);
}

async function requestJson(apiBaseUrl, path, { token, body } = {}) {
  const response = await fetch(new URL(path, apiBaseUrl), {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? undefined : {
      "content-type": "application/json",
      "x-harness-company-os-token": token,
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok || payload.ok === false) {
    throw new Error(`${body === undefined ? "GET" : "POST"} ${path} failed (${response.status}): ${JSON.stringify(payload)}`);
  }
  return payload.result ?? payload;
}

function humanRecord(canonical) {
  return {
    actor_type: "human",
    actor: {
      id: canonical.id,
      display_name: canonical.display_name,
      title: "Business owner",
      status: "active",
      availability: null,
      membership_refs: [`membership-${canonical.id}`],
      responsibility_summary: "Accountable owner for company brand, legal, finance, and organization decisions.",
      permission_policy_refs: ["company_os.admin", "company.records.write", "company.work.execute", "finance.commitment.write"],
      authority_policy_refs: ["company_os.admin", "company.approve", "policy-human-approval-financial-and-legal-submission"],
      created_at: canonical.created_at,
      updated_at: NOW,
    },
  };
}

function agentRecord(canonical) {
  const documentArchitecture = canonical.id === "actor-agent-document-architecture";
  return {
    actor_type: "agent",
    actor: {
      id: canonical.id,
      display_name: canonical.display_name,
      role: canonical.id === "actor-agent-trademark" ? "Proposed trademark role" : canonical.display_name.replace(/ Agent$/, ""),
      status: "active",
      availability: documentArchitecture ? "available" : "unknown",
      assignment_capacity: null,
      exclusive_assignment_ref: null,
      membership_refs: [`membership-${canonical.id}`],
      responsibility_summary: documentArchitecture
        ? "Maintain the company document architecture and propose governed business modules."
        : `Perform the declared ${canonical.display_name} responsibility without inferred runtime presence.`,
      capability_refs: ["company.records.write", "company.work.execute"],
      permission_policy_refs: canonical.id === "actor-agent-trademark"
        ? ["company.records.write", "company.work.execute", "finance.commitment.write"]
        : ["company.records.write"],
      runtime_refs: [],
      provider_session_refs: [],
      created_at: canonical.created_at,
      updated_at: NOW,
    },
  };
}

function externalRecord(canonical) {
  return {
    actor_type: "external",
    actor: {
      id: canonical.id,
      display_name_or_organization: canonical.display_name,
      engagement_scope: "Matter-specific legal review for trademark application CN-2026-018.",
      sponsor_actor_ref: actorRef("human", ADMIN_ID),
      access_expires_at: "2026-12-31T23:59:59+08:00",
      confidentiality_or_contract_refs: ["contract-external-lawyer-trademark-2026"],
      membership_refs: [`membership-${canonical.id}`],
      restricted_permission_refs: ["trademark.review"],
      status: "active",
      created_at: canonical.created_at,
      updated_at: NOW,
    },
  };
}

function documentRecord(entry, actorTypes) {
  const ownerId = entry.owner_ref ?? ADMIN_ID;
  const owner = actorRef(actorTypes.get(ownerId) ?? "human", ownerId);
  const contentReferences = entry.id === "document-brand-a-content-operating-plan"
    ? [
        { kind: "actor", id: "actor-agent-content-strategy" },
        { kind: "document", id: "document-trademark-application-cn-2026-018" },
      ]
    : [];
  return {
    id: entry.id,
    space_id: entry.space,
    parent_document_id: null,
    title: entry.title,
    kind: "record",
    lifecycle_status: "active",
    block_ids: [],
    template_ref: null,
    permission_policy_refs: ["company.records.write"],
    reference_refs: contentReferences,
    created_by: owner,
    updated_by: owner,
    created_at: entry.created_at,
    updated_at: entry.updated_at,
  };
}

function typedRecord(entry) {
  const isBrand = entry.id === "brand-brand-a";
  return {
    id: entry.id,
    module_id: "module-trademark-management",
    record_type: entry.record_type,
    title: isBrand ? entry.display_name : `Trademark application ${entry.display_id}`,
    fields: { ...entry },
    lifecycle_status: entry.status ?? "active",
    source_document_ref: entry.source_document_ref ?? (isBrand ? "document-brand-a-content-operating-plan" : null),
    created_by: actorRef(isBrand ? "human" : "agent", isBrand ? ADMIN_ID : "actor-agent-trademark"),
    updated_by: actorRef(isBrand ? "human" : "agent", isBrand ? ADMIN_ID : "actor-agent-trademark"),
    created_at: entry.created_at,
    updated_at: entry.updated_at,
  };
}

export async function seedCompanyOsTrademark({ apiBaseUrl, token, fixture }) {
  const actorTypes = new Map(fixture.actors.map((entry) => [entry.id, actorKind(entry.actor_type)]));
  const post = (resource, record, bootstrap = false) => requestJson(apiBaseUrl, `/v1/company-os/${resource}`, {
    token,
    body: bootstrap ? record : admin(record),
  });

  const root = fixture.actors.find((entry) => entry.id === ADMIN_ID);
  await post("actors", humanRecord(root), true);
  for (const entry of fixture.actors.filter((actor) => actor.id !== ADMIN_ID)) {
    const record = entry.actor_type === "Standing Agent" ? agentRecord(entry) : externalRecord(entry);
    await post("actors", record);
  }

  for (const entry of fixture.documents) await post("documents", documentRecord(entry, actorTypes));
  await post("blocks", {
    id: "block-trademark-approval-context",
    document_id: "document-trademark-application-cn-2026-018",
    kind: "callout",
    position: 0,
    content: { text: "Filing and the ¥3,000 commitment remain blocked on the requested Human approval." },
    // The visible links are added through the typed WorkItem/Commitment later;
    // this early document block deliberately has no forward references.
    referenced_entities: [],
    created_by: actorRef("agent", "actor-agent-trademark"),
    updated_by: actorRef("agent", "actor-agent-trademark"),
    created_at: NOW,
    updated_at: NOW,
  });

  for (const entry of fixture.organization.org_units) {
    await post("org-units", {
      id: entry.id,
      organization_id: "company",
      name: entry.name,
      purpose: entry.id === "org-company" ? "Company operating organization" : `Own ${entry.name} responsibilities`,
      parent_unit_id: entry.parent_id ?? null,
      status: "active",
      human_lead_actor_ref: entry.id === "org-brand-ip" ? actorRef("human", ADMIN_ID) : null,
      agent_lead_actor_ref: entry.id === "org-brand-ip" ? actorRef("agent", "actor-agent-ip-lead") : null,
      policy_refs: ["company.records.write"],
      document_space_ref: entry.name,
      created_at: entry.created_at,
      updated_at: NOW,
    });
  }
  for (const entry of fixture.organization.memberships) {
    const type = actorTypes.get(entry.actor_id);
    await post("memberships", {
      id: `membership-${entry.actor_id}`,
      organization_id: "company",
      org_unit_id: entry.org_unit_id,
      actor_ref: actorRef(type, entry.actor_id),
      membership_role: type === "external" ? "external_partner" : [ADMIN_ID, "actor-agent-ip-lead"].includes(entry.actor_id) ? "lead" : "member",
      title_or_function: entry.role_label,
      status: "active",
      starts_at: entry.created_at,
      ends_at: null,
      authority_policy_refs: entry.actor_id === ADMIN_ID ? ["company.approve"] : [],
      created_by_actor_ref: actorRef("human", ADMIN_ID),
      created_at: entry.created_at,
    });
  }

  await post("business-modules", {
    id: "module-trademark-management",
    name: "Trademark Management",
    purpose: "Govern trademark applications, responsibilities, approval, finance, and evidence as one linked truth.",
    root_document_ref: "document-trademark-application-cn-2026-018",
    record_types: ["TrademarkApplication", "Brand", "Governance_Proposal", "Metric_Observation"],
    relation_rules: [],
    default_view_refs: [],
    policy_refs: ["company.records.write", "policy-human-approval-financial-and-legal-submission"],
    lifecycle_rules: ["human_approval_before_financial_or_legal_effect"],
    metric_definition_refs: [],
    custom_page_definition_refs: [],
    status: "draft",
    owner: actorRef("human", ADMIN_ID),
    created_at: "2026-07-18T10:00:00+08:00",
    updated_at: NOW,
  });
  await post("views", {
    id: "view-trademark-management",
    module_id: "module-trademark-management",
    title: "Trademark applications",
    mode: "table",
    source_kinds: ["typed_record", "work_item", "financial_record"],
    query: { module_id: "module-trademark-management" },
    owner: actorRef("human", ADMIN_ID),
    policy_refs: ["company.records.write"],
    created_at: "2026-07-18T10:00:00+08:00",
    updated_at: NOW,
  });
  await post("custom-page-packages", {
    id: "package-trademark",
    definition_id: "page-trademark",
    version: "1.0.0",
    kind: "react",
    artifact_ref: "artifact://company-os/trademark-management/v1",
    entrypoint: "TrademarkManagementPage",
    integrity_digest: "sha256:company-os-trademark-v1-live-contract",
    built_at: NOW,
  });
  await post("custom-page-definitions", {
    id: "page-trademark",
    module_id: "module-trademark-management",
    purpose: "Present the governed trademark chain without authorizing payment or legal submission.",
    allowed_data_queries: [{
      id: "query-trademark-chain",
      source_kind: "financial_record",
      source_scope: "module-trademark-management",
      permission_policy_ref: "company.records.write",
    }],
    approved_ui_components: ["DocumentCard", "WorkItemCard", "ApprovalCard", "FinancialRecordCard"],
    action_command_refs: ["approval.decide", "work_item.transition", "commitment.append", "payment.append"],
    standard_view_fallback_ref: "view-trademark-management",
    owner: actorRef("human", ADMIN_ID),
    package_ref: "package-trademark",
    package_version: "1.0.0",
    fixture_ref: "company-os-trademark-v1",
    visual_contract_ref: "docs/design/company-os-v1/visual-contract.json",
    policy_refs: [
      "page-trademark:approval.decide",
      "page-trademark:work_item.transition",
      "page-trademark:commitment.append",
      "page-trademark:payment.append",
    ],
    created_at: "2026-07-18T10:00:00+08:00",
    updated_at: NOW,
  });

  for (const entry of fixture.typed_records) await post("typed-records", typedRecord(entry));
  const proposal = fixture.governance_proposals[0];
  await post("typed-records", {
    id: proposal.id,
    module_id: "module-trademark-management",
    record_type: "Governance_Proposal",
    title: proposal.title,
    fields: { ...proposal },
    lifecycle_status: proposal.status,
    source_document_ref: "document-trademark-application-cn-2026-018",
    created_by: actorRef("agent", proposal.proposed_by_ref),
    updated_by: actorRef("agent", proposal.proposed_by_ref),
    created_at: proposal.created_at,
    updated_at: proposal.updated_at,
  });
  const metric = fixture.explicit_metrics[0];
  await post("typed-records", {
    id: metric.id,
    module_id: "module-trademark-management",
    record_type: "Metric_Observation",
    title: metric.label,
    fields: { ...metric },
    lifecycle_status: "active",
    source_document_ref: "document-brand-a-content-operating-plan",
    created_by: actorRef("agent", "actor-agent-finance"),
    updated_by: actorRef("agent", "actor-agent-finance"),
    created_at: metric.observed_at,
    updated_at: metric.observed_at,
  });

  const work = fixture.work_items[0];
  const milestoneRecord = {
    id: "milestone-trademark-application-submitted",
    title: "Trademark application submitted",
    outcome: "The governed CN application has durable filing receipt evidence.",
    status: "active",
    accountable_owner: actorRef("human", work.accountable_owner_ref),
    source_document_ref: work.source_document_ref,
    business_module_ref: "module-trademark-management",
    target_at: "2026-07-31T18:00:00+08:00",
    acceptance_criteria: ["Human approval is recorded", "Filing receipt evidence is linked"],
    work_item_refs: [],
    created_at: work.created_at,
    updated_at: work.updated_at,
    achieved_at: null,
  };
  await post("milestones", milestoneRecord);
  const workRecord = {
    id: work.id,
    title: work.title,
    objective: "Prepare the Brand A trademark filing package and stop for Human approval before legal or financial effect.",
    status: "waiting_for_approval",
    source_document_ref: work.source_document_ref,
    source_record_refs: work.source_record_refs,
    milestone_ref: milestoneRecord.id,
    work_type: "legal",
    business_module_ref: "module-trademark-management",
    result_document_ref: work.result_document_ref,
    result_record_refs: ["trademark-application-cn-2026-018"],
    submitted_by: actorRef("agent", work.submitted_by_ref),
    requested_by: actorRef("human", work.requested_by_ref),
    accountable_owner: actorRef("human", work.accountable_owner_ref),
    assignees: work.assignee_refs.map((id) => actorRef("agent", id)),
    contributors: work.contributor_refs.map((id) => actorRef(actorTypes.get(id), id)),
    reviewer: actorRef("agent", work.reviewer_ref),
    approver: actorRef("human", work.approver_ref),
    execution_mode: "mixed",
    execution_refs: [],
    // The requested Approval is appended after the Proposed Commitment. The
    // WorkItem is then appended again with this durable link (latest-row-wins).
    approval_refs: [],
    evidence_refs: work.evidence_refs,
    artifact_refs: work.evidence_refs,
    outcome_summary: null,
    due_at: "2026-07-31T18:00:00+08:00",
    priority: "high",
    risk_level: "legal_and_financial",
    created_at: work.created_at,
    updated_at: work.updated_at,
    completed_at: null,
  };
  await post("work-items", workRecord);
  await post("milestones", { ...milestoneRecord, work_item_refs: [workRecord.id] });
  const assignment = fixture.assignments[0];
  await post("assignments", {
    id: assignment.id,
    work_item_id: assignment.work_item_ref,
    recipient: actorRef("agent", assignment.assignee_ref),
    sender: actorRef("human", ADMIN_ID),
    assigned_role: "Trademark filing owner",
    scope: "Prepare CN-2026-018 and stop at the Human approval boundary.",
    delivery_state: "acknowledged",
    delivery_policy_ref: "company.records.write",
    correlation_id: "corr-trademark-cn-2026-018",
    delivery_evidence_ref: "evidence-assignment-delivered-cn-2026-018",
    assigned_at: assignment.assigned_at,
    delivered_at: assignment.assigned_at,
    acknowledged_at: assignment.accepted_at,
  });
  await post("relations", {
    id: "relation-trademark-application-work",
    from_ref: { kind: "typed_record", id: "trademark-application-cn-2026-018" },
    relation_type: "implemented_by",
    to_ref: { kind: "work_item", id: work.id },
    provenance_ref: { kind: "document", id: work.source_document_ref },
    created_by: actorRef("human", ADMIN_ID),
    created_at: work.created_at,
  });

  const commitmentFixture = fixture.financial_records[0];
  const proposedCommitment = {
    id: commitmentFixture.id,
    amount: { amount: String(commitmentFixture.amount), currency: commitmentFixture.currency },
    status: "proposed",
    source_document_id: commitmentFixture.source_document_ref,
    submitted_by: actorRef("agent", commitmentFixture.submitted_by_ref),
    accountable_owner: actorRef("human", commitmentFixture.accountable_owner_ref),
    relation_ids: ["relation-trademark-application-work"],
    evidence_refs: commitmentFixture.evidence_refs,
    approval_refs: [],
    audit_event_ids: ["audit-trademark-commitment-proposed"],
    due_at: "2026-07-31T18:00:00+08:00",
    created_at: commitmentFixture.created_at,
    updated_at: commitmentFixture.created_at,
  };
  await post("commitments", proposedCommitment);

  const approvalFixture = fixture.approvals[0];
  const requestedApproval = {
    id: approvalFixture.id,
    subject_ref: { kind: "financial_record", id: commitmentFixture.id },
    action_summary: "Authorize commitment.append to enter the ¥3,000 trademark fee commitment into Human review; legal submission remains blocked.",
    requested_by: actorRef("agent", approvalFixture.requested_by_ref),
    required_approver_refs: [actorRef("human", ADMIN_ID)],
    required_actor_type: "human",
    policy_ref: "page-trademark:commitment.append",
    status: "requested",
    decided_by: [],
    decision_note: null,
    evidence_refs: approvalFixture.evidence_refs,
    requested_at: approvalFixture.requested_at,
    decided_at: null,
    expires_at: approvalFixture.expires_at,
  };
  await post("approvals", requestedApproval);
  await post("work-items", { ...workRecord, approval_refs: [requestedApproval.id] });

  // A small native cross-line ledger proves that Work views are projections,
  // not a trademark-specific page. These records use existing governed Actors
  // and durable source Documents; they create no Project or task graph.
  const workExpansionDocuments = [
    ["document-finance-july-review", "July finance operating review", "Finance"],
    ["document-company-os-work-rollout", "Company OS · Work rollout", "Product & Engineering"],
  ];
  for (const [id, title, space] of workExpansionDocuments) await post("documents", {
    id, space_id: space, parent_document_id: null, title, kind: "page",
    lifecycle_status: "active", block_ids: [], template_ref: null,
    permission_policy_refs: ["company.records.write"], reference_refs: [],
    created_by: actorRef("human", ADMIN_ID), updated_by: actorRef("human", ADMIN_ID),
    created_at: NOW, updated_at: NOW,
  });
  const workExpansionModules = [
    ["module-content-operations", "Content Operations", "document-brand-a-content-operating-plan", "Plan, publish, and measure content work."],
    ["module-finance-operations", "Finance Operations", "document-finance-july-review", "Review commitments and maintain company financial operations."],
    ["module-product-engineering", "Product & Engineering", "document-company-os-work-rollout", "Deliver governed Company OS product capabilities."],
  ];
  for (const [id, name, root_document_ref, purpose] of workExpansionModules) await post("business-modules", {
    id, name, purpose, root_document_ref, record_types: [], relation_rules: [],
    default_view_refs: [], policy_refs: ["company.records.write"], lifecycle_rules: [],
    metric_definition_refs: [], custom_page_definition_refs: [], status: "active",
    owner: actorRef("human", ADMIN_ID), created_at: NOW, updated_at: NOW,
  });
  const workExpansionMilestones = [
    {
      id: "milestone-content-campaign-live", title: "Brand campaign live",
      outcome: "The campaign is published and its first performance observation is recorded.", status: "active",
      accountable_owner: actorRef("agent", "actor-agent-content-strategy"),
      source_document_ref: "document-brand-a-content-operating-plan", business_module_ref: "module-content-operations",
      target_at: "2026-07-28T18:00:00+08:00", acceptance_criteria: ["Campaign is published", "First metric observation is linked"], work_item_refs: [],
    },
    {
      id: "milestone-july-finance-reviewed", title: "July commitments reviewed",
      outcome: "Open commitments have accountable review and explicit approval pressure.", status: "at_risk",
      accountable_owner: actorRef("agent", "actor-agent-finance"),
      source_document_ref: "document-finance-july-review", business_module_ref: "module-finance-operations",
      target_at: "2026-07-25T18:00:00+08:00", acceptance_criteria: ["Every open commitment has a review state"], work_item_refs: [],
    },
    {
      id: "milestone-work-os-released", title: "Work operating system released",
      outcome: "Company work is visible through one native multi-view ledger.", status: "active",
      accountable_owner: actorRef("agent", "actor-agent-document-architecture"),
      source_document_ref: "document-company-os-work-rollout", business_module_ref: "module-product-engineering",
      target_at: "2026-08-05T18:00:00+08:00", acceptance_criteria: ["Native queries pass", "Actual visual evidence is linked"], work_item_refs: [],
    },
  ];
  for (const milestone of workExpansionMilestones) await post("milestones", {
    ...milestone, created_at: NOW, updated_at: NOW, achieved_at: null,
  });
  const additionalWork = [
    {
      id: "work-content-publish-launch-video", title: "Publish Brand A launch video", objective: "Publish the approved launch asset and return its URL to the content plan.",
      status: "in_progress", source_document_ref: "document-brand-a-content-operating-plan", milestone_ref: "milestone-content-campaign-live", work_type: "content", business_module_ref: "module-content-operations",
      accountable_owner: actorRef("agent", "actor-agent-content-strategy"), assignees: [actorRef("agent", "actor-agent-content-strategy")], reviewer: actorRef("human", ADMIN_ID), due_at: "2026-07-26T18:00:00+08:00", priority: "high", risk_level: "medium",
    },
    {
      id: "work-content-measure-first-24h", title: "Measure first 24-hour campaign performance", objective: "Record views, engagement, and the next adjustment recommendation.",
      status: "submitted", source_document_ref: "document-brand-a-content-operating-plan", milestone_ref: "milestone-content-campaign-live", work_type: "research", business_module_ref: "module-content-operations",
      accountable_owner: actorRef("agent", "actor-agent-content-strategy"), assignees: [actorRef("agent", "actor-agent-analytics")], reviewer: actorRef("agent", "actor-agent-content-strategy"), due_at: "2026-07-29T18:00:00+08:00", priority: "medium", risk_level: "low",
    },
    {
      id: "work-finance-review-open-commitments", title: "Review July open commitments", objective: "Confirm owner, evidence, and approval pressure for every July commitment.",
      status: "blocked", source_document_ref: "document-finance-july-review", milestone_ref: "milestone-july-finance-reviewed", work_type: "finance", business_module_ref: "module-finance-operations",
      accountable_owner: actorRef("human", ADMIN_ID), assignees: [actorRef("agent", "actor-agent-finance")], reviewer: actorRef("human", ADMIN_ID), due_at: "2026-07-25T18:00:00+08:00", priority: "high", risk_level: "financial",
    },
    {
      id: "work-engineering-native-work-query", title: "Ship native Work query projection", objective: "Expose Milestone, WorkType, business-line, Board, and workload truth through one read model.",
      status: "in_review", source_document_ref: "document-company-os-work-rollout", milestone_ref: "milestone-work-os-released", work_type: "development", business_module_ref: "module-product-engineering",
      accountable_owner: actorRef("agent", "actor-agent-document-architecture"), assignees: [actorRef("agent", "actor-agent-document-architecture")], reviewer: actorRef("human", ADMIN_ID), due_at: "2026-07-23T18:00:00+08:00", priority: "high", risk_level: "product",
    },
    {
      id: "work-governance-review-work-taxonomy", title: "Review Work type governance", objective: "Confirm the company taxonomy stays small and domain-neutral.",
      status: "accepted", source_document_ref: "document-company-os-work-rollout", milestone_ref: "milestone-work-os-released", work_type: "governance", business_module_ref: "module-product-engineering",
      accountable_owner: actorRef("agent", "actor-agent-organization-governance"), assignees: [], reviewer: actorRef("human", ADMIN_ID), due_at: "2026-07-24T18:00:00+08:00", priority: "medium", risk_level: "governance",
    },
  ];
  for (const item of additionalWork) await post("work-items", {
    ...item, source_record_refs: [], result_document_ref: null, result_record_refs: [],
    submitted_by: actorRef("agent", "actor-agent-document-architecture"), requested_by: actorRef("human", ADMIN_ID),
    contributors: [], approver: null, execution_mode: "direct", execution_refs: [], approval_refs: [],
    evidence_refs: [], artifact_refs: [], outcome_summary: null, created_at: NOW, updated_at: NOW, completed_at: null,
  });
  for (const milestone of workExpansionMilestones) await post("milestones", {
    ...milestone,
    work_item_refs: additionalWork.filter((item) => item.milestone_ref === milestone.id).map((item) => item.id),
    created_at: NOW, updated_at: NOW, achieved_at: null,
  });

  const pendingCommitment = {
    ...proposedCommitment,
    status: "pending_approval",
    approval_refs: [requestedApproval.id],
    audit_event_ids: [
      "audit-trademark-commitment-proposed",
      "audit-trademark-commitment-pending-approval",
      "audit-action-trademark-commitment-enter-approval-queue",
    ],
    updated_at: commitmentFixture.updated_at,
  };
  await requestJson(apiBaseUrl, "/v1/company-os/actions/dispatch", {
    token,
    body: {
      id: "action-trademark-commitment-enter-approval-queue",
      command_name: "commitment.append",
      subject_ref: { kind: "financial_record", id: commitmentFixture.id },
      requested_by: actorRef("agent", "actor-agent-trademark"),
      payload: { definition_id: "page-trademark", record: pendingCommitment },
      required_permission: "finance.commitment.write",
      policy_ref: "page-trademark:commitment.append",
      risk_tier: "r3",
      requires_human_approval: true,
      approval_refs: [requestedApproval.id],
      status: "requested",
      audit_event_refs: ["audit-action-trademark-commitment-enter-approval-queue"],
      requested_at: NOW,
      completed_at: null,
    },
  });

  const snapshot = await requestJson(apiBaseUrl, "/v1/company-os/snapshot");
  const approval = snapshot.approvals.find((entry) => entry.id === requestedApproval.id);
  const commitment = snapshot.commitments.find((entry) => entry.id === pendingCommitment.id);
  if (approval?.status !== "requested" || approval.decided_at !== null || approval.decided_by?.length) {
    throw new Error(`seed crossed the Human gate: ${JSON.stringify(approval)}`);
  }
  if (commitment?.status !== "pending_approval" || commitment.amount?.amount !== "3000" || commitment.amount?.currency !== "CNY") {
    throw new Error(`seed did not produce the canonical pending Commitment: ${JSON.stringify(commitment)}`);
  }
  if (snapshot.payments.length !== 0 || snapshot.financial_records.some((entry) => entry.type === "payment")) {
    throw new Error("seed created a Payment before Human approval");
  }
  if (snapshot.approvals.some((entry) => entry.status === "approved")) {
    throw new Error("seed created an approved Approval; Wave 7 must stop at requested Human approval");
  }
  if (snapshot.milestones?.length !== 4
      || snapshot.work?.work_types?.legal?.[0] !== workRecord.id
      || snapshot.work?.business_lines?.["module-trademark-management"]?.[0] !== workRecord.id
      || snapshot.work?.business_lines?.["module-content-operations"]?.length !== 2
      || snapshot.work?.business_lines?.["module-product-engineering"]?.length !== 2) {
    throw new Error(`seed did not produce the native Work projection: ${JSON.stringify(snapshot.work)}`);
  }
  return snapshot;
}

async function main() {
  const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
  const externalApi = argument("--api-base-url");
  const suppliedToken = argument("--token");
  if (externalApi) {
    if (!suppliedToken) throw new Error("--api-base-url requires --token");
    const snapshot = await seedCompanyOsTrademark({ apiBaseUrl: externalApi, token: suppliedToken, fixture });
    console.log(JSON.stringify({ status: "seeded", source: snapshot.source, counts: projectionCounts(snapshot) }, null, 2));
    return;
  }

  const harnessBinary = resolve(argument("--harness-binary", join(repoRoot, "target/debug/harness")));
  const runId = argument("--run-id", defaultRunId);
  const captureContract = argument("--capture-contract", "v1");
  if (!new Set(["v1", "v2.2"]).has(captureContract)) throw new Error("--capture-contract must be v1 or v2.2");
  const evidenceWorkstream = captureContract === "v2.2" ? "company-os-v2" : "company-os-v1";
  const evidenceRoot = resolve(argument("--output", join(repoRoot, `.visual-evidence/${evidenceWorkstream}`, runId)));
  const token = `company-os-live-seed-${process.pid}`;
  const temporaryRoot = await mkdtemp(join(tmpdir(), "company-os-v1-live-"));
  const home = join(temporaryRoot, "home");
  const harnessHome = join(home, ".harness");
  const projectRoot = join(temporaryRoot, "company");
  await mkdir(harnessHome, { recursive: true });
  await mkdir(projectRoot, { recursive: true });
  const env = {
    ...process.env,
    HOME: home,
    HARNESS_HOME: harnessHome,
    HARNESS_COMPANY_OS_TOKEN: token,
  };
  delete env.HARNESS_ROOT;
  delete env.HARNESS_PROJECT;

  const init = spawnSync(harnessBinary, ["init"], { cwd: projectRoot, env, encoding: "utf8" });
  if (init.status !== 0) throw new Error(`harness init failed: ${init.stderr || init.stdout}`);
  const port = await freePort();
  const apiBaseUrl = `http://127.0.0.1:${port}`;
  const serverLog = [];
  const server = spawn(harnessBinary, ["serve", "--addr", `127.0.0.1:${port}`, "--no-truncate"], {
    cwd: projectRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  server.stdout.on("data", (chunk) => serverLog.push(chunk.toString()));
  server.stderr.on("data", (chunk) => serverLog.push(chunk.toString()));

  try {
    await waitFor(`${apiBaseUrl}/health`);
    const projects = await requestJson(apiBaseUrl, "/v1/projects");
    const project = projects.projects.find((entry) => entry.current) ?? projects.projects[0];
    if (!project?.id) throw new Error(`serve did not expose the isolated project: ${JSON.stringify(projects)}`);
    const snapshot = await seedCompanyOsTrademark({ apiBaseUrl, token, fixture });
    await mkdir(evidenceRoot, { recursive: true });
    await writeFile(join(evidenceRoot, "live-company-os-snapshot.json"), `${JSON.stringify(snapshot, null, 2)}\n`);
    const seedManifest = {
      contract: "company-os-trademark-live-seed-v1",
      status: "passed",
      project_id: project.id,
      source: snapshot.source,
      fixture: "docs/design/company-os-v1/fixtures/company-os-trademark-v1.json",
      capture_contract: captureContract,
      transport: "HARNESS_COMPANY_OS_TOKEN + administrative envelope",
      archived_store: "archived-harness-home",
      counts: projectionCounts(snapshot),
      gate: {
        phase: "before_optional_browser_action",
        approval_id: "approval-trademark-filing-fee-cn-2026-018",
        approval_status: "requested",
        commitment_id: "financial-commitment-trademark-filing-fee-cn-2026-018",
        commitment_status: "pending_approval",
        amount: { amount: "3000", currency: "CNY" },
        payment_count: 0,
        approved_approval_count: 0,
      },
    };
    await writeFile(join(evidenceRoot, "seed-manifest.json"), `${JSON.stringify(seedManifest, null, 2)}\n`);

    if (flag("--capture")) {
      await rm(join(evidenceRoot, "implemented"), { recursive: true, force: true });
      await rm(join(evidenceRoot, "store-live-actual"), { recursive: true, force: true });
      await rm(join(evidenceRoot, "capture-run.json"), { force: true });
      const captureScript = captureContract === "v2.2"
        ? "scripts/capture-company-os-v2.mjs"
        : "scripts/capture-company-os-v1.mjs";
      const captureArgs = [
        join(repoRoot, captureScript),
        "--data-mode", "live",
        "--api-base-url", apiBaseUrl,
        "--project-id", project.id,
        "--run-id", runId,
        "--output", evidenceRoot,
      ];
      if (captureContract === "v1") captureArgs.splice(3, 0, "--stage", "implemented");
      if (captureContract === "v2.2" && flag("--capture-approval-action")) {
        captureArgs.push("--approval-action-token", token);
        captureArgs.push("--approval-action-decision", argument("--approval-action-decision", "approved"));
      }
      if (captureContract === "v2.2" && flag("--capture-workitem-action")) {
        captureArgs.push("--workitem-action-token", token);
      }
      if (captureContract === "v2.2" && flag("--capture-work-views")) {
        captureArgs.push("--capture-work-views");
      }
      if (captureContract === "v2.2" && flag("--work-views-only")) {
        captureArgs.push("--work-views-only");
      }
      for (const option of ["--viewport-width", "--viewport-height", "--viewport-name"]) {
        const value = argument(option);
        if (value) captureArgs.push(option, value);
      }
      const capture = spawnSync(process.execPath, captureArgs, { cwd: repoRoot, env: process.env, stdio: "inherit" });
      if (capture.status !== 0) throw new Error(`live Company OS capture failed with status ${capture.status}`);
    }

    await rm(join(evidenceRoot, "archived-harness-home"), { recursive: true, force: true });
    await cp(harnessHome, join(evidenceRoot, "archived-harness-home"), { recursive: true });
    await writeFile(join(evidenceRoot, "serve.log"), serverLog.join(""));
    console.log(JSON.stringify({ ...seedManifest, evidence_root: evidenceRoot }, null, 2));
  } finally {
    server.kill("SIGTERM");
    await new Promise((resolveWait) => {
      const timer = setTimeout(() => { server.kill("SIGKILL"); resolveWait(); }, 2_000);
      server.once("exit", () => { clearTimeout(timer); resolveWait(); });
    });
    await rm(temporaryRoot, { recursive: true, force: true });
  }
}

function projectionCounts(snapshot) {
  const keys = ["actors", "documents", "blocks", "typed_records", "relations", "views", "business_modules", "work_items", "assignments", "approvals", "commitments", "payments", "custom_page_definitions", "custom_page_packages", "action_commands", "audit_events"];
  return Object.fromEntries(keys.map((key) => [key, Array.isArray(snapshot[key]) ? snapshot[key].length : 0]));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(error.stack || error.message);
    process.exit(1);
  });
}
