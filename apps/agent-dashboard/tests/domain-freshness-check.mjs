#!/usr/bin/env node

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const { default: ts } = await import("typescript");
const directory = await mkdtemp(join(tmpdir(), "dashboard-domain-freshness-"));
let passed = 0;
let failed = 0;
const check = (condition, message) => {
  console.log(`  ${condition ? "PASS" : "FAIL"}  ${message}`);
  if (condition) passed += 1; else failed += 1;
};

try {
  const source = await readFile(join(here, "..", "src", "app", "freshness.ts"), "utf8");
  const js = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText.replace('import type { ProjectionInvalidation } from "../api";\n', "");
  const output = join(directory, "freshness.mjs");
  await writeFile(output, js, "utf8");
  const { freshnessDomainsForInvalidation, uniformFreshness, updateFreshness } =
    await import(pathToFileURL(output).href);

  const invalidation = (scope, ledger) => ({
    scope, ledger, scope_id: "scope", revision: 1, reason: "append", stream_epoch: "epoch",
  });
  check(
    freshnessDomainsForInvalidation(invalidation("company", "company_os_work_items.jsonl")).join(",") === "works,runtime",
    "Work ledger invalidation affects Works plus read-model convergence only",
  );
  check(
    freshnessDomainsForInvalidation(invalidation("company", "company_os_documents.jsonl")).join(",") === "docs,runtime",
    "Docs ledger invalidation affects Docs plus read-model convergence only",
  );
  check(
    freshnessDomainsForInvalidation(invalidation("company", "company_os_org_units.jsonl")).join(",") === "organization,runtime",
    "Organization ledger invalidation affects Org plus read-model convergence only",
  );
  check(
    freshnessDomainsForInvalidation(null).length === 4,
    "malformed invalidation fails stale across every domain",
  );
  const scoped = updateFreshness(uniformFreshness("live"), ["docs", "runtime"], "stale");
  check(
    scoped.works === "live" && scoped.docs === "stale" && scoped.organization === "live" && scoped.runtime === "stale",
    "domain update preserves unaffected freshness claims",
  );
} finally {
  await rm(directory, { recursive: true, force: true });
}

console.log(`\nDomain freshness checks: ${passed} pass, ${failed} fail`);
process.exit(failed === 0 ? 0 : 1);
