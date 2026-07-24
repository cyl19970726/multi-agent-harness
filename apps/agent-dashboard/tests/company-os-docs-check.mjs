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
  const [index, workspace, document, structured, home, relation, health, healthAction, documentAction, adapter] = await Promise.all([
    source("index.ts"), source("DocsWorkspace.tsx"), source("BasicDocumentPage.tsx"),
    source("StructuredDocumentView.tsx"), source("CompanyHome.tsx"), source("RelationChips.tsx"), source("DocumentHealthReview.tsx"), source("healthAction.ts"), source("documentAction.ts"), source("fixtureAdapter.ts"),
  ]);

  check(index.includes("DocsWorkspace") && index.includes("BasicDocumentPage") && index.includes("StructuredDocumentView") && index.includes("CompanyHome") && index.includes("DocumentHealthReview"), "public Docs API exports all five Company OS Docs surfaces");
  check(index.includes("buildDocsTypedRecordCommand") && index.includes("buildDocsViewCommand") && index.includes("buildDocsRelationCommand") && index.includes("buildDocsReorderBlocksCommand"), "public Docs API exports Store-live module authoring command builders");
  check(workspace.includes('data-company-os-page="docs-workspace"') && document.includes('data-company-os-page="document-focus"') && structured.includes('data-company-os-page="business-module-focus"') && home.includes('data-company-os-page="home"') && health.includes('data-company-os-page="document-health"'), "capture-ready page markers identify each Docs surface");
  check([workspace, document, structured, home, health].every((file) => file.includes('data-company-os-ready="true"')), "every Docs root exposes a ready marker");
  check(structured.includes("availableViews") && structured.includes("fallback") && structured.includes("BoardView") && structured.includes("TimelineView"), "structured view exposes standard table, board, timeline, and fallback paths");
  check(structured.includes("StandardViewProvenance") && structured.includes('data-docs-standard-view-provenance="true"') && structured.includes("View is presentation, not a second truth"), "structured view exposes provenance for module scope, native View, source kinds, query, and record count");
  check(structured.includes("StandardViewConfiguration") && structured.includes('data-docs-standard-view-configuration="true"') && structured.includes("Configuration is stored in native View.query") && structured.includes('aria-label="View filter field"') && structured.includes('aria-label="View group by"'), "structured view exposes saved View configuration and Store-live View query authoring controls");
  check(structured.includes("CustomPageContractCard") && structured.includes('data-docs-custom-page-contract="true"') && structured.includes('data-docs-custom-page-status') && structured.includes("data-docs-custom-page-active-package") && structured.includes("data-docs-custom-page-latest-package") && structured.includes("data-docs-custom-page-boundary"), "structured view exposes code-declared custom page contract, package state markers, and source-of-truth boundary");
  check(structured.includes('data-docs-standard-view-empty="true"') && structured.includes("declared query returned no records") && structured.includes("does not delete the BusinessModule"), "structured view empty state is explicit without fabricating module truth");
  check(structured.includes('data-docs-authoring-panel="business-module-focus"') && structured.includes("buildDocsTypedRecordCommand") && structured.includes("buildDocsViewCommand") && structured.includes("buildDocsRelationCommand"), "Structured module view exposes Store-live TypedRecord, View, and Relation authoring controls");
  check(document.includes("SimpleTable") && document.includes("RelationChips") && document.includes("sourceLinks") && document.includes("resultLinks"), "basic document supports tables, relation chips, source, and result links");
  check(document.includes("data-docs-authoring-panel=\"document-focus\"") && document.includes("buildDocsChildDocumentCommand") && document.includes("buildDocsAppendBlockCommands") && document.includes("Document.block_ids"), "Document Focus exposes Store-live child Document and Block authoring controls");
  check(document.includes('aria-label="Child document template"') && document.includes("templateOptions") && document.includes("childTemplateRef"), "Document Focus exposes template provenance selection for child Documents");
  check(document.includes("buildDocsInstantiateTemplateBlockCommands") && document.includes('aria-label="Instantiate template blocks"') && document.includes('data-docs-template-instantiation="browser-action"'), "Document Focus exposes Store-live opt-in template Block instantiation controls");
  check(document.includes('data-docs-block-composer="true"') && document.includes("data-docs-block-kind-option") && document.includes("data-docs-block-composer-hint"), "Document Focus exposes a Notion-like governed Block composer with type affordances and durable-action hinting");
  check(document.includes('data-docs-slash-menu="true"') && document.includes('aria-label="Slash menu block commands"') && document.includes("data-docs-slash-command") && document.includes("/heading"), "Document Focus exposes a slash-menu affordance for governed Block type selection");
  check(document.includes('data-docs-block-order-boundary="true"') && document.includes("Document.block_ids sequence") && document.includes("data-docs-block-reorder") && document.includes("governed document.append update"), "Document Focus exposes native block order and governed reorder controls");
  check(document.includes("data-docs-authoring-error-boundary") && document.includes("role=\"status\"") && document.includes("server validates definition, policy, actor permission"), "Document Focus exposes governed authoring error and permission feedback boundary");
  check(document.includes('data-docs-empty-document="true"') && document.includes("data-docs-template-provenance") && document.includes("template Blocks are copied only by an explicit governed instantiation action"), "Document Focus surfaces empty document and template provenance states without fabricating content");
  check(document.includes("data-docs-template-record-policy") && document.includes("Template Blocks do not create records") && document.includes("Use a governed Relation after the child Document and TypedRecord exist"), "Document Focus exposes template-to-TypedRecord relation boundary during child Document creation");
  check(document.includes('aria-label="Block kind"') && document.includes('value: "heading"') && document.includes('value: "callout"') && document.includes('value: "table"'), "Document Focus exposes structured Block authoring controls");
  check(!document.includes("key={property.label}") && document.includes("property.ref ?? \"property\""), "repeated property labels use a stable React key rather than a duplicate display label");
  check(home.includes("Review decision") && home.includes("decisionRequester") && home.includes("decisionCollaborators"), "Home gives the pending decision a first-viewport review action and structured responsibility context");
  check(home.includes("Button asChild") && home.includes("data.decisionRequired.href") && home.includes("disabled"), "Home renders a real approval link without a callback and never leaves an enabled no-op CTA");
  check(adapter.includes("adaptCompanyOsDocsProjection") && adapter.includes("financialRecordType"), "projection adapter maps financial type from an explicit record field");
  check(adapter.includes("buildDocumentHealthData") && adapter.includes("missing_document_record_relation") && adapter.includes("No deletion without governed action") === false, "projection adapter computes document health without embedding UI policy copy");
  check(workspace.includes('data-docs-template-library="true"') && workspace.includes("data-docs-template-block-count") && workspace.includes("template_ref only") && workspace.includes("copy Blocks via Actions"), "Docs Workspace exposes a native template library with provenance and instantiation boundaries");
  check(workspace.includes("data-docs-template-lifecycle") && workspace.includes("harness company docs template status") && workspace.includes("archiving a template does not mutate existing Documents"), "Docs Workspace exposes template lifecycle state and governed status boundary");
  check(adapter.includes("template-create") && adapter.includes("harness company docs template create") && adapter.includes("--from-document <source-doc-id>"), "Docs Workspace command panel exposes reusable template creation without treating existing pages as mutable templates");
  check(adapter.includes("template-status") && adapter.includes("harness company docs template status") && adapter.includes("active|paused|archived"), "Docs Workspace command panel exposes governed template lifecycle updates");
  check(workspace.includes("data-docs-template-record-policy") && workspace.includes("Template → TypedRecord policy") && workspace.includes("Template instantiation never creates TypedRecords or Relations"), "Docs Workspace exposes template-to-TypedRecord relation policy without hidden record creation");
  check(workspace.includes('data-docs-workspace-search="projection"') && workspace.includes("Search projection-backed Docs workspace") && workspace.includes('data-docs-workspace-search-boundary="projection-only"') && workspace.includes("filteredSpaces") && workspace.includes("filteredTemplates") && workspace.includes("filteredRecent"), "Docs Workspace filters spaces, templates, and recent records from the current projection without claiming global search");
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
  check(pages.document.sourceLinks?.[0]?.label === "Trademark application CN-2026-018" && pages.document.resultLinks?.[0]?.label === "Trademark filing for Brand A", "fixture adapter preserves source and WorkItem provenance");
  check(pages.home.decisionActor?.name === "Brand Owner" && pages.home.financeSummary[0]?.value === "¥3,000" && pages.home.financeSummary[0]?.financialRecordType === "commitment", "home preserves the human decision and pending-commitment distinction");
  check(pages.home.decisionRequired?.href === "?surface=approvals&approval=approval-trademark-filing-fee-cn-2026-018", "projection adapter supplies the Home review CTA with the selected approval route");
  check(!/^[a-z][a-z0-9]*(?:[._:-][a-z0-9-]+)+$/i.test(pages.home.decisionRequired?.label ?? "") && pages.home.decisionRequired?.label !== pages.home.decisionSummary && pages.home.decisionRequester?.label === "Trademark Agent" && (pages.home.decisionCollaborators?.length ?? 0) > 0, "Home derives a readable non-duplicated approval prompt with grouped requester and collaborators");
  check(["actor-agent-content-strategy", "actor-external-lawyer"].every((id) => pages.home.decisionCollaborators?.some((actor) => actor.id === id)), "Home contributor selection retains projection-backed strategy and external legal collaborators without broad actor dumping");
  const documentHeadings = pages.document.blocks.filter((block) => block.type === "heading").map((block) => block.content);
  const documentTables = pages.document.blocks.filter((block) => block.type === "table").map((block) => block.table.caption);
  check(pages.workspace.rootSelected === true && !pages.workspace.tree.flatMap((item) => item.children ?? []).some((item) => item.selected), "Docs workspace selection remains on the Company workspace root");
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
  const alternatePages = adaptCompanyOsDocsProjection({
    documents: [{ id: "document-live-1", title: "Live operating brief", space: "Operations" }],
    typed_records: [{ id: "record-live-1", record_type: "Initiative", source_document_ref: "document-live-1" }],
    work_items: [{ id: "work-live-1", title: "Prepare live brief", source_document_ref: "document-live-1" }],
  });
  check(alternatePages.document.id === "document-live-1" && alternatePages.document.title === "Live operating brief" && alternatePages.home.changes.every((link) => !/trademark|brand a/i.test(link.label)), "a different live projection maps only its supplied records");
  const customPagePages = adaptCompanyOsDocsProjection({
    actors: [{ id: "human-docs-owner", actor_type: "human", display_name: "Docs Owner" }],
    documents: [{ id: "document-custom-root", title: "Custom Root", space_id: "company", block_ids: [] }],
    typed_records: [{ id: "record-custom-1", record_type: "TrademarkApplication", module_id: "module-custom-page", title: "Custom Application" }],
    views: [{ id: "view-custom-fallback", module_id: "module-custom-page", title: "Fallback table", mode: "table", source_kinds: ["typed_record"], query: { filters: [{ field: "module_id", value: "module-custom-page" }] } }],
    business_modules: [{ id: "module-custom-page", name: "Custom Page Module", root_document_ref: "document-custom-root", status: "active", default_view_refs: ["view-custom-fallback"], custom_page_definition_refs: ["page-custom-module"] }],
    custom_page_definitions: [{
      id: "page-custom-module",
      module_id: "module-custom-page",
      purpose: "Render a governed custom page over module records.",
      allowed_data_queries: [{ id: "query-custom-module", source_kind: "business_module", source_scope: "module-custom-page", permission_policy_ref: "company.records.write" }],
      approved_ui_components: ["CodeDeclaredPage", "VisualContractReview"],
      action_command_refs: ["typed_record.append", "view.append", "relation.append"],
      standard_view_fallback_ref: "view-custom-fallback",
      owner: { actor_type: "human", actor_id: "human-docs-owner" },
      package_ref: "package-custom-active",
      package_version: "1.0.0",
      fixture_ref: "docs/design/company-os/custom-pages/custom/fixture.json",
      visual_contract_ref: "docs/design/company-os/custom-pages/custom/review.html",
      policy_refs: ["page-custom-module:typed_record.append", "page-custom-module:view.append", "page-custom-module:relation.append"],
      created_at: "2026-07-24T10:00:00+08:00",
      updated_at: "2026-07-24T10:00:00+08:00",
    }],
    custom_page_packages: [
      { id: "package-custom-active", definition_id: "page-custom-module", version: "1.0.0", kind: "react", artifact_ref: "apps/agent-dashboard/src/company-os/modules/custom/CustomPage.tsx", entrypoint: "index.tsx", integrity_digest: "sha256:active", built_at: "2026-07-24T10:00:00+08:00" },
      { id: "package-custom-candidate", definition_id: "page-custom-module", version: "1.0.1", kind: "react", artifact_ref: "apps/agent-dashboard/src/company-os/modules/custom/CustomPage.tsx", entrypoint: "index.tsx", integrity_digest: "sha256:candidate", built_at: "2026-07-24T11:00:00+08:00" },
    ],
  }, { moduleId: "module-custom-page" });
  check(
    customPagePages.moduleView.customPage?.status === "candidate_recorded" &&
      customPagePages.moduleView.customPage.activeVersion === "1.0.0" &&
      customPagePages.moduleView.customPage.latestVersion === "1.0.1" &&
      customPagePages.moduleView.customPage.fallbackViewId === "view-custom-fallback" &&
      customPagePages.moduleView.customPage.declaredActions.includes("typed_record.append") &&
      customPagePages.moduleView.customPage.allowedQueries.some((query) => query.includes("query-custom-module")),
    "projection adapter exposes CustomPageDefinition active package, candidate package, declared query/action scopes, and fallback View without changing source truth",
  );
  check([workspace, document, structured, home, relation, health].every((file) => file.includes("data-company-os-ref")) && relation.includes("data-financial-record-type") && home.includes("data-actor-type"), "visible Docs, record, finance, and actor nodes propagate semantic references");

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
