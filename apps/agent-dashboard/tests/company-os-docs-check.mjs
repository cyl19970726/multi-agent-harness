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

/** The document tree is a real hierarchy, so ref collection must walk every depth. */
function flattenTree(items = []) {
  return items.flatMap((item) => [item, ...flattenTree(item.children)]);
}

function sortedIds(values) {
  return [...values].filter(Boolean).sort();
}

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

async function loadDocumentTree() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "company-os-docs-tree-"));
  try {
    const input = await source("documentTree.ts");
    const output = ts.transpileModule(input, {
      compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const outputPath = join(directory, "documentTree.mjs");
    await writeFile(outputPath, output, "utf8");
    return await import(pathToFileURL(outputPath).href);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function loadDocumentAction() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "company-os-docs-action-"));
  try {
    const input = await source("documentAction.ts");
    const output = ts.transpileModule(input, {
      compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const outputPath = join(directory, "documentAction.mjs");
    await writeFile(outputPath, output, "utf8");
    return await import(pathToFileURL(outputPath).href);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function main() {
  const fixture = JSON.parse(await readFile(join(repositoryRoot, "docs", "design", "company-os-v1", "fixtures", "company-os-trademark-v1.json"), "utf8"));
  const [index, workspace, document, structured, home, relation, health, healthAction, documentAction, adapter, types, tree] = await Promise.all([
    source("index.ts"), source("DocsWorkspace.tsx"), source("BasicDocumentPage.tsx"),
    source("StructuredDocumentView.tsx"), source("CompanyHome.tsx"), source("RelationChips.tsx"), source("DocumentHealthReview.tsx"), source("healthAction.ts"), source("documentAction.ts"), source("fixtureAdapter.ts"), source("types.ts"), source("documentTree.ts"),
  ]);

  check(index.includes("DocsWorkspace") && index.includes("BasicDocumentPage") && index.includes("StructuredDocumentView") && index.includes("CompanyHome") && index.includes("DocumentHealthReview"), "public Docs API exports all five Company OS Docs surfaces");
  check(index.includes("buildDocsTypedRecordCommand") && index.includes("buildDocsViewCommand") && index.includes("buildDocsRelationCommand") && index.includes("buildDocsReorderBlocksCommand"), "public Docs API exports Store-live module authoring command builders");
  check(workspace.includes('data-company-os-page="docs-workspace"') && document.includes('data-company-os-page="document-focus"') && structured.includes('data-company-os-page="business-module-focus"') && home.includes('data-company-os-page="home"') && health.includes('data-company-os-page="document-health"'), "capture-ready page markers identify each Docs surface");
  check([workspace, document, structured, home, health].every((file) => file.includes('data-company-os-ready="true"')), "every Docs root exposes a ready marker");
  check(structured.includes('className="h-full space-y-4 overflow-y-auto"'), "standard business-module pages retain their own bounded vertical scroll owner");
  check(structured.includes("availableViews") && structured.includes("fallback") && structured.includes("BoardView") && structured.includes("TimelineView"), "structured view exposes standard table, board, timeline, and fallback paths");
  check(structured.includes("StandardViewProvenance") && structured.includes('data-docs-standard-view-provenance="true"') && structured.includes("View is presentation, not a second truth"), "structured view exposes provenance for module scope, native View, source kinds, query, and record count");
  check(structured.includes("StandardViewConfiguration") && structured.includes('data-docs-standard-view-configuration="true"') && structured.includes("Configuration is stored in native View.query") && structured.includes('aria-label="View filter field"') && structured.includes('aria-label="View group by"'), "structured view exposes saved View configuration and Store-live View query authoring controls");
  check(structured.includes('data-docs-standard-view-empty="true"') && structured.includes("declared query returned no records") && structured.includes("does not delete the BusinessModule"), "structured view empty state is explicit without fabricating module truth");
  check(structured.includes('data-docs-authoring-panel="business-module-focus"') && structured.includes("buildDocsTypedRecordCommand") && structured.includes("buildDocsViewCommand") && structured.includes("buildDocsRelationCommand"), "Structured module view exposes Store-live TypedRecord, View, and Relation authoring controls");
  check(document.includes("SimpleTable") && document.includes("RelationChips") && document.includes("sourceLinks") && document.includes("resultLinks"), "basic document supports tables, relation chips, source, and result links");
  check(adapter.includes('kind === "table" || kind === "simple_table"'), "projection adapter renders CLI-native simple_table Blocks through the Document Focus table component");
  check(document.includes("data-docs-authoring-panel=\"document-focus\"") && document.includes("buildDocsChildDocumentCommand") && document.includes("buildDocsAppendBlockCommands") && document.includes("Document.block_ids"), "Document Focus exposes Store-live child Document and Block authoring controls");
  check(document.includes('aria-label="Child document template"') && document.includes("templateOptions") && document.includes("childTemplateRef"), "Document Focus exposes template provenance selection for child Documents");
  check(document.includes("buildDocsInstantiateTemplateBlockCommands") && document.includes('aria-label="Instantiate template blocks"') && document.includes('data-docs-template-instantiation="browser-action"'), "Document Focus exposes Store-live opt-in template Block instantiation controls");
  check(document.includes('data-docs-block-composer="true"') && document.includes("data-docs-block-kind-option") && document.includes("data-docs-block-composer-hint"), "Document Focus exposes a Notion-like governed Block composer with type affordances and durable-action hinting");
  check(document.includes('data-docs-slash-menu="true"') && document.includes('aria-label="Slash menu block commands"') && document.includes("data-docs-slash-command") && document.includes("/heading"), "Document Focus exposes a slash-menu affordance for governed Block type selection");
  check(document.includes('data-docs-block-order-boundary="true"') && document.includes("Document.block_ids sequence") && document.includes("data-docs-block-reorder") && document.includes("governed document.append update"), "Document Focus exposes native block order and governed reorder controls");
  check(document.includes("data-docs-authoring-error-boundary") && document.includes("role=\"status\"") && document.includes("server validates definition, policy, actor permission"), "Document Focus exposes governed authoring error and permission feedback boundary");
  check(document.includes('data-docs-document-architecture="true"') && document.includes('data-docs-document-architecture-link="true"') && document.includes("DocumentArchitecture") && document.includes("preserveCompanyOsWorkbenchContext"), "Document Focus exposes projection-backed document architecture navigation that preserves live api/project context");
  check(document.includes('data-docs-empty-document="true"') && document.includes("data-docs-template-provenance") && document.includes("template Blocks are copied only by an explicit governed instantiation action"), "Document Focus surfaces empty document and template provenance states without fabricating content");
  check(document.includes("data-docs-template-record-policy") && document.includes("Template Blocks do not create records") && document.includes("Use a governed Relation after the child Document and TypedRecord exist"), "Document Focus exposes template-to-TypedRecord relation boundary during child Document creation");
  check(document.includes('aria-label="Block kind"') && document.includes('value: "heading"') && document.includes('value: "callout"') && document.includes('value: "table"'), "Document Focus exposes structured Block authoring controls");
  check(!document.includes("key={property.label}") && document.includes("property.ref ?? \"property\""), "repeated property labels use a stable React key rather than a duplicate display label");
  check(home.includes("Review decision") && home.includes("decisionRequester") && home.includes("decisionCollaborators"), "Home gives the pending decision a first-viewport review action and structured responsibility context");
  check(home.includes("Button asChild") && home.includes("data.decisionRequired.href") && home.includes("disabled"), "Home renders a real approval link without a callback and never leaves an enabled no-op CTA");
  check(adapter.includes("adaptCompanyOsDocsProjection") && adapter.includes("financialRecordType"), "projection adapter maps financial type from an explicit record field");
  check(types.includes("documentTree?: CompanyOsWorkspaceTreeItem[]") && adapter.includes("documentTree: workspaceTree"), "projection adapter supplies the same Store-backed document tree to Document Focus without hard-coded project navigation");
  check(adapter.includes("buildDocumentHealthData") && adapter.includes("missing_document_record_relation") && adapter.includes("No deletion without governed action") === false, "projection adapter computes document health without embedding UI policy copy");
  check(workspace.includes('data-docs-template-library="true"') && workspace.includes("data-docs-template-block-count") && workspace.includes("template_ref only") && workspace.includes("copy Blocks via Actions"), "Docs Workspace exposes a native template library with provenance and instantiation boundaries");
  check(workspace.includes("data-docs-template-lifecycle") && workspace.includes("harness company docs template status") && workspace.includes("archiving a template does not mutate existing Documents"), "Docs Workspace exposes template lifecycle state and governed status boundary");
  check(adapter.includes("template-create") && adapter.includes("harness company docs template create") && adapter.includes("--from-document <source-doc-id>"), "Docs Workspace command panel exposes reusable template creation without treating existing pages as mutable templates");
  check(adapter.includes("template-status") && adapter.includes("harness company docs template status") && adapter.includes("active|paused|archived"), "Docs Workspace command panel exposes governed template lifecycle updates");
  check(workspace.includes("data-docs-template-record-policy") && workspace.includes("Template → TypedRecord policy") && workspace.includes("Template instantiation never creates TypedRecords or Relations"), "Docs Workspace exposes template-to-TypedRecord relation policy without hidden record creation");
  check(workspace.includes('data-docs-workspace-search="projection"') && workspace.includes("Search projection-backed Docs workspace") && workspace.includes('data-docs-workspace-search-boundary="projection-only"') && workspace.includes("filteredSpaces") && workspace.includes("filteredTemplates") && workspace.includes("filteredRecent"), "Docs Workspace filters spaces, templates, and recent records from the current projection without claiming global search");
  check(workspace.includes("preserveCompanyOsWorkbenchContext") && relation.includes("preserveCompanyOsWorkbenchContext") && health.includes("preserveCompanyOsWorkbenchContext") && workspace.includes('data-docs-tree-link="true"') && workspace.includes('data-docs-space-link="true"') && workspace.includes('data-docs-recent-link="true"'), "Docs surfaces preserve live api/project context while linking document tree, relation chips, Health, spaces, and recent records");
  check(health.includes("No deletion without governed action") && health.includes("data-docs-health-finding") && health.includes("Create corrective WorkItem") && health.includes("Direct Docs action"), "Document Health Review renders governed cleanup boundaries and actionable findings");
  check(health.includes('data-docs-cleanup-queue="true"') && health.includes("data-docs-cleanup-operation") && health.includes("Rename, split, merge, archive, and migration are high-judgment operations"), "Document Health Review exposes high-judgment cleanup routing without direct cleanup execution");
  check(health.includes("data-docs-health-action-token") && health.includes("data-docs-health-corrective-note") && health.includes("onCreateCorrectiveWork") && health.includes("data-company-os-action-state"), "Document Health Review exposes Store-live corrective WorkItem controls without storing capability");
  check(health.includes("onRepairRelation") && health.includes("data-docs-health-direct-action-state") && health.includes("buildDocsHealthRelationRepairCommand"), "Document Health Review exposes Store-live direct Relation repair controls without storing capability");
  check(healthAction.includes('command_name: "work_item.append"') && healthAction.includes('subject_ref: { kind: "document"') && healthAction.includes('required_permission: "company.records.write"') && !healthAction.includes("commitment") && !healthAction.includes("payment"), "Document Health corrective action builds a native WorkItem command without Finance effects");
  check(healthAction.includes('command_name: "relation.append"') && healthAction.includes('relation_type: context.relationType') && healthAction.includes('provenance_ref') && !healthAction.includes("action_note"), "Document Health direct action builds a strict native Relation command without polluting relation records");
  check(documentAction.includes('command_name: "document.append"') && documentAction.includes('command_name: "block.append"') && documentAction.includes("block_ids: [...context.blockIds, blockId]") && !documentAction.includes("work_item") && !documentAction.includes("commitment"), "Document authoring actions build native Docs commands and preserve Document.block_ids without Work or Finance effects");
  check(documentAction.includes("buildDocsReorderBlocksCommand") && documentAction.includes("Block reorder must preserve exactly the existing Document.block_ids set") && documentAction.includes("block_ids: next") && documentAction.includes('command_name: "document.append"'), "Document authoring actions support governed block reorder without changing Block content or non-Docs systems");
  check(documentAction.includes("templateRef") && documentAction.includes("template_ref: params.templateRef?.trim() || null") && documentAction.includes("template_ref: context.templateRef ?? null"), "Document authoring actions preserve optional template_ref provenance without clearing template content");
  check(documentAction.includes("buildDocsInstantiateTemplateBlockCommands") && documentAction.includes("template.templateBlocks") && documentAction.includes("referenced_entities: templateBlock.referencedEntities") && documentAction.includes('command_name: "block.append"') && documentAction.includes('command_name: "document.append"'), "Document authoring actions instantiate template Blocks through governed Block and Document commands");
  check(documentAction.includes("blockKind") && documentAction.includes('kind: blockKind') && documentAction.includes("columns") && documentAction.includes("calloutTitle"), "Document authoring actions build structured Block content for heading, callout, and table variants");
  check(documentAction.includes('command_name: "typed_record.append"') && documentAction.includes('command_name: "view.append"') && documentAction.includes('command_name: "relation.append"') && documentAction.includes('subject_ref: { kind: "business_module"') && documentAction.includes('source_document_ref: context.sourceDocumentId'), "Module authoring actions build native TypedRecord, View, and Relation commands from scoped Docs context");
  check(documentAction.includes("mode: params.mode ?? \"table\"") && documentAction.includes("source_kinds: sourceKinds?.length ? sourceKinds : [\"typed_record\"]") && documentAction.includes("query: params.query ?? {}"), "View authoring command preserves saved mode, source kinds, and query configuration in native View records");
  const [captureScript, seedScript] = await Promise.all([
    readFile(join(repositoryRoot, "scripts", "capture-company-os-v2.mjs"), "utf8"),
    readFile(join(repositoryRoot, "scripts", "seed-company-os-trademark-v1.mjs"), "utf8"),
  ]);
  check(captureScript.includes("--docs-health-action-token") && captureScript.includes("docs_health_action") && captureScript.includes("payment_count") && captureScript.includes("idempotent_replay"), "capture script verifies Store-live Docs Health corrective WorkItem action without payment side effects");
  check(captureScript.includes("--docs-health-relation-token") && captureScript.includes("docs_health_relation_action") && captureScript.includes("work_item_count_before"), "capture script verifies Store-live direct Docs Relation repair without Work or Finance side effects");
  check(captureScript.includes("--docs-module-action-token") && captureScript.includes("docs_module_action") && captureScript.includes('"typed_record.append"') && captureScript.includes('"view.append"') && captureScript.includes('"relation.append"') && captureScript.includes("work_item_count_before"), "capture script verifies Store-live standard module TypedRecord/View/Relation authoring without Work or Finance side effects");
  check(seedScript.includes("--capture-docs-health-action") && seedScript.includes("--docs-health-action-token"), "seed script can run the Store-live Docs Health action acceptance path");
  check(seedScript.includes("--capture-docs-health-relation") && seedScript.includes("--docs-health-relation-token") && seedScript.includes('"relation.append"'), "seed script declares and captures Store-live Docs Relation repair acceptance");
  check(seedScript.includes("--capture-docs-module-action") && seedScript.includes("--docs-module-action-token"), "seed script declares and captures Store-live Docs module authoring acceptance");
  check(!adapter.includes("trademark-application-cn-2026-018") && !adapter.includes("Trademark Management"), "projection adapter contains no canonical trademark fixture IDs or labels");
  check(!adapter.includes('type: "payment"') && !adapter.includes("Paid"), "fixture adapter does not fabricate a payment or settlement state");
  const commitment = fixture.financial_records.find((record) => record.type === "commitment");
  check(commitment?.display_amount === "¥3,000" && commitment?.status === "pending_approval" && !fixture.financial_records.some((record) => record.type === "payment"), "fixture has only the pending ¥3,000 trademark commitment");
  const { adaptCompanyOsDocsProjection, adaptTrademarkDocsFixture } = await loadFixtureAdapter();
  const pages = adaptTrademarkDocsFixture(fixture);
  const archivedSourceProjection = structuredClone(fixture);
  const archivedSource = archivedSourceProjection.documents.find((entry) => entry.id === "document-trademark-application-cn-2026-018");
  archivedSource.lifecycle_status = "archived";
  const archivedSourcePages = adaptCompanyOsDocsProjection(
    archivedSourceProjection,
    { documentId: archivedSource.id },
  );
  check(
    archivedSourcePages.document.id === archivedSource.id
      && archivedSourcePages.document.lifecycleStatus === "archived"
      && archivedSourcePages.document.authoring === undefined
      && archivedSourcePages.document.resultLinks?.some((link) => link.id === "workitem-trademark-filing-brand-a"),
    "an explicitly selected archived source remains readable and linked to active Work while Store-live authoring is disabled",
  );
  check(document.includes('data-docs-archived-history="true"') && document.includes("Archived history.") && types.includes("lifecycleStatus?: string"), "Document Focus labels the read-only archived history route instead of degrading Work provenance to a raw id");
  const archivedHealthPages = adaptCompanyOsDocsProjection(archivedSourceProjection, {});
  check(
    archivedHealthPages.health.findings.some((finding) => finding.kind === "work_item_source_document_archived"
      && finding.severity === "warning"
      && finding.subject?.id === "workitem-trademark-filing-brand-a"
      && finding.related?.id === archivedSource.id
      && finding.related?.label === archivedSource.title
      && finding.related?.meta === "archived")
      && archivedHealthPages.health.findings.some((finding) => finding.kind === "typed_record_source_document_archived" && finding.severity === "warning")
      && !archivedHealthPages.health.findings.some((finding) => (finding.kind === "work_item_source_document_missing" || finding.kind === "typed_record_source_document_missing") && finding.related?.id === archivedSource.id),
    "Docs health reports archived Work and record sources as explicit archived history with title and lifecycle, never as missing",
  );
  check(
    archivedHealthPages.home.changes.some((link) => link.id === archivedSource.id && link.meta === "Archived history"),
    "Home keeps the archived Work source navigable as archived history instead of falling back to another Document",
  );
  const missingSourceProjection = structuredClone(fixture);
  missingSourceProjection.work_items[0].source_document_ref = "document-pruned-away";
  const missingSourcePages = adaptCompanyOsDocsProjection(missingSourceProjection, {});
  check(
    missingSourcePages.health.findings.some((finding) => finding.kind === "work_item_source_document_missing"
      && finding.severity === "critical"
      && finding.subject?.id === "workitem-trademark-filing-brand-a"
      && finding.related?.id === "document-pruned-away"),
    "a missing Work source is a critical Docs health finding instead of a silently degraded id",
  );
  check(adapter.includes("work_item_source_document_archived") && adapter.includes("work_item_source_document_missing") && adapter.includes("typed_record_source_document_archived") && adapter.includes("sourceDocuments"), "Docs health adapter computes Work and record archived-source findings from the unfiltered Document projection");
  check(pages.document.sourceLinks?.[0]?.label === "Trademark application CN-2026-018" && pages.document.resultLinks?.[0]?.label === "Trademark filing for Brand A", "fixture adapter preserves source and WorkItem provenance");
  check(pages.home.decisionActor?.name === "Brand Owner" && pages.home.financeSummary[0]?.value === "¥3,000" && pages.home.financeSummary[0]?.financialRecordType === "commitment", "home preserves the human decision and pending-commitment distinction");
  check(pages.home.decisionRequired?.href === "?surface=approvals&approval=approval-trademark-filing-fee-cn-2026-018", "projection adapter supplies the Home review CTA with the selected approval route");
  check(!/^[a-z][a-z0-9]*(?:[._:-][a-z0-9-]+)+$/i.test(pages.home.decisionRequired?.label ?? "") && pages.home.decisionRequired?.label !== pages.home.decisionSummary && pages.home.decisionRequester?.label === "Trademark Agent" && (pages.home.decisionCollaborators?.length ?? 0) > 0, "Home derives a readable non-duplicated approval prompt with grouped requester and collaborators");
  check(["actor-agent-content-strategy", "actor-external-lawyer"].every((id) => pages.home.decisionCollaborators?.some((actor) => actor.id === id)), "Home contributor selection retains projection-backed strategy and external legal collaborators without broad actor dumping");
  const documentHeadings = pages.document.blocks.filter((block) => block.type === "heading").map((block) => block.content);
  const documentTables = pages.document.blocks.filter((block) => block.type === "table").map((block) => block.table.caption);
  check(pages.workspace.rootSelected === true && !pages.workspace.tree.flatMap((item) => item.children ?? []).some((item) => item.selected), "Docs workspace selection remains on the Company workspace root");
  check(pages.workspace.tree.flatMap((item) => item.children ?? []).some((item) => item.href?.startsWith("?surface=docs&document=")) && pages.workspace.recentlyUpdated?.some((link) => link.href?.startsWith("?surface=docs&document=")), "Docs workspace supplies URL-addressable document links for tree and recent records");
  check(pages.document.documentTree?.flatMap((item) => item.children ?? []).some((item) => item.href?.startsWith("?surface=docs&document=")), "Document Focus receives URL-addressable document architecture links from the same projection-backed tree");
  check(pages.workspace.tree.flatMap((item) => item.children ?? []).some((item) => /Trademark Management/.test(item.label) && /Proposed/.test(item.meta ?? "")), "proposed module is discoverable from the Company workspace tree");
  check(pages.workspace.maintainers?.some((actor) => actor.id === "actor-agent-document-architecture" && actor.actorType === "Standing Agent"), "Docs workspace exposes projection-backed Standing Agent maintainers");
  check(pages.moduleView.provenance?.moduleId === "module-trademark-management" && pages.moduleView.provenance?.sourceKinds?.includes("typed_record") && pages.moduleView.provenance?.recordCount === pages.moduleView.records.length, "Business Module standard view provenance preserves module scope, source kinds, and record count");
  check(pages.moduleView.configuration?.mode === "table" && pages.moduleView.configuration?.sourceKinds?.includes("typed_record"), "Business Module standard view configuration preserves fallback mode and source kinds when the projection has no native View row");
  const configuredViewPages = adaptCompanyOsDocsProjection({
    documents: [{ id: "document-configured-module-root", space_id: "company", parent_document_id: null, title: "Configured module root", kind: "page", lifecycle_status: "active", block_ids: [], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "human", actor_id: "actor-human-configured-module" }, updated_by: { actor_type: "human", actor_id: "actor-human-configured-module" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" }],
    business_modules: [{ id: "module-configured-standard-view", name: "Configured module", root_document_ref: "document-configured-module-root", status: "active", default_view_refs: ["view-configured-standard"] }],
    views: [{ id: "view-configured-standard", module_id: "module-configured-standard-view", title: "Configured standard records", mode: "board", source_kinds: ["typed_record"], query: { filters: [{ field: "record_type", value: "trademark_application" }], group_by: "lifecycle_status", sort_by: "updated_at" }, owner: { actor_type: "human", actor_id: "actor-human-configured-module" }, policy_refs: ["company.records.write"], created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" }],
    typed_records: [],
  }, { moduleId: "module-configured-standard-view" });
  check(configuredViewPages.moduleView.configuration?.mode === "board" && configuredViewPages.moduleView.configuration?.filters?.[0]?.field === "record_type" && configuredViewPages.moduleView.configuration?.groupBy === "lifecycle_status" && configuredViewPages.moduleView.configuration?.sortBy === "updated_at", "Business Module standard view configuration preserves native mode, filters, grouping, sorting, and query object");
  const emptyModulePages = adaptCompanyOsDocsProjection({
    documents: [{ id: "document-empty-module-root", space_id: "company", parent_document_id: null, title: "Empty module root", kind: "page", lifecycle_status: "active", block_ids: [], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "human", actor_id: "actor-human-empty-module" }, updated_by: { actor_type: "human", actor_id: "actor-human-empty-module" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" }],
    business_modules: [{ id: "module-empty-standard-view", name: "Empty module", root_document_ref: "document-empty-module-root", status: "active", default_view_refs: ["view-empty-standard"] }],
    views: [{ id: "view-empty-standard", module_id: "module-empty-standard-view", title: "Empty standard records", mode: "table", source_kinds: ["typed_record"], query: { record_type: "none" }, owner: { actor_type: "human", actor_id: "actor-human-empty-module" }, policy_refs: ["company.records.write"], created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" }],
    typed_records: [],
  }, { moduleId: "module-empty-standard-view" });
  check(emptyModulePages.moduleView.records.length === 0 && emptyModulePages.moduleView.provenance?.viewId === "view-empty-standard" && /record_type/.test(emptyModulePages.moduleView.provenance?.querySummary ?? ""), "empty Business Module standard view retains native View/query provenance without fixture records");
  const templatedPages = adaptCompanyOsDocsProjection({
    actors: [{ id: "actor-agent-docs-template", display_name: "Docs Template Agent", actor_type: "agent", permission_policy_refs: ["company.records.write"] }],
    documents: [
      { id: "document-root-template-test", space_id: "company", parent_document_id: null, title: "Root", kind: "page", lifecycle_status: "active", block_ids: [], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "human", actor_id: "actor-human-brand-owner" }, updated_by: { actor_type: "human", actor_id: "actor-human-brand-owner" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" },
      { id: "template-operating-note", space_id: "company", parent_document_id: "document-root-template-test", title: "Operating note template", kind: "template", lifecycle_status: "active", block_ids: ["block-template-operating-note-1"], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "human", actor_id: "actor-human-brand-owner" }, updated_by: { actor_type: "human", actor_id: "actor-human-brand-owner" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" },
    ],
    blocks: [{ id: "block-template-operating-note-1", document_id: "template-operating-note", kind: "callout", position: 0, content: { title: "Template note", text: "Reusable operating note" }, referenced_entities: [{ kind: "document", id: "document-root-template-test" }], created_by: { actor_type: "human", actor_id: "actor-human-brand-owner" }, updated_by: { actor_type: "human", actor_id: "actor-human-brand-owner" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" }],
    custom_page_definitions: [{ id: "definition-doc-template-test", module_id: "module-template-test", action_command_refs: ["document.append", "block.append"], policy_refs: ["definition-doc-template-test:document.append", "definition-doc-template-test:block.append"] }],
  }, { documentId: "document-root-template-test" });
  check(templatedPages.workspace.templates?.some((template) => template.id === "template-operating-note" && template.meta === "Active") && templatedPages.document.authoring?.templateOptions?.some((template) => template.id === "template-operating-note" && template.templateBlockIds.includes("block-template-operating-note-1")), "projection adapter exposes template Documents, lifecycle state, and ordered template Blocks to Workspace and Document authoring without fabricating template instantiation");
  const templatedPolicyPages = adaptCompanyOsDocsProjection({
    actors: [{ id: "actor-agent-docs-template-policy", display_name: "Docs Template Agent", actor_type: "agent", permission_policy_refs: ["company.records.write"] }],
    documents: [{ id: "document-template-policy-root", space_id: "company", parent_document_id: null, title: "Root", kind: "page", lifecycle_status: "active", block_ids: [], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "agent", actor_id: "actor-agent-docs-template-policy" }, updated_by: { actor_type: "agent", actor_id: "actor-agent-docs-template-policy" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" }],
    business_modules: [{ id: "module-template-policy", name: "Template policy", root_document_ref: "document-template-policy-root", record_types: ["TrademarkApplication"], relation_rules: [{ relation_type: "source_for", from_kind: "document", to_kind: "typed_record", required: true, cross_module: false }], status: "active", default_view_refs: [] }],
    custom_page_definitions: [{ id: "definition-template-policy", module_id: "module-template-policy", action_command_refs: ["document.append", "block.append", "relation.append"], policy_refs: ["definition-template-policy:document.append", "definition-template-policy:block.append", "definition-template-policy:relation.append"] }],
  });
  check(templatedPolicyPages.workspace.templateRecordPolicy?.status === "declared" && templatedPolicyPages.workspace.templateRecordPolicy.relationTypes.includes("source_for") && templatedPolicyPages.document.authoring?.templateRecordPolicy?.recordTypes.includes("TrademarkApplication"), "projection adapter exposes declared template-to-TypedRecord relation policy from native BusinessModule rules");
  const actionModule = await loadDocumentAction();
  const childCommand = actionModule.buildDocsChildDocumentCommand({
    document: templatedPages.document,
    title: "Child from template",
    templateRef: "template-operating-note",
    commandId: "action-test-child-template",
    createdAt: "2026-07-20T10:05:00+08:00",
  });
  const templateCommands = actionModule.buildDocsInstantiateTemplateBlockCommands({
    parentDocument: templatedPages.document,
    childDocumentCommand: childCommand,
    template: templatedPages.document.authoring.templateOptions[0],
    commandId: "action-test-template-copy",
    createdAt: "2026-07-20T10:05:00+08:00",
  });
  check(
    childCommand.command_name === "document.append" &&
      childCommand.payload.record.template_ref === "template-operating-note" &&
      templateCommands.length === 2 &&
      templateCommands[0].command_name === "block.append" &&
      templateCommands[0].payload.record.document_id === childCommand.payload.record.id &&
      templateCommands[0].payload.record.content.title === "Template note" &&
      templateCommands[1].command_name === "document.append" &&
      templateCommands[1].payload.record.block_ids.includes(templateCommands[0].payload.record.id) &&
      !JSON.stringify(templateCommands).includes("work_item") &&
      !JSON.stringify(templateCommands).includes("financial"),
    "Document action builders generate Store-live template Block instantiation commands without Work or Finance effects",
  );
  check(
    pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs module create")) &&
    pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs page-definition create")) &&
    pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs document create")) &&
    pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs template create")) &&
    pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs template status")) &&
    pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs block append")) &&
      pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs block reorder")) &&
      pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs typed-record append")) &&
      pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs view create")) &&
      pages.workspace.authoringCommands?.some((hint) => hint.command.includes("harness company docs relation link")),
    "Docs workspace exposes the complete CLI-backed module, page-definition, document, block, record, view, and relation authoring contracts",
  );
  check(workspace.includes("data-docs-authoring-command") && workspace.includes("CLI / Skill authoring") && workspace.includes("Governance commands require a Human admin"), "Docs workspace renders honest CLI/Skill authoring affordances without fake UI writes");
  check(pages.health.counts.documents === fixture.documents.length && pages.health.counts.typedRecords === fixture.typed_records.length && pages.health.counts.relations === (fixture.relations ?? []).length, "Document Health counts are projection-backed");
  check(pages.health.findings.some((finding) => finding.kind === "missing_document_record_relation") && pages.health.actionHints?.some((hint) => hint.command === "harness company docs health"), "Document Health surfaces relation findings and the ready CLI audit command");
  check(pages.health.findings.every((finding) => !finding.correctiveWorkContext && !finding.relationRepairContext) && pages.health.actionHints?.find((hint) => hint.id === "corrective-work")?.disabledReason, "fixture health review does not fabricate Store-live corrective or direct Docs action contracts");
  const duplicateHealthPages = adaptCompanyOsDocsProjection({
    actors: [{ id: "actor-agent-docs-cleanup", display_name: "Docs Governance Agent", actor_type: "agent", permission_policy_refs: ["company.records.write"] }],
    documents: [
      { id: "document-cleanup-root", space_id: "company", parent_document_id: null, title: "Cleanup Root", kind: "page", lifecycle_status: "active", block_ids: [], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "agent", actor_id: "actor-agent-docs-cleanup" }, updated_by: { actor_type: "agent", actor_id: "actor-agent-docs-cleanup" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" },
      { id: "document-duplicate-a", space_id: "company", parent_document_id: "document-cleanup-root", title: "Vendor onboarding", kind: "page", lifecycle_status: "active", block_ids: [], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "agent", actor_id: "actor-agent-docs-cleanup" }, updated_by: { actor_type: "agent", actor_id: "actor-agent-docs-cleanup" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" },
      { id: "document-duplicate-b", space_id: "company", parent_document_id: "document-cleanup-root", title: "Vendor onboarding", kind: "page", lifecycle_status: "active", block_ids: [], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "agent", actor_id: "actor-agent-docs-cleanup" }, updated_by: { actor_type: "agent", actor_id: "actor-agent-docs-cleanup" }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" },
    ],
    business_modules: [{ id: "module-docs-cleanup", name: "Docs Cleanup", root_document_ref: "document-cleanup-root", status: "active", default_view_refs: [] }],
    custom_page_definitions: [{ id: "definition-docs-cleanup", module_id: "module-docs-cleanup", action_command_refs: ["work_item.append"], policy_refs: ["definition-docs-cleanup:work_item.append"] }],
  });
  check(duplicateHealthPages.health.cleanupQueue?.some((item) => item.operation === "merge" && item.route === "corrective_work_item" && !item.disabledReason), "Document Health routes duplicate-title cleanup through a corrective WorkItem queue");
  check(workspace.includes('className="hidden border-b') && workspace.includes('className="hidden border-t'), "Docs mobile layout prioritizes document content over desktop tree and context rails");
  check(documentHeadings.includes("What this plan coordinates") && documentHeadings.includes("Why this context matters") && documentHeadings.includes("Strategy and next review") && documentTables.includes("Linked work") && documentTables.includes("Reported metrics"), "Document Focus renders projection-backed what, why, next, work, and metric sections");
  check(document.includes("grid min-w-0") && document.includes("DocumentSurface className=\"mx-0 min-w-0") && document.includes("break-words text-sm leading-6"), "Document Focus constrains intrinsic content width and wraps copy on mobile");
  check(pages.document.properties?.some((property) => property.label === "Operating status" && property.value === "On track") && !/T\d{2}:\d{2}:\d{2}/.test(pages.document.updatedLabel ?? ""), "Document Focus preserves on-track fixture truth without reintroducing Project language and formats timestamps for people");
  const emptyPages = adaptCompanyOsDocsProjection({});
  check(emptyPages.workspace.tree.length === 0 && emptyPages.document.id === undefined && emptyPages.home.decisionRequired === undefined && emptyPages.home.financeSummary.length === 0, "empty projections render honest empty Docs data without fixture facts");
  check(
    emptyPages.workspace.archive === undefined
      && emptyPages.document.breadcrumbs === undefined
      && emptyPages.document.childDocuments === undefined
      && emptyPages.document.backlinks === undefined
      && emptyPages.document.missingDocumentId === undefined,
    "an empty projection supplies no archive, breadcrumbs, child documents, backlinks, or not-found marker",
  );
  const alternatePages = adaptCompanyOsDocsProjection({
    documents: [{ id: "document-live-1", title: "Live operating brief", space: "Operations" }],
    typed_records: [{ id: "record-live-1", record_type: "Initiative", source_document_ref: "document-live-1" }],
    work_items: [{ id: "work-live-1", title: "Prepare live brief", source_document_ref: "document-live-1" }],
  });
  check(alternatePages.document.id === "document-live-1" && alternatePages.document.title === "Live operating brief" && alternatePages.home.changes.every((link) => !/trademark|brand a/i.test(link.label)), "a different live projection maps only its supplied records");
  const selectedDocumentPages = adaptCompanyOsDocsProjection({
    documents: [
      { id: "document-selected-a", title: "Selected A", space: "Operations", parent_document_id: null, block_ids: [] },
      { id: "document-selected-b", title: "Selected B", space: "Operations", parent_document_id: null, block_ids: [] },
    ],
    typed_records: [
      { id: "record-selected-a", title: "Selected A record", record_type: "brief", source_document_ref: "document-selected-a" },
      { id: "record-selected-b", title: "Selected B record", record_type: "brief", source_document_ref: "document-selected-b" },
    ],
    work_items: [
      { id: "work-selected-a", title: "Work for selected A", source_document_ref: "document-selected-a" },
      { id: "work-selected-b", title: "Work for selected B", source_document_ref: "document-selected-b" },
    ],
    financial_records: [
      { id: "finance-selected-a", display_name: "Finance for selected A", type: "commitment", work_item_ref: "work-selected-a" },
      { id: "finance-selected-b", display_name: "Finance for selected B", type: "commitment", work_item_ref: "work-selected-b" },
    ],
  }, { documentId: "document-selected-b" });
  check(
    selectedDocumentPages.document.connectedRecords?.some((link) => link.id === "record-selected-b")
      && selectedDocumentPages.document.connectedRecords?.some((link) => link.id === "finance-selected-b")
      && selectedDocumentPages.document.resultLinks?.some((link) => link.id === "work-selected-b")
      && !selectedDocumentPages.document.connectedRecords?.some((link) => link.id === "record-selected-a" || link.id === "finance-selected-a")
      && !selectedDocumentPages.document.resultLinks?.some((link) => link.id === "work-selected-a"),
    "selected Document Focus scopes context rail records to the selected document",
  );
  const archivedPages = adaptCompanyOsDocsProjection({
    documents: [
      { id: "document-active-root", title: "Active Root", space_id: "agentos", parent_document_id: null, kind: "page", lifecycle_status: "active", block_ids: [] },
      { id: "document-archived-company-root", title: "Archived Company Root", space_id: "company", parent_document_id: null, kind: "page", lifecycle_status: "archived", block_ids: [] },
      { id: "document-archived-company-child", title: "Archived Company Child", space_id: "company", parent_document_id: "document-archived-company-root", kind: "page", lifecycle_status: "archived", block_ids: [] },
    ],
    business_modules: [
      { id: "module-active", name: "Active module", root_document_ref: "document-active-root", status: "active", default_view_refs: [] },
      { id: "module-archived-root", name: "Archived root module", root_document_ref: "document-archived-company-root", status: "active", default_view_refs: [] },
    ],
  });
  const archivedVisibleRefs = [
    ...flattenTree(archivedPages.workspace.tree).map((item) => item.ref),
    ...(archivedPages.workspace.recentlyUpdated ?? []).map((link) => link.id),
    ...(archivedPages.health.structureLinks ?? []).map((link) => link.id),
    ...archivedPages.moduleView.sourceLinks.map((link) => link.id),
  ].filter(Boolean);
  check(
    archivedPages.health.counts.documents === 1
      && archivedPages.workspace.spaces?.every((space) => space.name !== "company")
      && !archivedVisibleRefs.some((id) => /archived-company/.test(id))
      && flattenTree(archivedPages.workspace.tree).some((item) => item.id === "module-active")
      && !flattenTree(archivedPages.workspace.tree).some((item) => item.id === "module-archived-root"),
    "Docs workspace hides archived Documents and modules whose root Document is archived from active navigation",
  );
  check([workspace, document, structured, home, relation, health].every((file) => file.includes("data-company-os-ref")) && relation.includes("data-financial-record-type") && home.includes("data-actor-type"), "visible Docs, record, finance, and actor nodes propagate semantic references");

  // U1/U2: the default tree is the real parent_document_id hierarchy under a single
  // "not archived" predicate, and the Archive view is that predicate's exact complement.
  const agentosChildIds = [
    "document-agentos-01-dogfood",
    "document-agentos-02-external-gateway",
    "document-agentos-03-org-work-doc-loop",
    "document-agentos-04-github-connector",
    "document-agentos-10-software-product-sources",
  ];
  const hierarchyDocuments = [
    { id: "document-agentos-root", space_id: "agentos", parent_document_id: null, title: "AgentOS / Star Harness", kind: "page", lifecycle_status: "draft", block_ids: [] },
    ...agentosChildIds.map((id, index) => ({ id, space_id: "agentos", parent_document_id: "document-agentos-root", title: `${String(index).padStart(2, "0")} AgentOS child page`, kind: "page", lifecycle_status: "draft", block_ids: [] })),
    // Archived leaks the single predicate must remove: a duplicate child, a legacy
    // company-space root with its own child, and a cross-space child of an active page.
    { id: "document-agentos-01-dogfood-intake", space_id: "agentos", parent_document_id: "document-agentos-root", title: "01 Dogfood Intake", kind: "page", lifecycle_status: "archived", block_ids: [] },
    { id: "document-agentos-home", space_id: "company", parent_document_id: null, title: "AgentOS Development", kind: "page", lifecycle_status: "archived", block_ids: [] },
    { id: "document-agentos-home-child", space_id: "company", parent_document_id: "document-agentos-home", title: "Legacy development note", kind: "page", lifecycle_status: "archived", block_ids: [] },
    { id: "document-wcw-11-agentos-dogfood", space_id: "company", parent_document_id: "document-wcw-00-project-home", title: "11 AgentOS Dogfood", kind: "page", lifecycle_status: "archived", block_ids: [] },
    { id: "document-wcw-root", space_id: "wanchengwanling", parent_document_id: null, title: "Wanchengwanling", kind: "page", lifecycle_status: "active", block_ids: [] },
    { id: "document-wcw-00-project-home", space_id: "wanchengwanling", parent_document_id: "document-wcw-root", title: "00 Project Home", kind: "page", lifecycle_status: "active", block_ids: [] },
  ];
  const hierarchyPages = adaptCompanyOsDocsProjection({
    documents: hierarchyDocuments,
    business_modules: [
      { id: "module-agentos-project-home", name: "AgentOS project home", root_document_ref: "document-agentos-root", status: "active", default_view_refs: [] },
      { id: "module-agentos-development", name: "AgentOS development", root_document_ref: "document-agentos-home", status: "active", default_view_refs: [] },
    ],
  });
  const hierarchyNodes = flattenTree(hierarchyPages.workspace.tree);
  const agentosRootNode = hierarchyNodes.find((item) => item.ref === "document-agentos-root");
  const archivedDocumentIds = hierarchyDocuments.filter((entry) => entry.lifecycle_status === "archived").map((entry) => entry.id);
  check(
    agentosRootNode !== undefined
      && JSON.stringify(sortedIds((agentosRootNode.children ?? []).map((child) => child.ref))) === JSON.stringify(sortedIds(agentosChildIds))
      && agentosChildIds.every((id) => hierarchyDocuments.find((entry) => entry.id === id).lifecycle_status === "draft"),
    "document-agentos-root nests exactly its five non-archived child pages even though every one of them is still draft",
  );
  check(
    hierarchyPages.workspace.tree.find((space) => space.label === "agentos")?.children?.filter((child) => child.href?.startsWith("?surface=docs&document=")).length === 1
      && flattenTree(hierarchyPages.workspace.tree.filter((space) => space.label === "wanchengwanling")).some((item) => item.ref === "document-wcw-00-project-home" && item.id !== "space:wanchengwanling")
      && hierarchyPages.workspace.tree.find((space) => space.label === "wanchengwanling")?.children?.every((child) => child.ref !== "document-wcw-00-project-home"),
    "space grouping only frames roots: a child Document nests under its parent instead of reappearing as a space-level sibling",
  );
  check(
    hierarchyNodes.some((item) => item.ref) && !hierarchyNodes.some((item) => archivedDocumentIds.includes(item.ref))
      && !hierarchyNodes.some((item) => item.ref === "module-agentos-development")
      && hierarchyNodes.some((item) => item.ref === "module-agentos-project-home"),
    "the default document tree excludes every archived Document and every module whose root Document is archived",
  );
  check(
    hierarchyNodes.every((item) => item.kind === "space" || item.kind === "document" || item.kind === "module")
      && hierarchyPages.workspace.tree.every((item) => item.kind === "space" && item.ref === undefined)
      && hierarchyNodes.filter((item) => item.ref === "document-agentos-root").every((item) => item.kind === "document")
      && hierarchyNodes.filter((item) => item.ref === "module-agentos-project-home").every((item) => item.kind === "module"),
    "the live workspace tree tags grouping spaces, Documents, and BusinessModules with their canonical kinds",
  );
  const archiveRefs = flattenTree(hierarchyPages.workspace.archive?.tree).map((item) => item.ref).filter(Boolean);
  check(
    JSON.stringify(sortedIds(archiveRefs)) === JSON.stringify(sortedIds(archivedDocumentIds))
      && JSON.stringify(sortedIds(hierarchyPages.workspace.archive?.documentIds ?? [])) === JSON.stringify(sortedIds(archivedDocumentIds))
      && hierarchyPages.workspace.archive?.modules.some((module) => module.id === "module-agentos-development")
      && !hierarchyPages.workspace.archive?.modules.some((module) => module.id === "module-agentos-project-home"),
    "the Archive view lists exactly the archived Documents and the modules the default tree withheld, and nothing else",
  );
  check(
    hierarchyPages.workspace.archive?.defaultFilter === 'lifecycle_status != "archived"'
      && !/\bactive\b/.test(hierarchyPages.workspace.archive?.defaultFilter ?? "")
      && flattenTree(hierarchyPages.workspace.archive?.tree).filter((item) => item.ref).every((item) => item.meta === "Archived")
      && flattenTree(hierarchyPages.workspace.archive?.tree).some((item) => item.ref === "document-wcw-11-agentos-dogfood"),
    "the Archive view states the default tree predicate as an exclusion and re-anchors an archived child whose parent stayed in the default tree",
  );
  check(
    workspace.includes('data-docs-archive-view="explicit"') && workspace.includes("data-docs-archive-toggle") && workspace.includes("data-docs-archive-filter") && workspace.includes("exact complement") && types.includes("CompanyOsWorkspaceArchive"),
    "Docs Workspace exposes the archive behind one explicit disclosure that names the default tree predicate",
  );
  check(
    adapter.includes("buildDocumentSpaceTree") && adapter.includes("parent_document_id") && !adapter.includes('lifecycle_status) === "active"'),
    "projection adapter derives the tree from Document.parent_document_id without an active-only lifecycle filter",
  );
  // The space node holds root Documents AND every BusinessModule attached to that space,
  // so a directory anchor chosen by raw child count picks the space and exposes one page
  // instead of eleven. Rank by Document children only.
  const { selectDocumentDirectoryAnchor, documentChildCount, isDocumentTreeNode, filterDocumentTree } = await loadDocumentTree();
  check(
    tree.includes('item.kind === "document"') && !tree.includes('includes("document=")') && !tree.includes('includes("module=")'),
    "documentTree distinguishes real Documents from grouping spaces and modules through the canonical node kind, never href substrings",
  );
  check(
    types.includes('kind?: "space" | "document" | "module"')
      && adapter.includes('kind: "document"') && adapter.includes('kind: "space" as const') && adapter.includes('kind: "module"'),
    "the projection adapter tags every tree node with the canonical kind of the store object it represents",
  );
  check(
    isDocumentTreeNode({ id: "d", ref: "document-x", label: "X", kind: "document" }) === true
      && isDocumentTreeNode({ id: "space:x", label: "X", kind: "space" }) === false
      && isDocumentTreeNode({ id: "m", ref: "module-x", label: "X", kind: "module" }) === false
      && documentChildCount({ id: "s", label: "s", kind: "space", children: [{ id: "d", ref: "d", label: "d", kind: "document" }, { id: "m", ref: "m", label: "m", kind: "module" }] }) === 1,
    "Document node detection and child counts accept only canonical Document nodes",
  );
  check(
    JSON.stringify(filterDocumentTree([{ id: "space:x", label: "x", kind: "space", children: [{ id: "d1", ref: "d1", label: "d1", kind: "document" }, { id: "m1", ref: "m1", label: "m1", kind: "module" }] }, { id: "space:empty", label: "empty", kind: "space", children: [{ id: "m2", ref: "m2", label: "m2", kind: "module" }] }]))
      === JSON.stringify([{ id: "space:x", label: "x", kind: "space", children: [{ id: "d1", ref: "d1", label: "d1", kind: "document" }] }]),
    "a document tree renders only Document children, dropping module nodes and spaces left without Documents",
  );
  check(
    document.includes("filterDocumentTree") && document.includes("isDocumentTreeNode"),
    "Document Focus renders documents-only trees and directory cards",
  );
  const wcwNumberedIds = Array.from({ length: 11 }, (_, index) => `document-wcw-${String(index).padStart(2, "0")}`);
  const wcwProjection = {
    documents: [
      { id: "document-wcw-root", space_id: "wanchengwanling", parent_document_id: null, title: "Wanchengwanling / 万城万灵", kind: "page", lifecycle_status: "active", block_ids: [] },
      ...wcwNumberedIds.map((id, index) => ({ id, space_id: "wanchengwanling", parent_document_id: "document-wcw-root", title: `${String(index).padStart(2, "0")} Wanchengwanling page`, kind: "page", lifecycle_status: "active", block_ids: [] })),
    ],
    // Eleven modules on the same space: the space node therefore has twelve raw children
    // while the real page holder has eleven.
    business_modules: wcwNumberedIds.map((id, index) => ({ id: `module-wcw-${index}`, name: `Wanchengwanling module ${index}`, root_document_ref: id, status: "active", default_view_refs: [] })),
  };
  const wcwPages = adaptCompanyOsDocsProjection(wcwProjection, { documentId: "document-wcw-00" });
  const wcwSpaceNode = wcwPages.workspace.tree.find((item) => item.label === "wanchengwanling");
  const wcwAnchor = selectDocumentDirectoryAnchor(wcwPages.document.documentTree, /wanchengwanling|万城万灵/i);
  check(
    (wcwSpaceNode?.children?.length ?? 0) > documentChildCount(wcwSpaceNode ?? {})
      && wcwAnchor?.ref === "document-wcw-root"
      && (wcwAnchor?.children ?? []).filter((child) => child.href?.includes("document=")).length === 11
      && JSON.stringify(sortedIds((wcwAnchor?.children ?? []).filter((child) => child.href?.includes("document=")).map((child) => child.ref))) === JSON.stringify(sortedIds(wcwNumberedIds)),
    "the document directory anchors on the Document holding all eleven numbered pages, not the space node whose children are inflated by BusinessModules",
  );
  check(
    selectDocumentDirectoryAnchor([{ id: "space:x", label: "wanchengwanling", kind: "space", children: [{ id: "m", ref: "m", label: "wanchengwanling module", kind: "module", href: "?surface=docs&module=m" }] }], /wanchengwanling/i)?.id === "space:x"
      && selectDocumentDirectoryAnchor(undefined, /wanchengwanling/i) === undefined,
    "the directory anchor degrades safely when a matching node has only BusinessModule children or no tree is supplied",
  );

  // Conservation: a BusinessModule is in the default tree or the Archive, never neither.
  const orphanSpaceProjection = {
    documents: [
      { id: "document-conserve-root", space_id: "wanchengwanling", parent_document_id: null, title: "Conserve root", kind: "page", lifecycle_status: "active", block_ids: [] },
      // Lives in space "company" but nests under a wanchengwanling parent, so space
      // "company" ends up with no root Document and therefore no tree node at all.
      { id: "document-conserve-cross-space", space_id: "company", parent_document_id: "document-conserve-root", title: "Cross space child", kind: "page", lifecycle_status: "active", block_ids: [] },
      { id: "document-conserve-archived", space_id: "wanchengwanling", parent_document_id: null, title: "Conserve archived", kind: "page", lifecycle_status: "archived", block_ids: [] },
    ],
    business_modules: [
      { id: "module-conserve-placed", name: "Placed module", root_document_ref: "document-conserve-root", status: "active", default_view_refs: [] },
      { id: "module-conserve-no-space", name: "Module without a space node", root_document_ref: "document-conserve-cross-space", status: "active", default_view_refs: [] },
      { id: "module-conserve-archived-root", name: "Module with archived root", root_document_ref: "document-conserve-archived", status: "active", default_view_refs: [] },
      { id: "module-conserve-missing-root", name: "Module with missing root", root_document_ref: "document-conserve-pruned-away", status: "active", default_view_refs: [] },
    ],
  };
  const conservePages = adaptCompanyOsDocsProjection(orphanSpaceProjection, {});
  const placedModuleIds = flattenTree(conservePages.workspace.tree).filter((item) => item.href?.includes("module=")).map((item) => item.ref);
  const withheldModuleIds = (conservePages.workspace.archive?.modules ?? []).map((module) => module.id);
  const declaredModuleIds = orphanSpaceProjection.business_modules.map((module) => module.id);
  check(
    JSON.stringify(sortedIds([...placedModuleIds, ...withheldModuleIds])) === JSON.stringify(sortedIds(declaredModuleIds))
      && placedModuleIds.every((id) => !withheldModuleIds.includes(id))
      && withheldModuleIds.includes("module-conserve-no-space"),
    "every declared BusinessModule appears exactly once across the default tree and the Archive, including one whose space has no root Document",
  );
  const withholdReason = (id) => conservePages.workspace.archive?.modules.find((module) => module.id === id)?.meta;
  check(
    withholdReason("module-conserve-archived-root") === "Root Document is archived"
      && withholdReason("module-conserve-missing-root") === "Root Document is missing from this projection"
      && withholdReason("module-conserve-no-space") === "No navigable space holds this module"
      && new Set([withholdReason("module-conserve-archived-root"), withholdReason("module-conserve-missing-root"), withholdReason("module-conserve-no-space")]).size === 3,
    "the Archive distinguishes archived-root, missing-root, and unplaceable withholding instead of asserting one reason for all",
  );
  check(
    workspace.includes("data-docs-archive-module-reason") && workspace.includes("an archived root Document is not the same withholding as a missing one") && workspace.includes("never in neither"),
    "Docs Workspace archive copy states the conservation invariant and renders each module's own withholding reason",
  );

  // Archived-ness must be the discriminator for authoring, not a missing policy context.
  const authoringActor = { id: "actor-agent-docs-authoring", display_name: "Docs Governance Agent", actor_type: "agent", permission_policy_refs: ["company.records.write"] };
  const authoringDefinitions = [{ id: "definition-authoring-probe", module_id: "module-authoring-probe", action_command_refs: ["document.append", "block.append"], policy_refs: ["definition-authoring-probe:document.append", "definition-authoring-probe:block.append"] }];
  const authoringDocument = (id, lifecycle) => ({ id, space_id: "company", parent_document_id: null, title: `Authoring probe ${lifecycle}`, kind: "page", lifecycle_status: lifecycle, block_ids: [], template_ref: null, permission_policy_refs: ["company.records.write"], reference_refs: [], created_by: { actor_type: "agent", actor_id: authoringActor.id }, updated_by: { actor_type: "agent", actor_id: authoringActor.id }, created_at: "2026-07-20T10:00:00+08:00", updated_at: "2026-07-20T10:00:00+08:00" });
  const authoringProjection = {
    actors: [authoringActor],
    documents: [authoringDocument("document-authoring-active", "draft"), authoringDocument("document-authoring-archived", "archived")],
    business_modules: [{ id: "module-authoring-probe", name: "Authoring probe", root_document_ref: "document-authoring-active", status: "active", default_view_refs: [] }],
    custom_page_definitions: authoringDefinitions,
  };
  const grantedAuthoring = adaptCompanyOsDocsProjection(authoringProjection, { documentId: "document-authoring-active" }).document.authoring;
  const refusedAuthoring = adaptCompanyOsDocsProjection(authoringProjection, { documentId: "document-authoring-archived" }).document.authoring;
  check(
    grantedAuthoring?.documentId === "document-authoring-active"
      && grantedAuthoring?.documentPolicyRef === "definition-authoring-probe:document.append"
      && grantedAuthoring?.blockPolicyRef === "definition-authoring-probe:block.append"
      && refusedAuthoring === undefined,
    "archived lifecycle alone withdraws Store-live authoring: the identical definition, policy refs, and writable actor grant it to the non-archived Document",
  );

  const deepLinkedArchivedPages = adaptCompanyOsDocsProjection({ documents: hierarchyDocuments }, { documentId: "document-agentos-home" });
  check(
    deepLinkedArchivedPages.document.id === "document-agentos-home"
      && deepLinkedArchivedPages.document.lifecycleStatus === "archived"
      && deepLinkedArchivedPages.document.authoring === undefined
      && !flattenTree(deepLinkedArchivedPages.workspace.tree).some((item) => item.ref === "document-agentos-home"),
    "an archived deep link still resolves read-only with its archived lifecycle while staying out of the default tree",
  );
  check(
    deepLinkedArchivedPages.workspace.archive?.documentIds.includes("document-agentos-home")
      && flattenTree(deepLinkedArchivedPages.workspace.archive?.tree).some((item) => item.ref === "document-agentos-home" && item.kind === "document")
      && deepLinkedArchivedPages.document.breadcrumbs?.some((link) => link.id === "document-agentos-home"),
    "an archived deep link stays reachable through the explicit Archive view and keeps its own location trail",
  );
  check(
    document.includes("archivedReason") && document.includes("withdrawn for archived Documents") && document.indexOf("archivedReason") < document.indexOf('"This projection does not expose a CustomPageDefinition'),
    "Document Focus names the archived lifecycle as the true Store-live authoring boundary instead of a false missing-policy reason",
  );
  check(
    workspace.includes('data-docs-archive-narrow="true"') && workspace.includes("lg:hidden"),
    "the Archive stays reachable below desktop widths where the desktop tree rail is hidden",
  );
  check(
    document.includes("lg:border-l lg:border-t-0") && document.includes("border-t border-border pt-5"),
    "Document Focus stacks its context rail below the document body on narrow widths",
  );

  // Navigation context derives from real snapshot relations only: the ancestor
  // chain from parent_document_id, scoped active children, backlinks from
  // Relations/reference_refs, related WorkItems, and maintained-by actors.
  const navigationPages = adaptCompanyOsDocsProjection({
    actors: [
      { id: "actor-agent-nav", display_name: "Nav Agent", actor_type: "agent", permission_policy_refs: ["company.records.write"] },
      { id: "actor-human-nav", display_name: "Nav Human", actor_type: "human" },
    ],
    documents: [
      { id: "document-nav-root", space_id: "company", parent_document_id: null, title: "Nav Root", kind: "page", lifecycle_status: "active", block_ids: [] },
      { id: "document-nav-parent", space_id: "company", parent_document_id: "document-nav-root", title: "Nav Parent", kind: "page", lifecycle_status: "active", block_ids: [] },
      { id: "document-nav-focus", space_id: "company", parent_document_id: "document-nav-parent", title: "Nav Focus", kind: "page", lifecycle_status: "active", block_ids: [], created_by: { actor_type: "agent", actor_id: "actor-agent-nav" }, updated_by: { actor_type: "human", actor_id: "actor-human-nav" } },
      { id: "document-nav-child", space_id: "company", parent_document_id: "document-nav-focus", title: "Nav Child", kind: "page", lifecycle_status: "active", block_ids: [] },
      { id: "document-nav-archived-child", space_id: "company", parent_document_id: "document-nav-focus", title: "Nav Archived Child", kind: "page", lifecycle_status: "archived", block_ids: [] },
      { id: "document-nav-backlink", space_id: "company", parent_document_id: null, title: "Nav Backlink", kind: "page", lifecycle_status: "active", block_ids: [], reference_refs: [{ kind: "document", id: "document-nav-focus" }] },
    ],
    work_items: [{ id: "work-nav-focus", title: "Nav focus work", source_document_ref: "document-nav-focus" }],
    relations: [{ id: "relation-nav-backlink", relation_type: "references", source_ref: "document-nav-backlink", target_ref: "document-nav-focus" }],
  }, { documentId: "document-nav-focus" });
  check(
    navigationPages.document.breadcrumb?.join(" / ") === "company / Nav Root / Nav Parent / Nav Focus"
      && navigationPages.document.breadcrumbs?.map((link) => link.id).join(",") === "document-nav-root,document-nav-parent,document-nav-focus"
      && navigationPages.document.breadcrumbs?.[0]?.href === "?surface=docs&document=document-nav-root"
      && navigationPages.document.breadcrumbs?.[2]?.href === undefined,
    "breadcrumbs derive the full ancestor chain from parent_document_id and stay navigable up to the current Document",
  );
  check(
    JSON.stringify(navigationPages.document.childDocuments?.map((link) => link.id)) === JSON.stringify(["document-nav-child"]),
    "scoped child documents list exactly the active parent_document_id children of the selected Document",
  );
  check(
    navigationPages.document.backlinks?.length === 1 && navigationPages.document.backlinks?.[0]?.id === "document-nav-backlink",
    "backlinks derive from snapshot Relations and reference_refs, deduplicated to each referencing Document",
  );
  check(
    navigationPages.document.resultLinks?.some((link) => link.id === "work-nav-focus")
      && navigationPages.document.properties?.some((property) => property.label === "Last maintained by" && property.ref === "actor-human-nav")
      && navigationPages.document.properties?.some((property) => property.label === "Created by" && property.ref === "actor-agent-nav"),
    "related WorkItems and maintained-by actors derive from real snapshot relations and actor refs",
  );
  check(
    document.includes('data-docs-breadcrumbs="true"') && document.includes("DocumentBreadcrumbs")
      && document.includes('label="Child pages"') && document.includes('label="Backlinks"'),
    "Document Focus renders navigable breadcrumbs, scoped child pages, and backlinks",
  );
  check(
    types.includes("breadcrumbs?: CompanyOsLink[]") && types.includes("childDocuments?: CompanyOsLink[]") && types.includes("backlinks?: CompanyOsLink[]") && types.includes("missingDocumentId?: string"),
    "the document page contract carries breadcrumbs, child documents, backlinks, and the not-found marker",
  );

  // A missing explicit selection is an honest not-found route, not a substitution.
  const missingPages = adaptCompanyOsDocsProjection({
    documents: [{ id: "document-nav-root", space_id: "company", parent_document_id: null, title: "Nav Root", kind: "page", lifecycle_status: "active", block_ids: [] }],
  }, { documentId: "document-pruned-away" });
  check(
    missingPages.document.id === undefined
      && missingPages.document.missingDocumentId === "document-pruned-away"
      && missingPages.document.title === "Document not found"
      && missingPages.document.authoring === undefined
      && missingPages.document.blocks.length === 0
      && adapter.includes("selectionMissed"),
    "a missing explicit document selection renders an explicit not-found state instead of substituting another Document",
  );
  check(
    document.includes('data-docs-document-not-found="true"') && document.includes("nothing else is substituted under the requested id"),
    "Document Focus renders the not-found state without fabricating document content",
  );

  // A long document renders every native Block in Document.block_ids order and
  // raises the existing oversized-document health signal.
  const longBlockIds = Array.from({ length: 60 }, (_, index) => `block-long-${String(index).padStart(2, "0")}`);
  const longPages = adaptCompanyOsDocsProjection({
    documents: [{ id: "document-long", space_id: "company", parent_document_id: null, title: "Long document", kind: "page", lifecycle_status: "active", block_ids: longBlockIds }],
    blocks: longBlockIds.map((id, index) => ({ id, document_id: "document-long", kind: "rich_text", position: 59 - index, content: { text: `Paragraph ${index}` } })),
  }, { documentId: "document-long" });
  check(
    longPages.document.blocks.length === 60
      && longPages.document.blocks[0]?.id === "block-long-00"
      && longPages.document.blocks[59]?.id === "block-long-59"
      && longPages.health.findings.some((finding) => finding.kind === "oversized_document" && finding.subject?.id === "document-long"),
    "a long document renders every native Block in Document.block_ids order and flags the oversized-document health signal",
  );

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
      ...flattenTree(pages.workspace.tree).map((item) => item.ref),
      ...(pages.workspace.recentlyUpdated ?? []).map((link) => link.id),
      ...(pages.workspace.suggestions ?? []).map((link) => link.id),
      pages.workspace.proposal?.id,
    ].filter(Boolean)),
    "document-focus": new Set([
      pages.document.id,
      ...flattenTree(pages.document.documentTree).map((item) => item.ref),
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
    "document-health": new Set([
      ...(pages.health.structureLinks ?? []).map((link) => link.id),
      ...(pages.health.governanceAgent ? [pages.health.governanceAgent.id] : []),
      ...pages.health.findings.flatMap((finding) => [
        finding.subject?.id,
        finding.related?.id,
        ...(finding.affected ?? []).map((link) => link.id),
      ]),
    ].filter(Boolean)),
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
