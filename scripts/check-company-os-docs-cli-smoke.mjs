#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(fileURLToPath(import.meta.url), "..", "..");
const mainPath = join(repoRoot, "crates", "harness-cli", "src", "main.rs");
const source = await readFile(mainPath, "utf8");
const docsOperatorSkill = await readFile(join(repoRoot, "skills", "company-docs-operator", "SKILL.md"), "utf8");
const docsOperatorAgent = await readFile(join(repoRoot, "skills", "company-docs-operator", "agents", "openai.yaml"), "utf8");
const docsSurfaceMatrix = await readFile(join(repoRoot, "docs", "company-os", "docs-operating-surface-matrix.md"), "utf8");
const documentSystem = await readFile(join(repoRoot, "docs", "company-os", "document-system.md"), "utf8");
const decisionsIndex = await readFile(join(repoRoot, "docs", "decisions", "README.md"), "utf8");
const sqlReadModelAdr = await readFile(join(repoRoot, "docs", "decisions", "0030-company-os-sql-read-model.md"), "utf8");
const agentOperatedDocsAdr = await readFile(join(repoRoot, "docs", "decisions", "0031-agent-operated-docs-and-code-declared-pages.md"), "utf8");
const healthStart = source.indexOf("fn company_docs_health_command");
const healthEnd = source.indexOf("fn company_docs_module_create_command");
const healthSource = healthStart >= 0 && healthEnd > healthStart ? source.slice(healthStart, healthEnd) : "";
const queryStart = source.indexOf("fn company_docs_query_command");
const queryEnd = source.indexOf("fn company_docs_health_command");
const querySource = queryStart >= 0 && queryEnd > queryStart ? source.slice(queryStart, queryEnd) : "";
const moduleStart = source.indexOf("fn company_docs_module_create_command");
const moduleEnd = source.indexOf("fn company_docs_page_definition_create_command");
const moduleSource = moduleStart >= 0 && moduleEnd > moduleStart ? source.slice(moduleStart, moduleEnd) : "";
const pageDefinitionStart = source.indexOf("fn company_docs_page_definition_create_command");
const pageDefinitionEnd = source.indexOf("fn company_docs_document_create_command");
const pageDefinitionSource = pageDefinitionStart >= 0 && pageDefinitionEnd > pageDefinitionStart ? source.slice(pageDefinitionStart, pageDefinitionEnd) : "";
const blockStart = source.indexOf("fn company_docs_block_append_command");
const blockEnd = source.indexOf("fn company_docs_typed_record_append_command");
const blockSource = blockStart >= 0 && blockEnd > blockStart ? source.slice(blockStart, blockEnd) : "";
const typedRecordStart = source.indexOf("fn company_docs_typed_record_append_command");
const typedRecordEnd = source.indexOf("fn company_docs_view_create_command");
const typedRecordSource = typedRecordStart >= 0 && typedRecordEnd > typedRecordStart ? source.slice(typedRecordStart, typedRecordEnd) : "";
const viewStart = source.indexOf("fn company_docs_view_create_command");
const viewEnd = source.indexOf("fn company_docs_relation_link_command");
const viewSource = viewStart >= 0 && viewEnd > viewStart ? source.slice(viewStart, viewEnd) : "";

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

