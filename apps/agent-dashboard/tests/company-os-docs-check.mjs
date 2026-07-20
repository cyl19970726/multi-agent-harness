#!/usr/bin/env node

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const dashboardRoot = join(here, "..");
const repositoryRoot = join(dashboardRoot, "..", "..");
let passed = 0;
let failed = 0;

function check(condition, message) {
  if (condition) {
    console.log(`  PASS  ${message}`);
    passed += 1;
  } else {
    console.log(`  FAIL  ${message}`);
    failed += 1;
  }
}

async function source(name) {
  return readFile(join(dashboardRoot, "src", "company-os", "docs", name), "utf8");
}

async function loadFixtureAdapter() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "company-os-docs-"));
  try {
    const input = await source("fixtureAdapter.ts");
    const output = ts.transpileModule(input, {
      compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const outputPath = join(directory, "fixtureAdapter.mjs");
    await writeFile(outputPath, output, "utf8");
    return await import(pathToFileURL(outputPath).href);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function main() {
  const fixture = JSON.parse(await readFile(join(repositoryRoot, "docs", "design", "company-os-v1", "fixtures", "company-os-trademark-v1.json"), "utf8"));
  const [index, workspace, document, structured, home, relation, adapter] = await Promise.all([
    source("index.ts"), source("DocsWorkspace.tsx"), source("BasicDocumentPage.tsx"),
    source("StructuredDocumentView.tsx"), source("CompanyHome.tsx"), source("RelationChips.tsx"), source("fixtureAdapter.ts"),
  ]);

  check(index.includes("DocsWorkspace") && index.includes("BasicDocumentPage") && index.includes("StructuredDocumentView") && index.includes("CompanyHome"), "public Docs API exports all four Company OS surfaces");
  check(workspace.includes('data-company-os-page="docs-workspace"') && document.includes('data-company-os-page="document-focus"') && structured.includes('data-company-os-page="business-module-focus"') && home.includes('data-company-os-page="home"'), "capture-ready page markers identify each Docs surface");
  check([workspace, document, structured, home].every((file) => file.includes('data-company-os-ready="true"')), "every Docs root exposes a ready marker");
  check(structured.includes("availableViews") && structured.includes("fallback") && structured.includes("BoardView") && structured.includes("TimelineView"), "structured view exposes standard table, board, timeline, and fallback paths");
  check(document.includes("SimpleTable") && document.includes("RelationChips") && document.includes("sourceLinks") && document.includes("resultLinks"), "basic document supports tables, relation chips, source, and result links");
  check(!document.includes("key={property.label}") && document.includes("property.ref ?? \"property\""), "repeated property labels use a stable React key rather than a duplicate display label");
  check(home.includes("Review decision") && home.includes("decisionRequester") && home.includes("decisionCollaborators"), "Home gives the pending decision a first-viewport review action and structured responsibility context");
  check(home.includes("Button asChild") && home.includes("data.decisionRequired.href") && home.includes("disabled"), "Home renders a real approval link without a callback and never leaves an enabled no-op CTA");
  check(adapter.includes("adaptCompanyOsDocsProjection") && adapter.includes("financialRecordType"), "projection adapter maps financial type from an explicit record field");
  check(!adapter.includes("trademark-application-cn-2026-018") && !adapter.includes("Trademark Management"), "projection adapter contains no canonical trademark fixture IDs or labels");
  check(!adapter.includes('type: "payment"') && !adapter.includes("Paid"), "fixture adapter does not fabricate a payment or settlement state");
  const commitment = fixture.financial_records.find((record) => record.type === "commitment");
  check(commitment?.display_amount === "¥3,000" && commitment?.status === "pending_approval" && !fixture.financial_records.some((record) => record.type === "payment"), "fixture has only the pending ¥3,000 trademark commitment");
  const { adaptCompanyOsDocsProjection, adaptTrademarkDocsFixture } = await loadFixtureAdapter();
  const pages = adaptTrademarkDocsFixture(fixture);
  check(pages.document.sourceLinks?.[0]?.label === "Trademark application CN-2026-018" && pages.document.resultLinks?.[0]?.label === "Trademark filing for Brand A", "fixture adapter preserves source and WorkItem provenance");
  check(pages.home.decisionActor?.name === "Brand Owner" && pages.home.financeSummary[0]?.value === "¥3,000" && pages.home.financeSummary[0]?.financialRecordType === "commitment", "home preserves the human decision and pending-commitment distinction");
  check(pages.home.decisionRequired?.href === "?surface=approvals&approval=approval-trademark-filing-fee-cn-2026-018", "projection adapter supplies the Home review CTA with the selected approval route");
  check(!/^[a-z][a-z0-9]*(?:[._:-][a-z0-9-]+)+$/i.test(pages.home.decisionRequired?.label ?? "") && pages.home.decisionRequired?.label !== pages.home.decisionSummary && pages.home.decisionRequester?.label === "Trademark Agent" && (pages.home.decisionCollaborators?.length ?? 0) > 0, "Home derives a readable non-duplicated approval prompt with grouped requester and collaborators");
  check(["actor-agent-content-strategy", "actor-external-lawyer"].every((id) => pages.home.decisionCollaborators?.some((actor) => actor.id === id)), "Home contributor selection retains projection-backed strategy and external legal collaborators without broad actor dumping");
  const documentHeadings = pages.document.blocks.filter((block) => block.type === "heading").map((block) => block.content);
  const documentTables = pages.document.blocks.filter((block) => block.type === "table").map((block) => block.table.caption);
  check(pages.workspace.rootSelected === true && !pages.workspace.tree.flatMap((item) => item.children ?? []).some((item) => item.selected), "Docs workspace selection remains on the Company workspace root");
  check(pages.workspace.tree.flatMap((item) => item.children ?? []).some((item) => /Trademark Management/.test(item.label) && /Proposed/.test(item.meta ?? "")), "proposed module is discoverable from the Company workspace tree");
  check(documentHeadings.includes("What this plan coordinates") && documentHeadings.includes("Why this context matters") && documentHeadings.includes("Strategy and next review") && documentTables.includes("Linked work") && documentTables.includes("Reported metrics"), "Document Focus renders projection-backed what, why, next, work, and metric sections");
  check(document.includes("grid min-w-0") && document.includes("DocumentSurface className=\"mx-0 min-w-0") && document.includes("break-words text-sm leading-6"), "Document Focus constrains intrinsic content width and wraps copy on mobile");
  check(pages.document.properties?.some((property) => property.label === "Project status" && property.value === "On track") && !/T\d{2}:\d{2}:\d{2}/.test(pages.document.updatedLabel ?? ""), "Document Focus preserves on-track fixture truth and formats timestamps for people");
  const emptyPages = adaptCompanyOsDocsProjection({});
  check(emptyPages.workspace.tree.length === 0 && emptyPages.document.id === undefined && emptyPages.home.decisionRequired === undefined && emptyPages.home.financeSummary.length === 0, "empty projections render honest empty Docs data without fixture facts");
  const alternatePages = adaptCompanyOsDocsProjection({
    documents: [{ id: "document-live-1", title: "Live operating brief", space: "Operations" }],
    typed_records: [{ id: "record-live-1", record_type: "Initiative", source_document_ref: "document-live-1" }],
    work_items: [{ id: "work-live-1", title: "Prepare live brief", source_document_ref: "document-live-1" }],
  });
  check(alternatePages.document.id === "document-live-1" && alternatePages.document.title === "Live operating brief" && alternatePages.home.changes.every((link) => !/trademark|brand a/i.test(link.label)), "a different live projection maps only its supplied records");
  check([workspace, document, structured, home, relation].every((file) => file.includes("data-company-os-ref")) && relation.includes("data-financial-record-type") && home.includes("data-actor-type"), "visible Docs, record, finance, and actor nodes propagate semantic references");

  const pageRefs = {
    home: new Set([
      pages.home.decisionRequired?.id,
      ...pages.home.changes.map((link) => link.id),
      pages.home.decisionActor?.id,
      pages.home.decisionRequester?.id,
      ...(pages.home.decisionCollaborators ?? []).map((link) => link.id),
      ...pages.home.workSummary.flatMap((item) => item.id ? [item.id] : []),
      ...pages.home.financeSummary.flatMap((item) => item.id ? [item.id] : []),
    ]),
    "docs-workspace": new Set([
      ...pages.workspace.tree.flatMap((item) => [item.ref, ...(item.children ?? []).map((child) => child.ref)]),
      ...(pages.workspace.recentlyUpdated ?? []).map((link) => link.id),
      ...(pages.workspace.suggestions ?? []).map((link) => link.id),
      pages.workspace.proposal?.id,
    ].filter(Boolean)),
    "document-focus": new Set([
      pages.document.id,
      ...(pages.document.properties ?? []).flatMap((property) => property.ref ? [property.ref] : []),
      ...(pages.document.sourceLinks ?? []).map((link) => link.id),
      ...(pages.document.resultLinks ?? []).map((link) => link.id),
      ...(pages.document.connectedRecords ?? []).map((link) => link.id),
    ]),
    "business-module-focus": new Set([
      pages.moduleView.id,
      ...pages.moduleView.records.flatMap((record) => [record.id, ...(record.links ?? []).map((link) => link.id)]),
      ...(pages.moduleView.sourceLinks ?? []).map((link) => link.id),
      ...(pages.moduleView.resultLinks ?? []).map((link) => link.id),
    ]),
  };
  for (const page of ["home", "docs-workspace", "document-focus", "business-module-focus"]) {
    const missing = fixture.page_slices[page].required_refs.filter((ref) => !pageRefs[page].has(ref));
    check(missing.length === 0, `${page} adapter exposes every fixture-required reference through a visible node (${missing.join(", ") || "complete"})`);
  }
  const crossPageRefs = [
    "document-trademark-application-cn-2026-018",
    "trademark-application-cn-2026-018",
    "workitem-trademark-filing-brand-a",
    "approval-trademark-filing-fee-cn-2026-018",
    "financial-commitment-trademark-filing-fee-cn-2026-018",
  ];
  for (const page of ["home", "docs-workspace", "document-focus"]) {
    const missing = crossPageRefs.filter((ref) => !pageRefs[page].has(ref));
    check(missing.length === 0, `${page} has visible document/application/work/approval/commitment reference nodes (${missing.join(", ") || "complete"})`);
  }

  console.log(`\n   Company OS Docs checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
