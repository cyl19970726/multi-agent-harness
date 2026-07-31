#!/usr/bin/env node

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(import.meta.dirname, "..");
const operations = resolve(root, "src/company-os/operations");
const fixturePath = resolve(root, "../../docs/design/company-os-v1/fixtures/company-os-trademark-v1.json");

let pass = 0;
let fail = 0;
function check(condition, message) {
  if (condition) { console.log(`  PASS  ${message}`); pass += 1; }
  else { console.error(`  FAIL  ${message}`); fail += 1; }
}

async function main() {
  const [pages, fixture, workOperatingPage, workProjectionSource, router, docsUrl] = await Promise.all([
    readFile(resolve(operations, "pages.tsx"), "utf8"),
    readFile(fixturePath, "utf8").then(JSON.parse),
    readFile(resolve(root, "src/company-os/work/WorkOperatingPage.tsx"), "utf8"),
    readFile(resolve(root, "src/company-os/work/projection.ts"), "utf8"),
    readFile(resolve(root, "src/company-os/CompanyOsRouter.tsx"), "utf8"),
    readFile(resolve(root, "src/company-os/docs/url.ts"), "utf8"),
  ]);
  const [components, fixtureAdapter, approvalAction, workItemAction] = await Promise.all([
    readFile(resolve(operations, "components.tsx"), "utf8"),
    readFile(resolve(operations, "fixture.ts"), "utf8"),
    readFile(resolve(operations, "approvalAction.ts"), "utf8"),
    readFile(resolve(operations, "workItemAction.ts"), "utf8"),
  ]);
  const types = await readFile(resolve(operations, "types.ts"), "utf8");
  const ts = (await import("typescript")).default;
  const adapterDirectory = await mkdtemp(resolve(tmpdir(), "company-os-operations-"));
  const adapterTarget = resolve(adapterDirectory, "fixture.mjs");
  const approvalActionTarget = resolve(adapterDirectory, "approvalAction.mjs");
  const workItemActionTarget = resolve(adapterDirectory, "workItemAction.mjs");
  const workProjectionTarget = resolve(adapterDirectory, "workProjection.mjs");
  await writeFile(adapterTarget, ts.transpileModule(fixtureAdapter, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  await writeFile(approvalActionTarget, ts.transpileModule(approvalAction, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  await writeFile(workItemActionTarget, ts.transpileModule(workItemAction, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  await writeFile(workProjectionTarget, ts.transpileModule(workProjectionSource, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  const adapterModule = await import(pathToFileURL(adapterTarget).href);
  const approvalActionModule = await import(pathToFileURL(approvalActionTarget).href);
  const workItemActionModule = await import(pathToFileURL(workItemActionTarget).href);
  const workProjectionModule = await import(pathToFileURL(workProjectionTarget).href);
  const required = ["OrganizationPage", "HumanMemberFocus", "StandingAgentFocus", "WorkboardPage", "WorkItemFocus", "ApprovalFocus", "FinancePage", "GovernanceProposalFocus", "BusinessModuleFocus"];
  check(required.every((name) => pages.includes(`function ${name}`)), "exports all nine Company OS operations pages");
  check(router.includes('<WorkOperatingPage source={resolved.value} />') && workOperatingPage.includes('data-work-operating-system="v1"'), "routes Work to the native multi-view operating workspace");
  check(["overview", "board", "all", "milestones", "timeline", "workload"].every((view) => workOperatingPage.includes(`id: "${view}"`)), "Work workspace exposes six projections over one WorkItem ledger");
  check(workOperatingPage.includes('useState<WorkView>("overview")') && workOperatingPage.includes("model.board.map") && !workOperatingPage.includes('const columns = ["submitted"'), "Work opens on overview and renders only Store aggregate board columns");
  check(workOperatingPage.includes('Object.prototype.hasOwnProperty.call(root, "work")') && workOperatingPage.includes("hasAggregate ? objects(projection.work_items)") && workOperatingPage.includes("hasAggregate ? objects(projection.milestones)"), "Work treats an explicit company_os.work aggregate as authoritative even when its lists are empty");
  check(['dimension("board")', "projectBusinessLineDimensions", 'dimension("work_types")', "objects(projection.workload)", "summaryValue"].every((contract) => workOperatingPage.includes(contract)), "Work consumes aggregate board, business-line, type, workload, and summary truth");
  const neutralAcceptanceStatuses = ["submitted", "in_progress", "in_review"];
  check(neutralAcceptanceStatuses.every((status) => {
    const presentation = workProjectionModule.acceptanceCriteriaPresentation(3, status);
    return presentation.label === "3 acceptance criteria"
      && presentation.semantic === "criteria_count"
      && presentation.tone === "neutral"
      && presentation.workItemStatus === status;
  })
    && workOperatingPage.includes("data-work-acceptance-semantic")
    && workOperatingPage.includes("text-muted-foreground")
    && !workOperatingPage.slice(workOperatingPage.indexOf("function AcceptanceCriteriaCount"), workOperatingPage.indexOf("function Board")).includes("text-status-good"),
  "submitted, in-progress, and in-review WorkItems render acceptance criteria as a neutral count rather than success evidence");
  check(workOperatingPage.includes('"No milestone"') && workOperatingPage.includes('"Unclassified"') && workOperatingPage.includes("Unassigned lane"), "Work views preserve missing Milestone, business-line, and assignment truth");
  check(workOperatingPage.includes("workItemHref(item.id)") && workOperatingPage.includes('aria-label={`Open WorkItem ${item.title}`}'), "Work overview and board cards deep-link to selected WorkItem truth");
  check(workOperatingPage.includes("No governed WorkItem creation transport is connected") && workOperatingPage.includes("opacity-60"), "unavailable Work creation looks and reads as disabled");
  check(["api", "project", "space", "company"].every((key) => docsUrl.includes(`"${key}"`)), "Company OS links preserve API, Project Binding, Execution Space, and Company Store context");
  check(types.includes('"human" | "standing_agent"') && types.includes("interface ActorSummary"), "keeps Human and Standing Agent as distinct actor kinds");
  check(pages.includes("Runtime attempts and private reasoning do not define membership or authority") && !pages.includes("MemberRunContext") && !pages.includes("TeamRunCompact"), "Standing Agent activity does not collapse into execution lifecycle or TeamRun state");
  check(pages.includes("Required human approver") && pages.includes("actorDescriptor(approval.requiredApprover)"), "renders the named Human approval boundary from the projection");
  check(components.includes("This is a pre-approval commitment, not a payment.") && fixtureAdapter.includes('type: "commitment"'), "renders ¥3,000 as a commitment and never invents a payment");
  check(components.includes('actor.availability === "available"'), "only renders availability indicator when explicitly provided");
  check(!pages.slice(pages.indexOf("function HumanMemberFocus"), pages.indexOf("function StandingAgentFocus")).match(/provider|runtime/i), "Human member page has no provider/runtime telemetry");
  check(pages.includes("module is awaiting final approval") && pages.includes("does not assert that it was created from an approved Module Design"), "keeps BusinessModule as a pending proposal");
  check(fixture.financial_records.length === 1 && fixture.financial_records[0].type === "commitment" && fixture.financial_records[0].amount === 3000, "fixture is the single ¥3,000 commitment source");
  check(fixture.negative_assertions.payment_financial_records.length === 0, "fixture confirms no payment record exists");
  check(components.includes("data-company-os-page") && components.includes("data-company-os-ready"), "exposes a deterministic Company OS capture marker");
  check(components.includes("data-actor-kind") && components.includes("data-actor-type") && components.includes("data-company-os-ref={actor.id}"), "visible actor pills expose canonical actor references and actor kinds");
  check(components.includes("data-financial-record-type") && components.includes("data-financial-status") && components.includes("data-company-os-ref={record.id}"), "visible financial cards expose canonical type, state and record references");
  check(components.includes("recordRef?: string") && components.includes("data-company-os-ref={recordRef}"), "visible linked records retain canonical source references");
  check(pages.includes("data-work-item-status") && pages.includes("data-company-os-ref={workItem.id}"), "workboard and WorkItem focus expose the actual WorkItem record");
  check(pages.includes("view.organization.rootUnitIds.map") && pages.includes("candidate.parentId === unit.id") && pages.includes("view.organization.memberships.filter"), "Organization renders the exact rooted OrgUnit forest and every unit membership");
  check(!pages.includes("Cross-unit Standing Agent capability roster") && !pages.includes("Primary operating unit"), "Organization never substitutes a flattened cross-unit roster for the hierarchy");
  check(pages.includes("view.evidence.map") && fixtureAdapter.includes("evidence_refs"), "approval and WorkItem surfaces expose both linked evidence records from the projection");
  check(pages.includes("view.businessModule.id") && pages.includes("view.governanceProposal.id"), "module and governance surfaces expose their actual linked records");
  check(pages.includes("view.julySpendMetric.id"), "finance overview exposes the shared July spend metric record");
  check(pages.includes("view.typedApplication.id"), "WorkItem focus exposes the linked typed application record");
  check(pages.includes("data-company-os-ref={view.workItem.id}"), "Human member focus exposes its visible accountable WorkItem");
  check(types.includes("TrademarkOperationsProjection") && fixtureAdapter.includes("adaptTrademarkOperationsProjection") && pages.includes("OperationsPageProps"), "all operations pages consume one adapted projection instead of module fixture constants");
  const snapshotProjection = structuredClone(fixture);
  snapshotProjection.actors.find((actor) => actor.id === "actor-human-brand-owner").display_name = "Snapshot Brand Owner";
  snapshotProjection.work_items[0].title = "Snapshot trademark filing";
  snapshotProjection.financial_records[0].display_amount = "¥4,200";
  const adapted = adapterModule.adaptTrademarkOperationsProjection(snapshotProjection);
  check(adapted.workItem.title === "Snapshot trademark filing" && adapted.commitment.amount === "¥4,200" && adapted.workItem.accountableOwner.name === "Snapshot Brand Owner", "adapter renders snapshot projection facts instead of static fixture values");
  const provenanceProjection = structuredClone(fixture);
  provenanceProjection.work_assignment_execution_chains = [{
    assignment_id: "assignment-provenance",
    work_item_id: provenanceProjection.work_items[0].id,
    assignment_state: "acknowledged",
    correlation_id: "corr-provenance",
    link_status: "linked",
    detail: "Exact durable chain.",
    handoffs: [{ id: "handoff-provenance", result: "completed", body: "RESULT: completed\nEvidence attached.", created_at: "2026-07-31T00:00:00Z", evidence_refs: ["evidence-provenance"] }],
    external_observations: [{
      id: "pr-provenance", kind: "pull_request", label: "PR #42",
      repository: "owner/repo", pull_request_number: "42", head_ref: "codex/fix",
      head_sha: "abc123", base_ref: "master", url: "https://example.test/pr/42",
      state: "open", observed_at: "2026-07-31T00:00:00Z",
      source_updated_at: "2026-07-30T23:00:00Z", source_completed_at: "2026-07-31T00:30:00Z",
      freshness: "fresh",
    }],
  }];
  const provenance = adapterModule.adaptTrademarkOperationsProjection(provenanceProjection);
  const provenanceChain = provenance.workAssignmentExecutionChains[0];
  check(
    provenanceChain.handoffs[0].result === "completed"
      && provenanceChain.handoffs[0].body.includes("Evidence attached")
      && provenanceChain.handoffs[0].evidenceRefs[0] === "evidence-provenance",
    "adapter preserves visible Handoff result, body, and evidence refs",
  );
  check(
    provenanceChain.externalObservations[0].repository === "owner/repo"
      && provenanceChain.externalObservations[0].pullRequestNumber === "42"
      && provenanceChain.externalObservations[0].headRef === "codex/fix"
      && provenanceChain.externalObservations[0].baseRef === "master"
      && provenanceChain.externalObservations[0].url === "https://example.test/pr/42"
      && provenanceChain.externalObservations[0].observedAt === "2026-07-31T00:00:00Z"
      && provenanceChain.externalObservations[0].sourceUpdatedAt === "2026-07-30T23:00:00Z"
      && provenanceChain.externalObservations[0].sourceCompletedAt === "2026-07-31T00:30:00Z",
    "adapter preserves stable repository, PR, branch, URL, observation, and source timestamps",
  );
  const internalCommandProjection = structuredClone(fixture);
  internalCommandProjection.approvals[0].title = "Authorize commitment.append to enter the trademark fee into Human review";
  internalCommandProjection.approvals[0].action_summary = "Authorize commitment.append; legal submission remains blocked.";
  const internalCommandAdapted = adapterModule.adaptTrademarkOperationsProjection(internalCommandProjection);
  check(!internalCommandAdapted.approval.title.includes("commitment.append") && !internalCommandAdapted.approval.actionSummary.includes("commitment.append"), "adapter keeps internal command names out of approval business copy");
  const emptyAuthoritativeProjection = adapterModule.adaptTrademarkOperationsProjection({});
  const emptyTruth = JSON.stringify(emptyAuthoritativeProjection);
  check(!emptyTruth.includes("CN-2026-018") && !emptyTruth.includes("¥3,000") && !emptyTruth.includes("Brand Owner"), "an explicit empty authoritative projection never falls back to prototype trademark facts");
  const aggregateProjection = structuredClone(fixture);
  const aggregateWorkItem = {
    ...structuredClone(fixture.work_items[0]),
    id: "work-aggregate-selected",
    title: "Aggregate selected detail",
    status: "in_review",
    work_type: "audit",
    business_module_ref: "module-trademark-management",
    milestone_ref: "milestone-aggregate",
  };
  aggregateProjection.work = {
    query: {},
    summary: {
      total: 1,
      active: 7,
      completed: 3,
      blocked: 2,
      waiting_for_approval: 4,
      unassigned: 5,
      without_milestone: 6,
      without_business_line: 8,
    },
    work_items: [aggregateWorkItem],
    milestones: [{
      milestone: {
        id: "milestone-aggregate",
        title: "Aggregate milestone",
        status: "active",
      },
      total_work_items: 9,
      completed_work_items: 2,
      blocked_work_items: 1,
      waiting_for_approval_work_items: 3,
      progress_percent: 22,
    }],
    board: { review_queue: ["work-aggregate-selected"] },
    business_lines: { "module-trademark-management": ["work-aggregate-selected"] },
    work_types: { audit: ["work-aggregate-selected"] },
    workload: [{
      actor: { actor_type: "agent", actor_id: "actor-agent-document-architecture" },
      accountable_count: 11,
      assigned_count: 12,
      active_count: 13,
      work_item_refs: ["work-aggregate-selected"],
    }],
  };
  const aggregateAdapted = adapterModule.adaptTrademarkOperationsProjection(aggregateProjection, { workItemId: "work-aggregate-selected" });
  check(
    aggregateAdapted.work.provenance === "company_os.work"
      && aggregateAdapted.work.summary.active === 7
      && aggregateAdapted.work.board.review_queue.join(",") === "work-aggregate-selected"
      && aggregateAdapted.work.workTypes.audit.join(",") === "work-aggregate-selected"
      && aggregateAdapted.work.workload[0].activeCount === 13
      && aggregateAdapted.work.milestones[0].progressPercent === 22,
    "shared adapter preserves every company_os.work aggregate dimension without recomputing supplied counts",
  );
  check(aggregateAdapted.workItem.id === "work-aggregate-selected"
    && aggregateAdapted.work.selection.status === "resolved",
  "selected WorkItem detail resolves only by its explicit id inside company_os.work");
  const mismatchedBusinessLines = workProjectionModule.projectBusinessLineDimensions(
    { "module-a": ["work-b"] },
    [{ id: "work-b", businessLineId: "module-b" }],
    new Map([["module-a", "Module A"], ["module-b", "Module B"]]),
  );
  check(mismatchedBusinessLines.dimensions[0]?.id === "module-a"
    && mismatchedBusinessLines.dimensions[0]?.label === "Module A"
    && mismatchedBusinessLines.dimensions[0]?.workItemIds.join(",") === "work-b"
    && mismatchedBusinessLines.integrityFindings.length === 1
    && mismatchedBusinessLines.integrityFindings[0].includes("business_module_ref is module-b")
    && !workOperatingPage.includes("linked[0]?.businessLine"),
  "business-line dimensions retain their exact aggregate/module identity and flag module-a=[work-b/module-b] mismatches");
  const missingSelected = adapterModule.adaptTrademarkOperationsProjection(aggregateProjection, { workItemId: "work-not-present" });
  check(missingSelected.work.selection.status === "not_found"
    && missingSelected.workItem.id === "work-not-present"
    && missingSelected.workItem.title === "WorkItem not found",
  "missing selected WorkItem fails closed without falling back to the first aggregate row");
  const emptyAggregateProjection = structuredClone(fixture);
  emptyAggregateProjection.work = {
    query: {},
    summary: { total: 0, active: 0, completed: 0, blocked: 0, waiting_for_approval: 0, unassigned: 0, without_milestone: 0, without_business_line: 0 },
    work_items: [],
    milestones: [],
    board: {},
    business_lines: {},
    work_types: {},
    workload: [],
  };
  const emptyAggregate = adapterModule.adaptTrademarkOperationsProjection(emptyAggregateProjection, { workItemId: fixture.work_items[0].id });
  check(emptyAggregate.workItems.length === 0
    && emptyAggregate.work.selection.status === "empty"
    && emptyAggregate.work.summary.total === 0,
  "an explicit empty company_os.work aggregate never falls back to populated raw work_items");
  check(!pages.includes('value="1" detail="From current projection"')
    && pages.includes("view.work.summary.active")
    && pages.includes('view.work.selection.status !== "resolved"'),
  "operations surfaces remove the hardcoded open count and first-row detail fallback");
  const canonicalProjection = adapterModule.adaptTrademarkOperationsProjection(fixture);
  const agentosOrganizationProjection = structuredClone(fixture);
  agentosOrganizationProjection.organization.org_units = [
    {
      id: "orgunit-agentos-root",
      name: "AgentOS",
      parent_unit_id: null,
      human_lead_actor_ref: { actor_type: "human", actor_id: "actor-human-brand-owner" },
    },
    {
      id: "orgunit-agentos-governance",
      name: "Any label",
      parent_unit_id: "orgunit-agentos-root",
      agent_lead_actor_ref: { actor_type: "agent", actor_id: "actor-agent-document-architecture" },
    },
    {
      id: "orgunit-agentos-child",
      name: "Nested child",
      parent_unit_id: "orgunit-agentos-governance",
    },
    {
      id: "orgunit-independent-root",
      name: "Independent root",
      parent_unit_id: null,
    },
  ];
  agentosOrganizationProjection.organization.memberships = [
    { id: "membership-owner", actor_ref: { actor_type: "human", actor_id: "actor-human-brand-owner" }, org_unit_id: "orgunit-agentos-root", membership_role: "lead" },
    { id: "membership-docs-lead", actor_ref: { actor_type: "agent", actor_id: "actor-agent-document-architecture" }, org_unit_id: "orgunit-agentos-governance", membership_role: "lead", authority_policy_refs: ["policy-membership-docs"] },
    { id: "membership-docs-child", actor_ref: { actor_type: "agent", actor_id: "actor-agent-document-architecture" }, org_unit_id: "orgunit-agentos-child", membership_role: "advisor" },
    { id: "membership-org", actor_ref: { actor_type: "agent", actor_id: "actor-agent-organization-governance" }, org_unit_id: "orgunit-agentos-governance", membership_role: "member" },
    { id: "membership-strategy", actor_ref: { actor_type: "agent", actor_id: "actor-agent-content-strategy" }, org_unit_id: "orgunit-agentos-child", membership_role: "member" },
  ];
  const agentosOrganization = adapterModule.adaptTrademarkOperationsProjection(agentosOrganizationProjection);
  check(
    agentosOrganization.organization.rootUnitIds.join(",") === "orgunit-agentos-root,orgunit-independent-root"
      && agentosOrganization.organization.units.find((unit) => unit.id === "orgunit-agentos-child")?.parentId === "orgunit-agentos-governance"
      && agentosOrganization.organization.units.find((unit) => unit.id === "orgunit-agentos-governance")?.agentLeadActorId === "actor-agent-document-architecture",
    "organization projection preserves every explicit root, parent_unit_id edge, and Agent lead without label or first-row selection",
  );
  check(agentosOrganization.organization.memberships.filter((membership) => membership.actorId === "actor-agent-document-architecture").length === 2
    && agentosOrganization.organization.units.find((unit) => unit.id === "orgunit-agentos-governance")?.actorIds.includes("actor-agent-document-architecture")
    && agentosOrganization.organization.units.find((unit) => unit.id === "orgunit-agentos-child")?.actorIds.includes("actor-agent-document-architecture")
    && agentosOrganization.organization.memberships.find((membership) => membership.id === "membership-docs-lead")?.authorityPolicyRefs.includes("policy-membership-docs"),
  "organization projection preserves all memberships and declared membership policies when one actor belongs to multiple units");
  check(!fixtureAdapter.includes("membershipAgentLead")
    && !fixtureAdapter.includes("legacyRoleAgentLead")
    && !fixtureAdapter.includes("primaryOperatingUnit")
    && !pages.includes('/lead/i.test')
    && !pages.includes("actorList[0]"),
  "Organization contains no membership-role, label, name, order, or first-actor lead/root heuristic");
  const brokenOrganizationProjection = structuredClone(fixture);
  brokenOrganizationProjection.organization.org_units = [
    { id: "root", name: "Root", parent_unit_id: null, human_lead_actor_ref: { actor_type: "human", actor_id: "actor-agent-finance" } },
    { id: "orphan", name: "Orphan", parent_unit_id: "missing-parent" },
    { id: "cycle-a", name: "Cycle A", parent_unit_id: "cycle-b" },
    { id: "cycle-b", name: "Cycle B", parent_unit_id: "cycle-a" },
  ];
  brokenOrganizationProjection.organization.memberships = [
    { id: "duplicate-a", actor_ref: { actor_type: "agent", actor_id: "actor-agent-finance" }, org_unit_id: "root", membership_role: "member" },
    { id: "duplicate-b", actor_ref: { actor_type: "agent", actor_id: "actor-agent-finance" }, org_unit_id: "root", membership_role: "advisor" },
    { id: "missing-unit", actor_ref: { actor_type: "agent", actor_id: "actor-agent-trademark" }, org_unit_id: "absent-unit", membership_role: "member" },
    { id: "missing-actor", actor_ref: { actor_type: "agent", actor_id: "actor-missing" }, org_unit_id: "root", membership_role: "member" },
  ];
  const brokenOrganization = adapterModule.adaptTrademarkOperationsProjection(brokenOrganizationProjection);
  const findingKinds = new Set(brokenOrganization.organization.integrityFindings.map((finding) => finding.kind));
  check(["orphan_parent", "parent_cycle", "unknown_membership_unit", "unknown_membership_actor", "duplicate_membership", "invalid_human_lead"].every((kind) => findingKinds.has(kind))
    && brokenOrganization.organization.unplacedUnitIds.includes("orphan")
    && brokenOrganization.organization.unplacedUnitIds.includes("cycle-a"),
  "organization adapter deterministically surfaces orphan, cycle, membership, duplicate, and explicit-lead integrity failures");
  check(pages.includes("data-organization-integrity-count")
    && pages.includes("data-org-parent-unit-id")
    && pages.includes("AgentMember / runtime binding")
    && pages.includes("standingExecutionAssignmentsForActor(actor, view.standingAssignments)"),
  "Organization renders integrity evidence and exact StandingAgent → AgentMember → MemberRun/runtime bindings");
  const nonFinancialProjection = structuredClone(fixture);
  nonFinancialProjection.work_items.push({
    id: "work-agentos-loop",
    title: "Run AgentOS self-hosting loop",
    objective: "Operate Docs, Work, and Org through native truth.",
    description: "No monetary effect exists.",
    acceptance_criteria: ["Selected page contains no borrowed finance relation"],
    context_refs: [{ kind: "document", id: "document-company-operating-manual" }],
    deliverable_refs: [],
    status: "submitted",
    source_document_ref: "document-company-operating-manual",
    source_record_refs: [],
    result_document_ref: null,
    result_record_refs: [],
    submitted_by: { actor_type: "agent", actor_id: "actor-agent-document-architecture" },
    requested_by: null,
    accountable_owner: { actor_type: "agent", actor_id: "actor-agent-document-architecture" },
    assignees: [{ actor_type: "agent", actor_id: "actor-agent-document-architecture" }],
    contributors: [],
    reviewer: { actor_type: "agent", actor_id: "actor-agent-organization-governance" },
    approver: null,
    execution_mode: "direct",
    execution_refs: [],
    approval_refs: [],
    evidence_refs: [],
    artifact_refs: [],
    outcome_summary: null,
    due_at: null,
    priority: "high",
    risk_level: null,
    created_at: "2026-07-30T10:00:00+08:00",
    updated_at: "2026-07-30T10:00:00+08:00",
    completed_at: null,
  });
  const selectedNonFinancial = adapterModule.adaptTrademarkOperationsProjection(nonFinancialProjection, { workItemId: "work-agentos-loop" });
  check(selectedNonFinancial.workItem.status === "submitted" && selectedNonFinancial.workItem.requestedBy === undefined, "selected WorkItem preserves submitted state and absent requester without fabricating in-progress or unresolved identity");
  check(selectedNonFinancial.linkedApproval === undefined && selectedNonFinancial.linkedCommitment === undefined && selectedNonFinancial.linkedTypedRecords.length === 0, "selected non-financial WorkItem does not borrow unrelated Approval, Commitment, or TypedRecord rows");
  check(pages.includes("No Approval or Finance record is linked to this WorkItem") && pages.includes("linkedApproval || linkedCommitment"), "WorkItem focus renders absent governed relations honestly");
  const linkedExecutionProjection = structuredClone(fixture);
  const linkedActor = linkedExecutionProjection.actors.find((actor) => actor.id === "actor-agent-trademark");
  linkedActor.execution_agent_member_ref = "execution-agent-trademark";
  linkedActor.availability = "busy";
  linkedActor.assignment_capacity = 2;
  linkedActor.capability_refs = ["capability-trademark-drafting"];
  linkedActor.permission_policy_refs = ["policy-trademark-work"];
  linkedActor.runtime_refs = ["declared-runtime-locator"];
  linkedActor.native_session_refs = ["declared-session-locator"];
  linkedExecutionProjection.standing_assignments = [{
    id: "standing-assignment-member-build-corr-build",
    agent_member_id: "execution-agent-trademark",
    source_kind: "agent_team_assignment",
    source_ref: "message-build",
    mission_id: "mission-build",
    wave_id: null,
    team_run_id: "team-run-build",
    member_run_id: "member-run-build",
    title: "Implement the linked Organization slice",
    role: "builder",
    status: "idle",
    assigned_at: "2026-07-20T09:15:00+08:00",
    last_activity_at: "2026-07-20T09:20:00+08:00",
    correlation_id: "corr-build",
    native_session: { provider: "codex", native_session_id: "thread-build", availability: "available" },
    navigation_target: "?surface=team&team=team-run-build&memberRun=member-run-build",
  }];
  const linkedExecution = adapterModule.adaptTrademarkOperationsProjection(linkedExecutionProjection);
  check(linkedExecution.actors["actor-agent-trademark"]?.executionAgentMemberRef === "execution-agent-trademark"
    && linkedExecution.standingAssignments?.length === 1
    && linkedExecution.standingAssignments[0].memberRunId === "member-run-build"
    && linkedExecution.standingAssignments[0].agentMemberId === "execution-agent-trademark",
  "adapter preserves the explicit Company-owned StandingAgent-to-AgentMember link");
  const linkedActorView = linkedExecution.actors["actor-agent-trademark"];
  check(linkedActorView?.availability === "busy"
    && linkedActorView.assignmentCapacity === 2
    && linkedActorView.capabilityRefs.includes("capability-trademark-drafting")
    && linkedActorView.permissionPolicyRefs.includes("policy-trademark-work")
    && linkedActorView.runtimeRefs.includes("declared-runtime-locator")
    && linkedActorView.nativeSessionRefs.includes("declared-session-locator"),
  "adapter preserves declared Organization availability, capacity, permission, capability, and locator configuration without treating it as runtime truth");
  const exactLinkedAssignments = adapterModule.standingExecutionAssignmentsForActor(linkedActorView, linkedExecution.standingAssignments);
  const mismatchActor = { ...linkedActorView, executionAgentMemberRef: "execution-agent-other", name: "Trademark Agent", role: "builder" };
  check(exactLinkedAssignments.length === 1
    && exactLinkedAssignments[0].memberRunId === "member-run-build"
    && adapterModule.standingExecutionAssignmentsForActor(mismatchActor, linkedExecution.standingAssignments).length === 0,
  "execution resolver accepts the exact AgentMember id and rejects name, role, provider, session, and actor-id similarities");
  check(pages.includes("actor.executionAgentMemberRef") && !pages.includes("assignment.agentMemberId === actor.id"),
    "Standing Agent focus never binds execution by same-string actor id");
  linkedExecutionProjection.organization.effective_delegated_authority = { status: "active", grant_refs: ["fabricated-grant"] };
  const unsupportedAuthority = adapterModule.adaptTrademarkOperationsProjection(linkedExecutionProjection);
  check(unsupportedAuthority.organization.effectiveDelegatedAuthority.status === "not_projected"
    && unsupportedAuthority.organization.effectiveDelegatedAuthority.grantRefs.length === 0
    && unsupportedAuthority.organization.effectiveDelegatedAuthority.detail.includes("does not project evaluated scoped grants"),
  "adapter does not promote an unsupported input field or declared policy/capability refs into effective delegated authority");
  check(pages.includes("workItem.reviewer?.id === actor.id")
    && pages.includes("activeRelatedItems")
    && pages.includes('return "Reviewer"'),
  "Standing Agent focus includes active review responsibility instead of hiding review work");
  check((canonicalProjection.standingAssignmentConflicts ?? []).length === 0, "a healthy snapshot with no standing_assignment_conflicts adapts to an empty list");
  const conflictProjection = structuredClone(fixture);
  conflictProjection.standing_assignment_conflicts = [
    {
      id: "standing-link-conflict:member-shared",
      kind: "duplicate_execution_agent_member_ref",
      severity: "error",
      agent_member_id: "member-shared",
      standing_agent_ids: ["standing-dup-a", "standing-dup-b"],
      affected_member_run_ids: ["run-shared"],
      detail: "duplicate StandingAgent execution_agent_member_ref member-shared: standing-dup-a, standing-dup-b; relation must be one-to-one",
      resolution_hint: "harness company org actor unlink-execution --authority <human-id> --actor <one of: standing-dup-a, standing-dup-b>",
    },
    { id: "", kind: "duplicate_execution_agent_member_ref", severity: "error", agent_member_id: "member-missing-id", standing_agent_ids: ["standing-x"] },
    { id: "standing-link-conflict:no-agents", kind: "duplicate_execution_agent_member_ref", severity: "error", agent_member_id: "member-orphan", standing_agent_ids: [] },
  ];
  const conflictAdapted = adapterModule.adaptTrademarkOperationsProjection(conflictProjection);
  check(conflictAdapted.standingAssignmentConflicts?.length === 1
    && conflictAdapted.standingAssignmentConflicts[0].agentMemberId === "member-shared"
    && conflictAdapted.standingAssignmentConflicts[0].standingAgentIds.join(",") === "standing-dup-a,standing-dup-b"
    && conflictAdapted.standingAssignmentConflicts[0].affectedMemberRunIds.join(",") === "run-shared"
    && conflictAdapted.standingAssignmentConflicts[0].resolutionHint?.includes("unlink-execution"),
  "adapter parses a duplicate execution_agent_member_ref conflict naming both competing Standing Agents and drops incomplete conflict records");
  const manyConflictsProjection = structuredClone(fixture);
  manyConflictsProjection.standing_assignment_conflicts = Array.from({ length: 8 }, (_unused, index) => ({
    id: `standing-link-conflict:member-${index}`,
    kind: "duplicate_execution_agent_member_ref",
    severity: "error",
    agent_member_id: `member-${index}`,
    standing_agent_ids: [`standing-${index}-a`, `standing-${index}-b`],
    affected_member_run_ids: [`run-${index}`],
    detail: `duplicate link ${index}`,
    resolution_hint: `resolve ${index}`,
  }));
  const manyConflictsAdapted = adapterModule.adaptTrademarkOperationsProjection(manyConflictsProjection);
  check(manyConflictsAdapted.standingAssignmentConflicts?.length === 8, "adapter preserves every conflict record; bounding rendered entries is a page concern, not a data-loss concern");
  check(pages.includes("const STANDING_LINK_CONFLICT_VISIBLE_CAP = 5;")
    && pages.includes("conflicts.slice(0, STANDING_LINK_CONFLICT_VISIBLE_CAP)")
    && pages.includes("{hiddenCount} more"),
  "Standing Agent link conflict banner caps rendered entries at 5 and shows a '+N more' indicator instead of rendering every conflict");
  check(pages.includes("if (conflicts.length === 0) return null;"), "Standing Agent link conflict banner renders nothing extra on a healthy, conflict-free snapshot");
  check(pages.includes("<StandingLinkConflictBanner conflicts={standingLinkConflicts} />") && pages.includes("<StandingLinkConflictBanner conflicts={actorLinkConflicts} />"),
    "both the Organization overview and the Standing Agent focus surface the withheld-participation conflict banner");
  const brandUnit = canonicalProjection.organization.units.find((unit) => unit.id === "org-brand-ip");
  check(brandUnit?.actorIds.length === 4 && canonicalProjection.governanceProposal.proposedById === "actor-agent-document-architecture", "adapter retains the actual Brand & IP membership branch and governance proposal author");
  check(brandUnit?.agentLeadActorId === undefined && pages.includes("unit.agentLeadActorId === actor.id") && !pages.includes("candidate.id !== actor.id).slice(0, 4)"), "membership role and actor naming cannot invent an Agent lead when OrgUnit.agent_lead_actor_ref is absent");
  check(canonicalProjection.actors["actor-agent-document-architecture"]?.availability === "available" && !canonicalProjection.actors["actor-agent-finance"]?.availability, "explicit Organization availability is preserved while a null value remains absent rather than inferred from runtime or membership");
  check(pages.includes("OrganizationUnitBranch") && pages.includes("ExplicitUnitLeads") && pages.includes("All memberships"), "organization surface is a recursive forest with explicit leads and complete membership branches");
  check(pages.includes("data-organization-membership-grid")
    && pages.includes("repeat(auto-fit,minmax(min(100%,18rem),1fr))")
    && pages.includes("data-org-depth={depth}")
    && pages.includes("depth={depth + 1}")
    && pages.includes('depth === 1')
    && pages.includes('"border-l-0 pl-0"'),
  "nested Organization branches use container-aware membership grids and cap recursive indentation without flattening");
  check(pages.includes("membership.id}</code>")
    && pages.includes("assignment.memberRunId}</code>")
    && pages.includes("break-all font-mono")
    && !pages.slice(pages.indexOf("function OrgActorCard"), pages.indexOf("function OrgActorCardBody")).includes("truncate"),
  "Organization membership, AgentMember, and MemberRun canonical ids wrap instead of truncating");
  check(pages.includes("Propose agent") && pages.includes("Create org unit") && pages.includes("disabled"), "organization actions are visibly disabled until a governed action path exists");
  check(pages.includes("unplacedUnitIds") && pages.includes("Actors without OrganizationMembership"), "unplaced units and unassigned actors remain visible as integrity state rather than entering the primary forest");
  check(pages.includes("<PageFrame dense") && components.includes('dense ? "py-5" : "py-8"') && components.includes('dense ? "mb-4 pb-4" : "mb-7 pb-6"'), "Organization opts into compact vertical rhythm without changing the default page frame");
  check(pages.includes("<LinkedRecord wrapLabel") && components.includes("wrapLabel ? \"whitespace-normal leading-5\" : \"truncate\""), "governance proposal title is allowed to wrap instead of truncating in the context rail");
  check(pages.includes("BoardFact label=\"Requested by\"") && pages.includes("BoardFact label=\"Submitted by\"") && pages.includes("actor={workItem.submittedBy}"), "workboard keeps requester and submitter visible as distinct full actor facts");
  check(pages.indexOf('Panel title="Durable Work truth"') < pages.indexOf('Panel title="Responsibility"') && pages.includes("approvalTitle") && pages.includes("break-words text-sm leading-6"), "WorkItem focus moves durable evidence into the first viewport and wraps a human-readable approval summary");
  check(types.includes("interface WorkAssignmentExecutionChain") && fixtureAdapter.includes("root.work_assignment_execution_chains"), "adapter exposes the explicit Company Assignment execution-link projection");
  check(pages.includes('Panel title="Computed execution & delivery evidence"') && pages.includes("data-observation-freshness") && pages.includes("data-handoff-result") && pages.includes("handoff.body") && pages.includes("handoff.evidenceRefs.map") && pages.includes("observation.repository") && pages.includes("observation.pullRequestNumber") && pages.includes("observation.baseRef") && pages.includes("observation.url") && pages.includes("observation.observedAt") && pages.includes("observation.sourceUpdatedAt") && pages.includes("observation.sourceCompletedAt") && pages.includes("do not accept or transition this WorkItem"), "WorkItem focus renders complete Handoff and external observation evidence separately from durable Company acceptance");
  check(pages.includes("FinanceRecordTable") && ["Record type", "Amount", "Cost context", "Source", "Approval status"].every((label) => pages.includes(`\"${label}\"`)) && !pages.includes("\"Project\""), "finance renders auditable record fields without reintroducing a Project object");
  check(pages.includes("data-standing-agent-workspace") && pages.includes('mainLabel="Standing Agent work and activity"') && pages.includes("<ActivityStream") && !pages.includes('kind: "thinking"'), "Standing Agent focus has a central projection-backed work/activity surface without thinking persistence");
  check(types.includes("interface StandingExecutionAssignment") && fixtureAdapter.includes("root.standing_assignments"), "Company OS adapter exposes the explicit standing Agent Team assignment projection");
  check(pages.includes("MemberRun explicitly links this durable identity") && pages.includes("assignment.memberRunId") && pages.includes("assignment.correlationId"), "Standing Agent focus shows exact MemberRun/correlation evidence and an honest unlinked empty state");
  check(pages.includes('surface: "team"') && pages.includes("teamId: assignment.teamRunId") && pages.includes("memberRunId: assignment.memberRunId"), "Standing Agent participation deep-links to the native Team/Member surface");
  check(pages.includes('!["completed", "failed", "stopped"].includes(assignment.status)') && pages.includes("Completed participation remains in Activity"), "terminal MemberRuns remain historical activity instead of active Standing Agent work");
  check(pages.includes('assignment.sourceKind === "agent_team_assignment"')
    && pages.includes("Boolean(assignment.correlationId)")
    && pages.includes("assignment.correlationId!"),
  "assignment-less Agent Team participation stays out of current work and current cards require a real correlation");
  check(pages.includes("selectionForActor") && pages.includes("onOpen={selectionForActor") && router.includes("onSelectionChange={onSelectionChange}"), "Organization actor cards route through the shared selection state");
  check(pages.includes("authoredProposal") && pages.includes("proposal-${authoredProposal.id}") && pages.includes('title="Maintained Docs"'), "Standing Agent distinguishes authored proposal activity from related durable Docs");
  check(pages.includes('title="Prompt, tools & skills"')
    && pages.includes('title="Declared permission configuration"')
    && pages.includes('title="Effective delegated authority"')
    && pages.includes('title="Runtime availability"')
    && pages.includes('title="Implemented vs target"')
    && pages.includes('data-effective-authority-status')
    && !pages.includes('label="Reports to"')
    && !pages.includes("Direct reports"),
  "Standing Agent separates declared configuration, effective-authority absence, and runtime availability without inventing ReportingRelation");
  check(pages.includes("view.workItems ?? [view.workItem]") && pages.includes("view.assignments ?? []"), "Standing Agent workspace consumes all projected WorkItems and native Assignments");
  check(pages.includes("textarea") && pages.includes("standing-agent-message-reason") && pages.includes("Send message. Unavailable"), "Standing Agent composer is visibly disabled with a governed transport reason");
  check(pages.includes("displayTimestamp(workItem.updatedAt)") && pages.includes("function displayTimestamp"), "WorkItem focus renders raw update timestamps in a human-readable form");
  check(pages.includes('data-handoff-body-disclosure="collapsed-default"') && pages.includes('aria-label={`Full Handoff body ${handoff.id}`}') && pages.includes("<Markdown source={handoff.body} compact />") && !pages.includes('whitespace-pre-wrap text-muted-foreground">{handoff.body}'), "WorkItem focus keeps Handoff metadata visible while full bodies use collapsed sanitized Markdown");
  check(pages.includes('return <div className="h-full min-h-0 overflow-hidden" data-company-os-ref={workItem.id} data-work-item-status={workItem.status}><PageFrame'), "WorkItem focus propagates the bounded route height to the PageFrame scroll owner");
  check(pages.includes('Panel title="Impact surfaces"') && pages.includes('Panel title="Governed actions"') && pages.includes("Approve proposal") && pages.includes("Request changes"), "governance proposal shows impacts, proposed structure, and honestly disabled governed actions");
  check(pages.includes('decide("approved")') && pages.includes('GovernedActionButton label="Request changes"') && pages.includes('decide("rejected")'), "approval focus has explicit governed approve/reject controls and an honest request-changes boundary");
  check(pages.includes("action={decisionControls}") && pages.includes('aria-label="Approval decision controls"'), "approval decision controls stay in the first-viewport page header");
  check(pages.includes("data-actor-kind={kind}") && pages.includes("data-actor-type={kind}") && pages.includes("BoardFact label=\"Reviewer\""), "workboard actor facts preserve canonical actor references and kinds for capture evidence");
  check(pages.includes("data-financial-record-type={record.type}") && pages.includes("data-financial-status={record.status}") && pages.includes("FinanceRecordTable"), "finance audit table preserves commitment reference, type, and state evidence");
  check(pages.includes('ImpactSurface label="Financial commitment" financialRecord={view.commitment}') && pages.includes("<FinancialRecordCard record={financialRecord} />"), "governance financial impact preserves the linked commitment semantic marker");
  check(fixtureAdapter.includes("approvalPresentation") && fixtureAdapter.includes("financialBusinessLabel") && !fixtureAdapter.includes("title: text(approvalRecord.title"), "approval and finance presentation remove internal command names from primary business copy");
  check(fixtureAdapter.includes("humanizeEvidenceLabel") && fixtureAdapter.includes('return "Lawyer review"'), "raw evidence references receive readable evidence labels");
  check(!canonicalProjection.approval.title.includes("commitment.append") && !canonicalProjection.approval.actionSummary.includes("commitment.append"), "canonical approval copy never exposes the internal commitment command");
  check(canonicalProjection.evidence.every((item) => !item.label.startsWith("evidence-")), "canonical evidence copy never exposes raw evidence ids as labels");
  const governedProjection = structuredClone(fixture);
  governedProjection.approvals[0] = {
    id: "approval-browser-test",
    subject_ref: { kind: "financial_record", id: fixture.financial_records[0].id },
    action_summary: "Authorize commitment.append for browser test",
    requested_by: { actor_type: "agent", actor_id: "actor-agent-trademark" },
    required_approver_refs: [{ actor_type: "human", actor_id: "actor-human-brand-owner" }],
    required_actor_type: "human",
    policy_ref: "page-trademark:commitment.append",
    status: "requested",
    decided_by: [],
    decision_note: null,
    evidence_refs: ["evidence-trademark-filing-package-cn-2026-018"],
    requested_at: "2026-07-20T09:00:00+08:00",
    decided_at: null,
    expires_at: "2026-07-31T18:00:00+08:00",
  };
  governedProjection.work_items[0].approval_refs = ["approval-browser-test"];
  governedProjection.work_items[0].accountable_owner = { actor_type: "human", actor_id: "actor-human-brand-owner" };
  governedProjection.work_items[0].assignees = [{ actor_type: "agent", actor_id: "actor-agent-trademark" }];
  governedProjection.work_items[0].reviewer = { actor_type: "agent", actor_id: "actor-agent-finance" };
  governedProjection.custom_page_definitions = [{
    id: "page-trademark",
    action_command_refs: ["approval.decide", "work_item.transition"],
    policy_refs: ["page-trademark:approval.decide", "page-trademark:work_item.transition"],
  }];
  const governed = adapterModule.adaptTrademarkOperationsProjection(governedProjection);
  const command = approvalActionModule.buildApprovalDecisionCommand({ approval: governed.approval, decision: "approved", note: "Approved in browser acceptance", commandId: "action-browser-test", decidedAt: "2026-07-20T10:00:00+08:00" });
  check(command.command_name === "approval.decide" && command.requested_by.actor_type === "human" && command.requested_by.actor_id === "actor-human-brand-owner", "browser decision command uses the named Human approver and canonical server command");
  check(command.policy_ref === "page-trademark:approval.decide" && command.payload.record.policy_ref === "page-trademark:commitment.append" && command.payload.record.status === "approved", "decision command keeps Action policy separate from the Approval's governed subject policy");
  check(command.subject_ref.kind === "approval" && command.payload.record.subject_ref.kind === "financial_record", "Action subject is the Approval while the Approval record preserves its governed financial subject");
  check(command.requires_human_approval === false && command.risk_tier === "r2" && command.approval_refs.length === 0, "approval.decide does not recursively require a second Approval");
  check(pages.includes("data-company-os-action-token") && approvalAction.includes("A durable decision note is required") && pages.includes("Request changes needs a separate native Approval status"), "Approval Focus exposes a session-only capability, durable note, and honest request-changes boundary");
  const workCommand = workItemActionModule.buildWorkItemTransitionCommand({ workItem: governed.workItem, targetStatus: "in_progress", note: "Preparation started", commandId: "action-work-browser-test", transitionedAt: "2026-07-20T10:05:00+08:00" });
  check(workCommand.command_name === "work_item.transition" && workCommand.subject_ref.kind === "work_item" && workCommand.requested_by.actor_id === "actor-agent-trademark", "WorkItem browser command attributes execution to the explicit assignee");
  check(workCommand.required_permission === "company.work.execute" && workCommand.risk_tier === "r2" && workCommand.payload.record.status === "in_progress", "WorkItem transition uses the declared lifecycle policy and complete next record");
  check(pages.includes('aria-label="WorkItem transition controls"') && pages.includes("Every linked Approval must be approved before completion") && workItemAction.includes("A durable transition note is required"), "WorkItem Focus exposes governed lifecycle controls and the explicit Approval completion gate");
  await rm(adapterDirectory, { recursive: true, force: true });
  console.log(`\nCompany OS operations checks: ${pass} pass, ${fail} fail`);
  process.exit(fail === 0 ? 0 : 1);
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
