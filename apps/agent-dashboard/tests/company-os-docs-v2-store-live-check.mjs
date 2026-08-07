#!/usr/bin/env node
/**
 * Store-live browser acceptance for the AI-first Docs v2 surface (ADR 0054
 * Phase 0). Boots a REAL Company Store + `harness serve`, seeds pages through
 * the governed CLI, and drives the dashboard surface in headless chromium.
 *
 * The docs-v2 data path is proxied verbatim to the live server: assertions
 * below prove the rendered blocks, revision banner, and page_embed cards
 * come from Store truth — there is no fixture anywhere in this surface.
 */

import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";
import { chromium } from "playwright";
import { createServer } from "vite";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const serveAddr = "127.0.0.1:18915";
const serveBase = `http://${serveAddr}`;
const companyId = "docs-v2-browser";

const home = mkdtempSync(join(tmpdir(), "harness-docs-v2-browser-"));
const env = { ...process.env, HARNESS_HOME: home };

let pass = 0;
let fail = 0;
const check = (condition, message) => {
  console.log(`  ${condition ? "PASS" : "FAIL"}  ${message}`);
  condition ? pass++ : fail++;
};

function cli(args) {
  return execFileSync(harness, args, { env, encoding: "utf8", cwd: repoRoot });
}

// --- seed a real Company Store through the governed CLI --------------------
cli(["company", "init", "--id", companyId, "--name", "Docs V2 Browser"]);
cli(["company", "switch", companyId]);
cli([
  "company", "docs", "page", "create",
  "--title", "Browser Target",
  "--id", "document-browser-target",
  "--actor", "agent-browser-seed",
  "--markdown", "Target body **resolved live**.",
]);
cli([
  "company", "docs", "page", "create",
  "--title", "Browser Main",
  "--id", "document-browser-main",
  "--actor", "agent-browser-seed",
  "--markdown", [
    "# Main Overview",
    "",
    "Intro paragraph with **bold** text.",
    "",
    "## Capabilities",
    "",
    "- revisions",
    "- embeds",
    "",
    "1. first",
    "2. second",
    "",
    "- [x] seeded",
    "- [ ] reviewed",
    "",
    "> [!note] Scope",
    "> Phase 0 browser acceptance.",
    "",
    "| surface | status |",
    "| --- | --- |",
    "| docs-v2 | green |",
    "",
    "```text",
    "code block body",
    "```",
    "",
    "> plain quote line",
    "",
    "---",
    "",
    "![[page:document-browser-target display=card]]",
    "",
    "![[page:document-browser-target display=inline]]",
    "",
    "![[typed_record:tr-browser-1 display=card]]",
    "",
    "![[typed_record:tr-missing display=card]]",
  ].join("\n"),
]);

const browserToken = "docs-v2-browser-check-token";
const server = spawn(harness, ["serve", "--addr", serveAddr], {
  env: { ...env, HARNESS_COMPANY_OS_TOKEN: browserToken },
  cwd: repoRoot,
  stdio: ["ignore", "pipe", "pipe"],
});
let serverLog = "";
server.stdout.on("data", (chunk) => (serverLog += chunk.toString()));
server.stderr.on("data", (chunk) => (serverLog += chunk.toString()));

const vite = await createServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 0 },
  logLevel: "silent",
});
await vite.listen();
const base = `http://127.0.0.1:${vite.httpServer.address().port}`;