check(source.includes('"company" => company_command'), "top-level harness company command is routed");
check(source.includes('company docs query') && source.includes('company docs search') && source.includes('company docs traverse') && source.includes('company docs refs') && source.includes('company docs related') && source.includes('company docs health') && source.includes('company docs module create') && source.includes('company docs page scaffold') && source.includes('company docs page verify') && source.includes('company docs page publish') && source.includes('company docs page-definition create') && source.includes('company docs document create') && source.includes('company docs document rename') && source.includes('company docs document move') && source.includes('company docs document archive') && source.includes('company docs template create') && source.includes('company docs template status') && source.includes('company docs block append') && source.includes('company docs block update') && source.includes('company docs block archive') && source.includes('company docs block remove') && source.includes('company docs block reorder') && source.includes('company docs typed-record append') && source.includes('company docs typed-record update') && source.includes('company docs typed-record validate') && source.includes('company docs view create') && source.includes('company docs view update') && source.includes('company docs relation link') && source.includes('company docs relation unlink') && source.includes('company docs relation relink') && source.includes('company docs diff') && source.includes('company docs snapshot') && source.includes('company docs change-report'), "help and usage expose Docs query/search/traversal, page contracts, document/block/record/view/relation maintenance, diff, snapshot, and change-report commands");
check(querySource.includes("fn company_docs_query_command") && querySource.includes("--document") && querySource.includes("--module") && querySource.includes("latest_projection") && querySource.includes("future_sql_role"), "Docs query command reads one Document or module context from latest projections and preserves the future SQL boundary");
check(querySource.includes("fn company_docs_search_command") && querySource.includes("fn company_docs_traverse_command") && querySource.includes("fn company_docs_refs_command") && querySource.includes("fn company_docs_related_command") && querySource.includes("company_docs_read_boundaries"), "Docs search/traverse/refs/related expose Agent-readable projection context without writes");
check(["document", "blocks", "children", "templates", "typed_records", "relations", "views", "business_module", "health_findings", "available_commands", "boundaries"].every((field) => querySource.includes(`\"${field}\"`)), "Docs query returns the stable Agent-facing operating context fields");
check(querySource.includes("work_side_effects") && querySource.includes("finance_side_effects") && querySource.includes("organization_side_effects") && querySource.includes("execution_side_effects") && !querySource.includes("handle_post"), "Docs query is read-only and declares no Work, Finance, Organization, or Execution side effects");
check(healthSource.includes("fn company_docs_health_command") && !/GoalPhase|Task Graph|compat-goal/.test(healthSource), "Docs health command is implemented without legacy Goal/Task terminology");
check(source.includes('"missing_doc-record"') === false && source.includes('"missing_document_record_relation"'), "Docs health surfaces native missing Document-to-TypedRecord Relation findings");
check(moduleSource.includes("fn company_docs_module_create_command") && moduleSource.includes('"/v1/company-os/business-modules"') && moduleSource.includes('"/v1/company-os/views"') && moduleSource.includes("--relation-rule-json"), "Docs module create builds a governance-scoped BusinessModule, fallback View, and optional relation rules");
check(pageDefinitionSource.includes("fn company_docs_page_definition_create_command") && pageDefinitionSource.includes('"/v1/company-os/custom-page-packages"') && pageDefinitionSource.includes('"/v1/company-os/custom-page-definitions"') && pageDefinitionSource.includes("action_command_refs"), "Docs page-definition create installs a package and CustomPageDefinition policy bundle");
check(pageDefinitionSource.includes("fn company_docs_page_scaffold_command") && pageDefinitionSource.includes("CodeDeclaredPage") && pageDefinitionSource.includes("page_is_not_second_truth") && pageDefinitionSource.includes("fn company_docs_page_verify_command") && pageDefinitionSource.includes("fn company_docs_page_publish_command"), "Docs page scaffold/verify/publish model code-declared custom pages as governed PageDefinition/PagePackage metadata");
check(source.includes("fn company_docs_document_create_command") && source.includes('"document.append"') && source.includes("parent_document_id") && source.includes("--template") && source.includes("--instantiate-template"), "Docs document create builds scoped child Document records through document.append and can preserve or instantiate template provenance");
check(source.includes("fn company_docs_document_rename_command") && source.includes("fn company_docs_document_move_command") && source.includes("fn company_docs_document_archive_command") && source.includes("--dry-run") && source.includes("document archive requires --confirm") && source.includes("company_docs_document_update_command"), "Docs document rename/move/archive are governed structure maintenance commands with dry-run and archive confirmation");
check(source.includes("fn company_docs_template_create_command") && source.includes('"kind": "template"') && source.includes("--from-document") && source.includes("company_docs_copy_document_blocks"), "Docs template create builds reusable template Documents and can copy source Blocks through governed Actions");
check(source.includes("fn company_docs_template_status_command") && source.includes("DocumentKind::Template") && source.includes("--status must be one of draft|active|paused|archived") && source.includes('"document.append"'), "Docs template status updates only template Document lifecycle through governed document.append");
check(blockSource.includes("fn company_docs_block_append_command") && blockSource.includes('"block.append"') && blockSource.includes('"document.append"'), "Docs block append creates a Block and then updates Document.block_ids through governed Actions");
check(blockSource.includes("block_ids.push") && blockSource.includes('document_record["block_ids"]'), "Docs block append preserves the Document-to-Block navigation invariant");
check(blockSource.includes("--kind") && blockSource.includes("--content-json") && blockSource.includes("--text"), "Docs block append supports structured Block kind/content as well as text shorthand");
check(blockSource.includes("fn company_docs_block_update_command") && blockSource.includes("fn company_docs_block_archive_command") && blockSource.includes("fn company_docs_block_remove_command") && blockSource.includes("block archive requires --confirm") && blockSource.includes("block remove requires --confirm") && blockSource.includes("physical_delete") && blockSource.includes("_archived"), "Docs block update/archive/remove preserve Block identity, support dry-run, require confirmation for removal from view, and avoid physical delete");
check(blockSource.includes("fn company_docs_block_reorder_command") && blockSource.includes("--block-order") && blockSource.includes("existing Document.block_ids set") && blockSource.includes('"document.append"'), "Docs block reorder preserves the exact native Document.block_ids set through governed document.append");
check(typedRecordSource.includes("fn company_docs_typed_record_append_command") && typedRecordSource.includes('"typed_record.append"') && typedRecordSource.includes("source_document_ref"), "Docs typed-record append builds scoped TypedRecord records from a source Document");
check(typedRecordSource.includes("fn company_docs_typed_record_update_command") && typedRecordSource.includes("--merge-fields") && typedRecordSource.includes("--dry-run") && typedRecordSource.includes('"kind": "typed_record"'), "Docs typed-record update preserves identity/source and supports field merge plus dry-run");
check(typedRecordSource.includes("fn company_docs_typed_record_validate_command") && typedRecordSource.includes("missing_required_field") && typedRecordSource.includes("field_type_mismatch") && typedRecordSource.includes("module_schema_persistence"), "Docs typed-record validate provides the first schema-validation slice without persisting a fake module schema");
check(viewSource.includes("fn company_docs_view_create_command") && viewSource.includes('"view.append"') && viewSource.includes('"business_module"'), "Docs view create builds scoped View records under a BusinessModule subject");
check(viewSource.includes("fn company_docs_view_update_command") && viewSource.includes("view_is_presentation_truth_not_record_store") && viewSource.includes("--dry-run"), "Docs view update maintains saved View/query configuration without becoming a second record store");
check(source.includes('command_name": "relation.append"') && source.includes('"/v1/company-os/actions/dispatch"'), "Docs relation link builds relation.append and uses the governed Action dispatcher");
check(source.includes("fn company_docs_relation_unlink_command") && source.includes("relation unlink requires --confirm") && source.includes('"lifecycle_status": "active"') && source.includes('"lifecycle_status"') && source.includes('"archived"') && source.includes("json_relation_is_active"), "Docs relation unlink archives the latest Relation row, requires confirmation, and active query/health filters ignore archived relations");
check(source.includes("fn company_docs_relation_relink_command") && source.includes("two_governed_relation_append_actions") && source.includes("relation relink requires --confirm"), "Docs relation relink is an explicit governed archive-plus-link cleanup action");
check(source.includes("fn company_docs_diff_command") && source.includes("fn company_docs_snapshot_command") && source.includes("fn company_docs_change_report_command") && source.includes("rollback_evidence_only"), "Docs diff/snapshot/change-report provide review evidence without dispatching mutations");
check(!source.includes("append_relation") && !source.includes("append_block") && source.includes("company_os_api::handle_post"), "Docs authoring commands use the Company OS API instead of direct ledger appends");
check(source.includes("HARNESS_COMPANY_OS_TOKEN") || source.includes("authenticate_write_transport"), "Docs write commands remain behind the Company OS capability");
check(source.includes('"risk_tier": "r1"') && source.includes('"requires_human_approval": false'), "Docs relation link uses the low-risk Relation repair policy shape");
check(docsOperatorSkill.includes("name: company-docs-operator") && docsOperatorAgent.includes("Company Docs Operator"), "Company Docs Operator skill is discoverable with Codex agent metadata");
check(["query", "search", "traverse", "refs", "related", "health", "module create", "page scaffold", "page verify", "page publish", "page-definition create", "document create", "document rename", "document move", "document archive", "template create", "template status", "block append", "block update", "block archive", "block remove", "block reorder", "typed-record append", "typed-record update", "typed-record validate", "view create", "view update", "relation link", "relation unlink", "relation relink", "diff", "snapshot", "change-report"].every((command) => docsOperatorSkill.includes(`harness company docs ${command}`)), "Company Docs Operator skill covers every implemented Docs CLI command");
check(["Docs may reference", "Work, Organization, Finance", "Execution records", "Never infer approval", "payment", "organization authority", "executor"].every((token) => docsOperatorSkill.includes(token)), "Company Docs Operator skill preserves system truth boundaries");
check(docsOperatorSkill.includes("--kind callout") && docsOperatorSkill.includes("--content-json") && docsOperatorSkill.includes("Document.block_ids"), "Company Docs Operator skill documents structured Block authoring and navigation invariant");
check(docsOperatorSkill.includes("--template <template-document-id>") && docsOperatorSkill.includes("--instantiate-template") && docsOperatorSkill.includes("Document.template_ref") && docsOperatorSkill.includes("does not create TypedRecords"), "Company Docs Operator skill documents template provenance and opt-in Block instantiation boundaries");
check(["Docs Workspace", "Document Focus", "Business Module Focus", "Document Health Review"].every((surface) => docsSurfaceMatrix.includes(surface)), "Docs operating surface matrix covers all four Docs operating surfaces");
check(["query", "search", "traverse", "refs", "related", "health", "module create", "page scaffold", "page verify", "page publish", "page-definition create", "document create", "document rename", "document move", "document archive", "template create", "template status", "block append", "block update", "block archive", "block remove", "block reorder", "typed-record append", "typed-record update", "typed-record validate", "view create", "view update", "relation link", "relation unlink", "relation relink", "diff", "snapshot", "change-report"].every((command) => docsSurfaceMatrix.includes(`harness company docs ${command}`)), "Docs operating surface matrix covers every implemented Docs CLI command");
check(["Document", "Block", "TypedRecord", "Relation", "View", "BusinessModule"].every((object) => docsSurfaceMatrix.includes(`\`${object}\``)), "Docs operating surface matrix names the native Docs objects");
check(["Store-live", "company-docs-operator", "visual contract", "Current gaps", "No Docs page, CLI command, or skill may infer approval"].every((token) => docsSurfaceMatrix.includes(token)), "Docs operating surface matrix records UI/skill/visual evidence and cross-system boundaries");
check(documentSystem.includes("Agent-operated, Human-reviewed") && /authoritative machine\s+interface is CLI\/API/.test(documentSystem) && docsSurfaceMatrix.includes("Agent primary interface: CLI/API") && docsSurfaceMatrix.includes("CLI-first backlog"), "Docs contracts preserve Agent-operated, Human-reviewed and CLI-first posture");
check(decisionsIndex.includes("0030-company-os-sql-read-model.md") && documentSystem.includes("ADR 0030") && docsSurfaceMatrix.includes("SQL is introduced only as a derived read/query/index layer") && sqlReadModelAdr.includes("Do **not** replace the canonical Company OS Store with SQL now") && sqlReadModelAdr.includes("JSONL ledgers remain canonical"), "Docs storage contract preserves SQL as a derived read/query/index layer, not the current canonical Store");
check(decisionsIndex.includes("0031-agent-operated-docs-and-code-declared-pages.md") && documentSystem.includes("ADR 0031") && docsSurfaceMatrix.includes("code-declared custom business pages") && agentOperatedDocsAdr.includes("Docs is not a Notion editor clone") && agentOperatedDocsAdr.includes("PageDefinition") && agentOperatedDocsAdr.includes("PagePackage"), "Docs product contract preserves Agent-operated substrate plus code-declared custom pages");
check(docsSurfaceMatrix.includes("template_ref") && docsSurfaceMatrix.includes("--instantiate-template") && docsSurfaceMatrix.includes("template-to-typed-record relation policy"), "Docs operating surface matrix distinguishes template provenance, Block instantiation, and remaining template gaps");

console.log(`\nCompany OS Docs CLI smoke: ${passed} pass, ${failed} fail`);
process.exit(failed === 0 ? 0 : 1);
