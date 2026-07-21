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
  const [pages, fixture, workOperatingPage, router] = await Promise.all([
    readFile(resolve(operations, "pages.tsx"), "utf8"),
    readFile(fixturePath, "utf8").then(JSON.parse),
    readFile(resolve(root, "src/company-os/work/WorkOperatingPage.tsx"), "utf8"),
    readFile(resolve(root, "src/company-os/CompanyOsRouter.tsx"), "utf8"),
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
  await writeFile(adapterTarget, ts.transpileModule(fixtureAdapter, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  await writeFile(approvalActionTarget, ts.transpileModule(approvalAction, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  await writeFile(workItemActionTarget, ts.transpileModule(workItemAction, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  const adapterModule = await import(pathToFileURL(adapterTarget).href);
  const approvalActionModule = await import(pathToFileURL(approvalActionTarget).href);
  const workItemActionModule = await import(pathToFileURL(workItemActionTarget).href);
  const required = ["OrganizationPage", "HumanMemberFocus", "StandingAgentFocus", "WorkboardPage", "WorkItemFocus", "ApprovalFocus", "FinancePage", "GovernanceProposalFocus", "BusinessModuleFocus"];
  check(required.every((name) => pages.includes(`function ${name}`)), "exports all nine Company OS operations pages");
  check(router.includes('<WorkOperatingPage source={resolved.value} />') && workOperatingPage.includes('data-work-operating-system="v1"'), "routes Work to the native multi-view operating workspace");
  check(["overview", "board", "all", "milestones", "timeline", "workload"].every((view) => workOperatingPage.includes(`id: "${view}"`)), "Work workspace exposes six projections over one WorkItem ledger");
  check(workOperatingPage.includes('useState<WorkView>("board")') && workOperatingPage.includes('"submitted", "accepted", "in_progress", "blocked", "in_review", "waiting_for_approval", "completed"'), "Work opens on the seven-lane operating board in the approved lifecycle order");
  check(workOperatingPage.includes("root.work") && workOperatingPage.includes("projection.work_items") && workOperatingPage.includes("projection.milestones"), "Work workspace consumes native Work and Milestone projections before raw fallback records");
  check(workOperatingPage.includes('"No milestone"') && workOperatingPage.includes('"Unclassified"') && workOperatingPage.includes("Unassigned lane"), "Work views preserve missing Milestone, business-line, and assignment truth");
  check(types.includes('"human" | "standing_agent"') && types.includes("interface ActorSummary"), "keeps Human and Standing Agent as distinct actor kinds");
  check(pages.includes("Execution attempts and hidden reasoning do not define membership") && !pages.includes("MemberRun") && !pages.includes("provider session"), "Standing Agent activity does not collapse into execution lifecycle or provider state");
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
  check(pages.includes("view.organization.company.id") && pages.includes("view.organization.brandUnit.id"), "organization chart exposes real Company and Brand & IP units from the projection");
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
  const internalCommandProjection = structuredClone(fixture);
  internalCommandProjection.approvals[0].title = "Authorize commitment.append to enter the trademark fee into Human review";
  internalCommandProjection.approvals[0].action_summary = "Authorize commitment.append; legal submission remains blocked.";
  const internalCommandAdapted = adapterModule.adaptTrademarkOperationsProjection(internalCommandProjection);
  check(!internalCommandAdapted.approval.title.includes("commitment.append") && !internalCommandAdapted.approval.actionSummary.includes("commitment.append"), "adapter keeps internal command names out of approval business copy");
  const emptyAuthoritativeProjection = adapterModule.adaptTrademarkOperationsProjection({});
  const emptyTruth = JSON.stringify(emptyAuthoritativeProjection);
  check(!emptyTruth.includes("CN-2026-018") && !emptyTruth.includes("¥3,000") && !emptyTruth.includes("Brand Owner"), "an explicit empty authoritative projection never falls back to prototype trademark facts");
  const canonicalProjection = adapterModule.adaptTrademarkOperationsProjection(fixture);
  const brandUnit = canonicalProjection.organization.units.find((unit) => unit.id === "org-brand-ip");
  check(brandUnit?.actorIds.length === 4 && canonicalProjection.governanceProposal.proposedById === "actor-agent-document-architecture", "adapter retains the actual Brand & IP membership branch and governance proposal author");
  check(brandUnit?.agentLeadActorId === "actor-agent-ip-lead" && pages.includes("leadUnit.actorIds") && !pages.includes("candidate.id !== actor.id).slice(0, 4)"), "Lead direct reports come from the governed organization unit instead of actor ordering");
  check(canonicalProjection.actors["actor-agent-document-architecture"]?.availability === "available" && !canonicalProjection.actors["actor-agent-finance"]?.availability, "availability remains explicit rather than inferred from runtime or membership");
  check(pages.includes("OrganizationNode") && pages.includes("OrganizationMember") && pages.includes("membersForUnit"), "organization surface is a connected tree with projection-backed member branches");
  check(pages.includes("Propose agent") && pages.includes("Create org unit") && pages.includes("disabled"), "organization actions are visibly disabled until a governed action path exists");
  check(pages.includes("actor.id === view.workItem.assignees[0]?.id") && pages.includes("linkedDocument"), "proposed agent branch visibly links its actual work source document");
  check(!pages.includes("remainingUnits") && pages.includes("Other explicit organization units"), "unrelated units are secondary explicit projection data, not a generic primary card grid");
  check(pages.includes("<PageFrame dense") && components.includes('dense ? "py-5" : "py-8"') && components.includes('dense ? "mb-4 pb-4" : "mb-7 pb-6"'), "Organization opts into compact vertical rhythm without changing the default page frame");
  check(pages.includes("<LinkedRecord wrapLabel") && components.includes("wrapLabel ? \"whitespace-normal leading-5\" : \"truncate\""), "governance proposal title is allowed to wrap instead of truncating in the context rail");
  check(pages.includes("BoardFact label=\"Requested by\"") && pages.includes("BoardFact label=\"Submitted by\"") && pages.includes("actor={workItem.submittedBy}"), "workboard keeps requester and submitter visible as distinct full actor facts");
  check(pages.indexOf('Panel title="Evidence"') < pages.indexOf('Panel title="Responsibility"') && pages.includes("approvalTitle") && pages.includes("break-words text-sm leading-6"), "WorkItem focus moves evidence into the first viewport and wraps a human-readable approval summary");
  check(pages.includes("FinanceRecordTable") && ["Record type", "Amount", "Project", "Source", "Approval status"].every((label) => pages.includes(`\"${label}\"`)), "finance renders auditable record fields instead of only a summary card");
  check(pages.includes('aria-label="Standing Agent collaboration"') && pages.includes('aria-label="Message composer"') && pages.includes("explicitly reported") && !pages.includes("thinking"), "Standing Agent focus has a central projection-backed collaboration surface and explicit availability without thinking persistence");
  check(pages.includes("authoredProposal") && pages.includes('Panel title="Related structure"') && pages.includes("not a second authored activity"), "Standing Agent distinguishes authored proposal activity from related BusinessModule structure");
  check(pages.includes("textarea") && pages.includes("standing-agent-message-reason") && pages.includes("Send message. Unavailable"), "Standing Agent composer is visibly disabled with a governed transport reason");
  check(pages.includes("displayTimestamp(workItem.updatedAt)") && pages.includes("function displayTimestamp"), "WorkItem focus renders raw update timestamps in a human-readable form");
  check(pages.includes('Panel title="Impact surfaces"') && pages.includes('Panel title="Governed actions"') && pages.includes("Approve proposal") && pages.includes("Request changes"), "governance proposal shows impacts, proposed structure, and honestly disabled governed actions");
  check(pages.includes('decide("approved")') && pages.includes('GovernedActionButton label="Request changes"') && pages.includes('decide("rejected")'), "approval focus has explicit governed approve/reject controls and an honest request-changes boundary");
  check(pages.includes("action={decisionControls}") && pages.includes('aria-label="Approval decision controls"'), "approval decision controls stay in the first-viewport page header");
  check(pages.includes("data-actor-kind={kind}") && pages.includes("data-actor-type={kind}") && pages.includes("BoardFact label=\"Finance reviewer\""), "workboard actor facts preserve canonical actor references and kinds for capture evidence");
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
