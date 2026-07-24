#!/usr/bin/env node

import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const here = dirname(fileURLToPath(import.meta.url));
const dashboardRoot = join(here, "..");
const repoRoot = resolve(dashboardRoot, "..", "..");

function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => error ? reject(error) : resolvePort(address.port));
    });
  });
}

function startVite(port) {
  const child = spawn(process.execPath, [
    join(repoRoot, "node_modules/vite/bin/vite.js"),
    "--config", "apps/agent-dashboard/vite.config.ts",
    "--host", "127.0.0.1",
    "--port", String(port),
  ], {
    cwd: repoRoot,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  for (const stream of [child.stdout, child.stderr]) {
    stream.on("data", (chunk) => { output = `${output}${chunk}`.slice(-20_000); });
  }
  child.failure = () => output;
  return child;
}

async function waitFor(url, label) {
  const deadline = Date.now() + 30_000;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
      lastError = new Error(`HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 200));
  }
  throw new Error(`${label} did not become ready: ${lastError?.message ?? "timeout"}`);
}

async function stopProcess(child) {
  if (!child || child.exitCode !== null) return;
  child.kill("SIGTERM");
  await Promise.race([
    new Promise((resolveExit) => child.once("exit", resolveExit)),
    new Promise((resolveWait) => setTimeout(resolveWait, 2_000)),
  ]);
  if (child.exitCode === null) child.kill("SIGKILL");
}

function customPageProjection() {
  const now = "2026-07-24T10:00:00+08:00";
  return {
    fixture_id: "company-os-custom-page-browser-smoke",
    actors: [
      { id: "human-docs-owner", actor_type: "human", display_name: "Docs Owner", permission_policy_refs: ["company_os.admin", "company.records.write"] },
      { id: "agent-docs-governance", actor_type: "agent", display_name: "Docs Governance Agent", permission_policy_refs: ["company.records.write"] },
    ],
    documents: [
      {
        id: "document-custom-page-root",
        space_id: "company",
        parent_document_id: null,
        title: "Custom Page Root",
        kind: "page",
        lifecycle_status: "active",
        block_ids: [],
        permission_policy_refs: ["company.records.write"],
        reference_refs: [],
        created_by: { actor_type: "human", actor_id: "human-docs-owner" },
        updated_by: { actor_type: "human", actor_id: "human-docs-owner" },
        created_at: now,
        updated_at: now,
      },
    ],
    typed_records: [
      {
        id: "record-custom-page-1",
        module_id: "module-custom-page",
        record_type: "TrademarkApplication",
        title: "Custom page application",
        source_document_ref: "document-custom-page-root",
        fields: { status: "review" },
        created_at: now,
        updated_at: now,
      },
    ],
    views: [
      {
        id: "view-custom-page-fallback",
        module_id: "module-custom-page",
        title: "Custom page fallback view",
        mode: "table",
        source_kinds: ["typed_record"],
        query: { filters: [{ field: "module_id", value: "module-custom-page" }], sort_by: "updated_at" },
      },
    ],
    business_modules: [
      {
        id: "module-custom-page",
        name: "Custom Page Module",
        root_document_ref: "document-custom-page-root",
        record_types: ["TrademarkApplication"],
        status: "active",
        default_view_refs: ["view-custom-page-fallback"],
        custom_page_definition_refs: ["page-custom-page-module"],
      },
    ],
    custom_page_definitions: [
      {
        id: "page-custom-page-module",
        module_id: "module-custom-page",
        purpose: "Render a governed custom page over module records.",
        allowed_data_queries: [
          { id: "query-custom-page-module", source_kind: "business_module", source_scope: "module-custom-page", permission_policy_ref: "company.records.write" },
        ],
        approved_ui_components: ["CodeDeclaredPage", "VisualContractReview"],
        action_command_refs: ["typed_record.append", "view.append", "relation.append"],
        standard_view_fallback_ref: "view-custom-page-fallback",
        owner: { actor_type: "human", actor_id: "human-docs-owner" },
        package_ref: "package-custom-page-active",
        package_version: "1.0.0",
        fixture_ref: "docs/design/company-os/custom-pages/custom-page/fixture.json",
        visual_contract_ref: "docs/design/company-os/custom-pages/custom-page/review.html",
        policy_refs: ["page-custom-page-module:typed_record.append", "page-custom-page-module:view.append", "page-custom-page-module:relation.append"],
        created_at: now,
        updated_at: now,
      },
    ],
    custom_page_packages: [
      { id: "package-custom-page-active", definition_id: "page-custom-page-module", version: "1.0.0", kind: "react", artifact_ref: "apps/agent-dashboard/src/company-os/modules/custom-page/CustomPage.tsx", entrypoint: "index.tsx", integrity_digest: "sha256:active-custom-page", built_at: now },
      { id: "package-custom-page-candidate", definition_id: "page-custom-page-module", version: "1.0.1", kind: "react", artifact_ref: "apps/agent-dashboard/src/company-os/modules/custom-page/CustomPage.tsx", entrypoint: "index.tsx", integrity_digest: "sha256:candidate-custom-page", built_at: "2026-07-24T11:00:00+08:00" },
    ],
  };
}

async function main() {
  const port = await freePort();
  const vite = startVite(port);
  let browser;
  try {
    await waitFor(`http://127.0.0.1:${port}`, "dashboard dev server");
    browser = await chromium.launch();
    const page = await browser.newPage({ viewport: { width: 1365, height: 900 } });
    await page.addInitScript((fixture) => {
      window.__COMPANY_OS_FIXTURE__ = fixture;
    }, customPageProjection());
    await page.goto(`http://127.0.0.1:${port}/?surface=docs&module=module-custom-page`, { waitUntil: "networkidle" });
    const root = page.locator('[data-company-os-page="business-module-focus"]').first();
    await root.waitFor({ timeout: 15_000 });

    const contract = root.locator('[data-docs-custom-page-contract="true"]').first();
    await contract.waitFor({ timeout: 15_000 });
    const status = await contract.getAttribute("data-docs-custom-page-status");
    if (status !== "candidate_recorded") throw new Error(`expected candidate_recorded custom page status, received ${status}`);
    if (await contract.getAttribute("data-docs-custom-page-active-package") !== "package-custom-page-active") {
      throw new Error("active package marker is missing or incorrect");
    }
    if (await contract.getAttribute("data-docs-custom-page-latest-package") !== "package-custom-page-candidate") {
      throw new Error("candidate/latest package marker is missing or incorrect");
    }
    const text = (await contract.innerText()).replace(/\s+/g, " ");
    for (const expected of ["view-custom-page-fallback", "typed_record.append", "relation.append", "not a second truth", "docs/design/company-os/custom-pages/custom-page/review.html"]) {
      if (!text.includes(expected)) throw new Error(`custom page contract card is missing ${expected}: ${text}`);
    }
    if (await root.locator('[data-docs-standard-view-provenance="true"]').count() !== 1) {
      throw new Error("standard View fallback/provenance remains required beside the custom page contract");
    }
    if (await root.locator('[data-company-os-ref="record-custom-page-1"]').count() < 1) {
      const refs = await root.locator("[data-company-os-ref]").evaluateAll((nodes) => nodes.map((node) => node.getAttribute("data-company-os-ref")));
      const visible = (await root.innerText()).replace(/\s+/g, " ").slice(0, 2_000);
      throw new Error(`custom page route lost its native TypedRecord reference: ${JSON.stringify({ refs, visible })}`);
    }
    const overflow = await page.evaluate(() => ({ width: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
    if (overflow.scroll > overflow.width) throw new Error(`custom page Docs route has horizontal overflow: ${JSON.stringify(overflow)}`);
    console.log("Company OS custom page browser smoke: passed");
  } finally {
    if (browser) await browser.close();
    await stopProcess(vite);
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
