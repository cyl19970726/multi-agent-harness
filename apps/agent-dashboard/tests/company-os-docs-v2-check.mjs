#!/usr/bin/env node
/**
 * Deterministic static check for the AI-first Docs v2 dashboard slice
 * (ADR 0054 Phase 0). Verifies the surface wiring exists and stays
 * store-live: no fixture fallback may be introduced into DocsV2Surface.
 */

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
let failures = 0;
const fail = (message) => {
  failures += 1;
  console.error(`FAIL  ${message}`);
};
const pass = (message) => console.log(`PASS  ${message}`);
const read = (rel) => readFileSync(resolve(root, rel), "utf8");
const checkFile = (rel, markers, label) => {
  const source = read(rel);
  for (const marker of markers) {
    if (source.includes(marker)) {
      pass(`${label}: contains ${JSON.stringify(marker.slice(0, 64))}`);
    } else {
      fail(`${label}: missing ${JSON.stringify(marker.slice(0, 64))}`);
    }
  }
  return source;
};

// --- DocsV2Surface component --------------------------------------------
const surface = checkFile(
  "src/company-os/docs/DocsV2Surface.tsx",
  [
    "data-docs-v2-surface",
    "data-docs-v2-page",
    "data-docs-v2-index",
    "data-docs-v2-block",
    "data-docs-v2-embed",
    "data-docs-v2-revision",
    "data-docs-v2-error",
    "fetchDocsV2Page",
    "fetchDocsV2PageIndex",
    "MAX_TRANSCLUSION_DEPTH = 2",
    "Transclusion cycle detected",
    "store-live",
    "data-docs-v2-embed-resolved",
    "resolvedEmbeds",
  ],
  "DocsV2Surface",
);
if (/__COMPANY_OS_FIXTURE__|fixture/i.test(surface.replace(/fixture fallback/gi, ""))) {
  // Fixture words may appear in comments describing their absence, but the
  // component must never import or read a fixture source.
  if (/import .*fixture|adaptTrademarkDocsFixture|company-os-trademark-v1\.json/i.test(surface)) {
    fail("DocsV2Surface must not import or read any fixture source");
  } else {
    pass("DocsV2Surface has no fixture imports (store-live only)");
  }
} else {
  pass("DocsV2Surface has no fixture imports (store-live only)");
}

// --- api fetch helpers -----------------------------------------------------
checkFile(
  "src/api.ts",
  [
    "export function fetchDocsV2Page(",
    "export function fetchDocsV2PageIndex(",
    "/v1/company-os/docs-v2/pages",
    "export interface DocsV2PageView",
    "export interface DocsV2PageIndexItem",
    "export interface DocsV2ResolvedEmbed",
    "resolved_embeds",
  ],
  "api.ts docs-v2 helpers",
);

// --- selection + routing -----------------------------------------------------
checkFile(
  "src/app/selection.ts",
  ['"docs-v2"'],
  "selection.ts docs-v2 surface",
);
checkFile(
  "src/app/WorkbenchShell.tsx",
  [
    'import { DocsV2Surface } from "../company-os/docs/DocsV2Surface";',
    'selection.surface === "docs-v2"',
    "companyId={selectedCompanyId}",
  ],
  "WorkbenchShell docs-v2 wiring",
);
checkFile(
  "src/company-os/routeMeta.ts",
  ['"docs-v2"'],
  "routeMeta Company OS surface set",
);

if (failures > 0) {
  console.error(`\ncompany-os docs-v2 dashboard check: ${failures} failure(s)`);
  process.exit(1);
}
console.log("\ncompany-os docs-v2 dashboard check: all checks passed");
