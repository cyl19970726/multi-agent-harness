#!/usr/bin/env node

/**
 * Docs CLI surface smoke (v2 era, retirement stage R3).
 *
 * Static mirror assertions: the AI-first Docs v2 page command surface exists,
 * the record-layer commands survive, and the Block-era document/block/template
 * command tree plus the document.append/block.append API actions are gone.
 * Behavioral assertions live in check-company-os-docs-v2-smoke.mjs (CLI),
 * check-company-os-docs-v2-api.mjs (serve), and the dashboard v2 checks.
 */

import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(fileURLToPath(import.meta.url), "..", "..");
const read = (rel) => readFile(join(repoRoot, rel), "utf8");

const mainSource = await read("crates/harness-cli/src/main.rs");
const docsV2Source = await read("crates/harness-cli/src/docs_v2_page.rs");
const apiSource = await read("crates/harness-cli/src/company_os_api.rs");
const skillSource = await read("skills/company-docs-operator/SKILL.md");
const cliMap = await read("docs/cli-map.md");
const spec = await read("docs/company-os/ai-first-docs-spec.md");

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

// --- v2 page command surface (docs_v2_page.rs) -----------------------------
for (const fn of [
  "fn page_create_command",
  "fn page_read_command",
  "fn page_write_command",
  "fn page_append_command",
  "fn page_search_command",
  "fn page_rename_command",
  "fn page_move_command",
  "fn page_archive_command",
]) {
  check(docsV2Source.includes(fn), `v2 page surface implements ${fn.replace("fn ", "")}`);
}
check(docsV2Source.includes("pub struct PageReadOptions"), "v2 page read keeps scoped-read options (scope/detail/revision)");
check(docsV2Source.includes("legacy_block_to_v2"), "v2 page read keeps the legacy read-only projection mapping");
check(docsV2Source.includes("parent cycle"), "v2 page move keeps parent-cycle rejection");
check(docsV2Source.includes("--confirm"), "v2 page archive keeps the --confirm gate");

// --- main.rs dispatch -------------------------------------------------------
check(
  mainSource.includes("company docs page create|read|write|append|search|rename|move|archive"),
  "main.rs usage lists the full v2 page verb set",
);
check(
  mainSource.includes("fn company_docs_typed_record_append_command") &&
    mainSource.includes("fn company_docs_view_create_command") &&
    mainSource.includes("fn company_docs_relation_link_command") &&
    mainSource.includes("fn company_docs_module_create_command"),
  "record-layer commands survive (typed-record/view/relation/module)",
);
check(
  mainSource.includes("fn company_docs_query_command") &&
    mainSource.includes("fn company_docs_health_command") &&
    mainSource.includes("fn company_docs_source_sync_command"),
  "read/health/source-sync commands survive",
);

// --- Block-era command tree is gone ----------------------------------------
for (const dead of [
  "fn company_docs_document_create_command",
  "fn company_docs_document_rename_command",
  "fn company_docs_document_move_command",
  "fn company_docs_document_archive_command",
  "fn company_docs_template_create_command",
  "fn company_docs_template_status_command",
  "fn company_docs_block_append_command",
  "fn company_docs_block_update_command",
  "fn company_docs_block_archive_command",
  "fn company_docs_block_remove_command",
  "fn company_docs_block_reorder_command",
]) {
  check(!mainSource.includes(dead), `Block-era command removed: ${dead.replace("fn company_docs_", "").replace("_command", "")}`);
}

// --- API actions retired -----------------------------------------------------
check(!apiSource.includes('"document.append"'), "document.append action removed from the serve API");
check(!apiSource.includes('"block.append"'), "block.append action removed from the serve API");
check(!apiSource.includes("validate_document_append") && !apiSource.includes("validate_block_append"), "document/block append validators removed");
check(
  apiSource.includes('"typed_record.append"') &&
    apiSource.includes('"view.append"') &&
    apiSource.includes('"relation.append"'),
  "record-layer governed actions survive (typed_record/view/relation)",
);
check(apiSource.includes("/v1/company-os/docs-v2/pages"), "v2 page endpoints remain registered");

// --- Skill contract follows the v2 surface ----------------------------------
check(skillSource.includes("page read") && skillSource.includes("page write"), "operator skill documents the v2 page commands");
check(!/\bdocument (rename|move|archive)\b/.test(skillSource), "operator skill no longer teaches Block-era document maintenance");

// --- Docs coherence -----------------------------------------------------------
check(!cliMap.includes("Blocks (Block-era)"), "cli-map no longer lists the Block-era block rows");
check(cliMap.includes("page rename") && cliMap.includes("page move") && cliMap.includes("page archive"), "cli-map lists the v2 metadata commands");
check(spec.includes("R3 (done)"), "spec §13 marks R3 done");

console.log(
  failed === 0
    ? `\ncompany-os docs CLI surface smoke (v2 era): ${passed} checks passed`
    : `\ncompany-os docs CLI surface smoke (v2 era): ${failed} failure(s)`,
);
process.exit(failed === 0 ? 0 : 1);
