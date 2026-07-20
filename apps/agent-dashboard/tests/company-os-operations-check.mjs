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
  const [pages, fixture] = await Promise.all([
    readFile(resolve(operations, "pages.tsx"), "utf8"),
    readFile(fixturePath, "utf8").then(JSON.parse),
  ]);
  const [components, fixtureAdapter] = await Promise.all([
    readFile(resolve(operations, "components.tsx"), "utf8"),
    readFile(resolve(operations, "fixture.ts"), "utf8"),
  ]);
  const types = await readFile(resolve(operations, "types.ts"), "utf8");
  const ts = (await import("typescript")).default;
  const adapterDirectory = await mkdtemp(resolve(tmpdir(), "company-os-operations-"));
  const adapterTarget = resolve(adapterDirectory, "fixture.mjs");
  await writeFile(adapterTarget, ts.transpileModule(fixtureAdapter, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText, "utf8");
  const adapterModule = await import(pathToFileURL(adapterTarget).href);
  const required = ["OrganizationPage", "HumanMemberFocus", "StandingAgentFocus", "WorkboardPage", "WorkItemFocus", "ApprovalFocus", "FinancePage", "GovernanceProposalFocus", "BusinessModuleFocus"];
  check(required.every((name) => pages.includes(`function ${name}`)), "exports all nine Company OS operations pages");
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
  check(pages.includes('GovernedActionButton label="Approve"') && pages.includes('GovernedActionButton label="Request changes"') && pages.includes('GovernedActionButton label="Reject"'), "approval focus has explicit governed decision controls when transport is unavailable");
  check(pages.includes("action={decisionControls}") && pages.includes('aria-label="Approval decision controls"'), "approval decision controls stay in the first-viewport page header");
  check(pages.includes("data-actor-kind={kind}") && pages.includes("data-actor-type={kind}") && pages.includes("BoardFact label=\"Finance reviewer\""), "workboard actor facts preserve canonical actor references and kinds for capture evidence");
  check(pages.includes("data-financial-record-type={record.type}") && pages.includes("data-financial-status={record.status}") && pages.includes("FinanceRecordTable"), "finance audit table preserves commitment reference, type, and state evidence");
  check(pages.includes('ImpactSurface label="Financial commitment" financialRecord={view.commitment}') && pages.includes("<FinancialRecordCard record={financialRecord} />"), "governance financial impact preserves the linked commitment semantic marker");
  check(fixtureAdapter.includes("approvalPresentation") && fixtureAdapter.includes("financialBusinessLabel") && !fixtureAdapter.includes("title: text(approvalRecord.title"), "approval and finance presentation remove internal command names from primary business copy");
  check(fixtureAdapter.includes("humanizeEvidenceLabel") && fixtureAdapter.includes('return "Lawyer review"'), "raw evidence references receive readable evidence labels");
  check(!canonicalProjection.approval.title.includes("commitment.append") && !canonicalProjection.approval.actionSummary.includes("commitment.append"), "canonical approval copy never exposes the internal commitment command");
  check(canonicalProjection.evidence.every((item) => !item.label.startsWith("evidence-")), "canonical evidence copy never exposes raw evidence ids as labels");
  await rm(adapterDirectory, { recursive: true, force: true });
  console.log(`\nCompany OS operations checks: ${pass} pass, ${fail} fail`);
  process.exit(fail === 0 ? 0 : 1);
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