const browser = await chromium.launch({ headless: true });
try {
  // Wait for serve readiness.
  let ready = false;
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await fetch(`${serveBase}/v1/company-os/docs-v2/pages?company=${companyId}`);
      if (response.ok) {
        ready = true;
        break;
      }
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  if (!ready) {
    console.error(`serve did not become ready:\n${serverLog.slice(-800)}`);
    process.exit(2);
  }

  // F4 seed: business module + typed record via the token-gated direct path,
  // so the docs-v2 page endpoint can resolve entity_embed targets live.
  const seedHeaders = { "Content-Type": "application/json", "X-Harness-Company-OS-Token": browserToken };
  const seedFetch = (path, body) =>
    fetch(`${serveBase}${path}?company=${companyId}`, { method: "POST", headers: seedHeaders, body: JSON.stringify(body) });
  const actorSeed = await seedFetch("/v1/company-os/actors", {
    actor_type: "human",
    actor: {
      id: "human-browser-root",
      display_name: "Browser Root Human",
      title: null,
      status: "active",
      availability: null,
      membership_refs: [],
      responsibility_summary: "Browser acceptance bootstrap root",
      permission_policy_refs: ["company_os.admin"],
      authority_policy_refs: [],
      created_at: "unix-ms:0",
      updated_at: "unix-ms:0",
    },
  });
  const adminSeed = (path, record) =>
    seedFetch(path, { mode: "administrative", authority: { actor_type: "human", actor_id: "human-browser-root" }, record });
  const moduleSeed = await adminSeed("/v1/company-os/business-modules", {
    id: "module-browser-smoke",
    name: "Browser Smoke Module",
    purpose: "embed resolution browser acceptance",
    root_document_ref: "document-browser-main",
    record_types: ["smoke_record"],
    relation_rules: [],
    default_view_refs: [],
    policy_refs: [],
    lifecycle_rules: [],
    metric_definition_refs: [],
    custom_page_definition_refs: [],
    status: "active",
    owner: { actor_type: "human", actor_id: "human-browser-root" },
    created_at: "unix-ms:0",
    updated_at: "unix-ms:0",
  });
  const recordSeed = await adminSeed("/v1/company-os/typed-records", {
    id: "tr-browser-1",
    module_id: "module-browser-smoke",
    record_type: "smoke_record",
    title: "Resolved Browser Record",
    fields: {},
    lifecycle_status: "active",
    source_document_ref: "document-browser-main",
    created_by: { actor_type: "human", actor_id: "human-browser-root" },
    updated_by: { actor_type: "human", actor_id: "human-browser-root" },
    created_at: "unix-ms:0",
    updated_at: "unix-ms:0",
  });
  if (actorSeed.status !== 200 || moduleSeed.status !== 200 || recordSeed.status !== 200) {
    console.error(`F4 seed failed: actor=${actorSeed.status} module=${moduleSeed.status} record=${recordSeed.status}`);
    process.exit(2);
  }

  const page = await browser.newPage({ viewport: { width: 1536, height: 1024 } });
  // Docs-v2 requests are proxied VERBATIM to the live server; everything else
  // gets a minimal app-chrome stub so the shell can boot. The docs-v2 surface
  // itself renders nothing that is not Store truth.
  await page.route("**/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    const json = (body) => route.fulfill({ status: 200, contentType: "application/json", body });
    if (path.startsWith("/v1/company-os/docs-v2/")) {
      const upstream = await fetch(`${serveBase}${path}${url.search}`);
      const body = await upstream.text();
      return route.fulfill({ status: upstream.status, contentType: "application/json", body });
    }
    if (path === "/v1/snapshot") {
      return json(JSON.stringify({
        ok: true,
        result: {
          generated_at: "2026-08-06T00:00:00Z", teams: [], missions: [], waves: [], team_runs: [],
          member_runs: [], team_messages: [], member_actions: [], delegation_runs: [], team_run_events: [],
          evidence: [], members: [], messages: [], events: [], provider_child_threads: [],
          workflow_runs: [], workflow_steps: [], workflow_patches: [], workflow_artifact_manifests: [],
          team_supervisor_leases: [], team_member_close_requests: [], agent_message_routes: [],
          pending_interactions: [],
        },
      }));
    }
    if (path === "/v1/events") {
      return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
    }
    if (["/v1/projects", "/v1/spaces", "/v1/companies"].includes(path)) {
      return json('{"projects":[],"spaces":[],"companies":[],"current":""}');
    }
    if (path === "/v1/workflows") return json('{"workflows":[]}');
    if (path === "/v1/meta") return json('{"ok":true,"result":{"rev":"test","built_at":null}}');
    return json('{"ok":false,"error":"stub"}');
  });

  // --- index ----------------------------------------------------------------
  await page.goto(`${base}/?surface=docs-v2&company=${companyId}`, { waitUntil: "networkidle" });
  await page.waitForSelector('[data-docs-v2-index]', { timeout: 20000 });
  check((await page.locator('[data-docs-v2-index]').count()) === 1, "index surface renders from the live store");
  check(
    (await page.locator('[data-docs-v2-page="document-browser-main"]').count()) === 1 &&
      (await page.locator('[data-docs-v2-page="document-browser-target"]').count()) === 1,
    "index lists both seeded pages",
  );
  await page.screenshot({ path: "/tmp/docs-v2-index.png" });

  // --- page rendering ---------------------------------------------------------
  await page.locator('[data-docs-v2-page="document-browser-main"]').click();
  await page.waitForSelector('[data-docs-v2-page="document-browser-main"] article, article[data-docs-v2-page="document-browser-main"]', { timeout: 20000 });
  const blockAttr = (kind) => page.locator(`[data-docs-v2-block="${kind}"]`);
  check((await blockAttr("heading").count()) >= 2, "heading blocks render");
  check((await blockAttr("paragraph").count()) >= 1, "paragraph blocks render");
  check((await blockAttr("bullet_list").count()) === 1, "bullet list renders");
  check((await blockAttr("ordered_list").count()) === 1, "ordered list renders");
  check((await blockAttr("checklist").count()) === 1, "checklist renders");
  check((await blockAttr("callout").count()) === 1, "callout renders");
  check((await blockAttr("code").count()) === 1, "code block renders");
  check((await blockAttr("divider").count()) === 1, "divider renders");
  check((await blockAttr("quote").count()) === 1, "quote renders");
  check((await page.locator('[data-docs-v2-block="heading"] ~ * table, table').count()) >= 1, "table renders");
  check(
    (await page.locator('[data-docs-v2-revision="1"]').count()) === 1,
    "revision banner shows store-live r1",
  );

  // --- page_embed live resolution -----------------------------------------------
  const embeds = page.locator('[data-docs-v2-embed="document-browser-target"]');
  check((await embeds.count()) === 2, "both page_embed blocks (card + inline) render");
  const surfaceText = await page.locator('[data-docs-v2-surface]').innerText();
  check(surfaceText.includes("Browser Target"), "embed card resolves the live target title");
  check(surfaceText.includes("resolved live"), "inline transclusion renders the target's live body");
  await page.screenshot({ path: "/tmp/docs-v2-page.png" });

  // --- embed navigation -------------------------------------------------------
  await page.locator('button[data-docs-v2-embed="document-browser-target"]').first().click();
  await page.waitForSelector('article[data-docs-v2-page="document-browser-target"]', { timeout: 20000 });
  check(
    (await page.locator('article[data-docs-v2-page="document-browser-target"]').count()) === 1,
    "embed card click navigates to the target page",
  );

  // --- F4 entity_embed live resolution ----------------------------------------
  await page.goto(`${base}/?surface=docs-v2&company=${companyId}&document=document-browser-main`, { waitUntil: "networkidle" });
  await page.waitForSelector('[data-docs-v2-embed="typed_record:tr-browser-1"]', { timeout: 20000 });
  const resolvedEmbed = page.locator('[data-docs-v2-embed="typed_record:tr-browser-1"]');
  check(
    (await resolvedEmbed.getAttribute("data-docs-v2-embed-resolved")) === "true",
    "F4 entity_embed card resolves live from the owning ledger",
  );
  const mainText = await page.locator('article[data-docs-v2-page="document-browser-main"]').innerText();
  check(mainText.includes("Resolved Browser Record"), "F4 entity_embed card shows the live record title");
  check(
    (await page.locator('[data-docs-v2-embed="typed_record:tr-missing"]').getAttribute("data-docs-v2-embed-resolved")) === "false",
    "F4 missing entity_embed target renders honest found:false state",
  );
  await page.screenshot({ path: "/tmp/docs-v2-entity-embed.png", fullPage: true });

  // --- honest error state -------------------------------------------------------
  await page.goto(`${base}/?surface=docs-v2&company=${companyId}&document=document-does-not-exist`, { waitUntil: "networkidle" });
  await page.waitForSelector('[data-docs-v2-error]', { timeout: 20000 });
  check((await page.locator('[data-docs-v2-error]').count()) === 1, "missing page renders an honest error card, never a fixture");
} finally {
  await browser.close();
  await vite.close();
  server.kill("SIGTERM");
  await new Promise((r) => setTimeout(r, 300));
  if (!server.killed) server.kill("SIGKILL");
  rmSync(home, { recursive: true, force: true });
}

console.log(`\n  Docs v2 store-live browser check: ${pass} pass, ${fail} fail`);
process.exit(fail === 0 ? 0 : 1);
