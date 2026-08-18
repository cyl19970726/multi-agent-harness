// INACTIVE HISTORICAL (DOC-108 Stage B): this gate exercised the retired
// legacy CompanyOS surface and is removed from every pipeline. Kept as
// source-only history per the inactive-historical convention (file kept,
// removed from pipelines, named replacement) — see
// docs/current/operations/operations.md.
// Replacement: none — built-in Docs v2 is retired (DOC-108)

#!/usr/bin/env node
/**
 * Real-use visual evidence capture for the AI-first Docs Phase 0 session.
 *
 * Boots `harness serve` over the REAL-USE Company Store (.real-use/harness-home,
 * company `ai-first-docs`) populated entirely through governed CLI operations,
 * then browses the docs-v2 surface in headless chromium and captures evidence
 * screenshots. No seeding happens here — every page already exists as Store
 * truth created during the real-use session.
 */

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";
import { chromium } from "playwright";
import { createServer } from "vite";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const dashboardRoot = join(repoRoot, "apps", "agent-dashboard");
const harness = join(repoRoot, "target", "debug", "harness");
const serveAddr = "127.0.0.1:18917";
const serveBase = `http://${serveAddr}`;
const companyId = "ai-first-docs";
const outDir = join(repoRoot, ".visual-evidence", "docs-v2-real-use-v1");
mkdirSync(outDir, { recursive: true });

const env = { ...process.env, HARNESS_HOME: join(repoRoot, ".real-use", "harness-home") };
const server = spawn(harness, ["serve", "--addr", serveAddr], { env, cwd: repoRoot, stdio: ["ignore", "pipe", "pipe"] });
let serverLog = "";
server.stdout.on("data", (c) => (serverLog += c.toString()));
server.stderr.on("data", (c) => (serverLog += c.toString()));

const vite = await createServer({ configFile: join(dashboardRoot, "vite.config.ts"), server: { host: "127.0.0.1", port: 0 }, logLevel: "silent" });
await vite.listen();
const base = `http://127.0.0.1:${vite.httpServer.address().port}`;

const shots = [];
let pass = 0;
let fail = 0;
const check = (c, m) => {
  console.log(`  ${c ? "PASS" : "FAIL"}  ${m}`);
  c ? pass++ : fail++;
};

try {
  let ready = false;
  for (let i = 0; i < 60; i += 1) {
    try {
      const r = await fetch(`${serveBase}/v1/company-os/docs-v2/pages?company=${companyId}`);
      if (r.ok) {
        ready = true;
        break;
      }
    } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  if (!ready) {
    console.error(`serve not ready:\n${serverLog.slice(-600)}`);
    process.exit(2);
  }

  const page = await (await chromium.launch({ headless: true })).newPage({ viewport: { width: 1536, height: 1024 } });
  await page.route("**/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const json = (body) => route.fulfill({ status: 200, contentType: "application/json", body });
    if (url.pathname.startsWith("/v1/company-os/docs-v2/")) {
      const upstream = await fetch(`${serveBase}${url.pathname}${url.search}`);
      return route.fulfill({ status: upstream.status, contentType: "application/json", body: await upstream.text() });
    }
    if (url.pathname === "/v1/snapshot") {
      return json(JSON.stringify({ ok: true, result: { generated_at: "2026-08-06T00:00:00Z", teams: [], missions: [], waves: [], team_runs: [], member_runs: [], team_messages: [], member_actions: [], delegation_runs: [], team_run_events: [], evidence: [], members: [], messages: [], events: [], provider_child_threads: [], workflow_runs: [], workflow_steps: [], workflow_patches: [], workflow_artifact_manifests: [], team_supervisor_leases: [], team_member_close_requests: [] } }));
    }
    if (url.pathname === "/v1/events") return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
    if (["/v1/projects", "/v1/spaces", "/v1/companies"].includes(url.pathname)) return json('{"projects":[],"spaces":[],"companies":[],"current":""}');
    if (url.pathname === "/v1/workflows") return json('{"workflows":[]}');
    if (url.pathname === "/v1/meta") return json('{"ok":true,"result":{"rev":"real-use","built_at":null}}');
    return json('{"ok":false,"error":"stub"}');
  });

  const shot = async (name) => {
    const path = join(outDir, `${name}.png`);
    await page.screenshot({ path, fullPage: true });
    shots.push(name);
  };

  // 1. Index of the real-use documentation set.
  await page.goto(`${base}/?surface=docs-v2&company=${companyId}`, { waitUntil: "networkidle" });
  await page.waitForSelector('[data-docs-v2-index]', { timeout: 20000 });
  check((await page.locator('[data-docs-v2-index] [data-docs-v2-page]').count()) === 4, "index shows the 4 real-use pages");
  await shot("01-index");

  // 2. Operating home: inline roadmap transclusion + embed cards.
  await page.locator('[data-docs-v2-page="ai-first-docs-home"]').click();
  await page.waitForSelector('article[data-docs-v2-page="ai-first-docs-home"]', { timeout: 20000 });
  await page.waitForSelector('[data-docs-v2-embed="ai-first-docs-roadmap"] table', { timeout: 20000 });
  const homeText = await page.locator('article[data-docs-v2-page="ai-first-docs-home"]').innerText();
  check(homeText.includes("Phase 0 checklist"), "home inline-transcludes the live roadmap (table + checklist visible)");
  check(homeText.includes("AI-first Docs — Spec Brief"), "home embed card resolves the live spec-brief title");
  await shot("02-home-inline-transclusion");

  // 3. Roadmap at revision 3 with the completed checklist.
  await page.goto(`${base}/?surface=docs-v2&company=${companyId}&document=ai-first-docs-roadmap`, { waitUntil: "networkidle" });
  await page.waitForSelector('[data-docs-v2-revision="3"]', { timeout: 20000 });
  check((await page.locator('[data-docs-v2-revision="3"]').count()) === 1, "roadmap banner shows store-live r3");
  const roadmapText = await page.locator('article[data-docs-v2-page="ai-first-docs-roadmap"]').innerText();
  check(roadmapText.includes("real-use acceptance session (done — see ops log)"), "roadmap shows the completed real-use checklist item");
  await shot("03-roadmap-r3");

  // 4. Ops log with both appended session entries.
  await page.goto(`${base}/?surface=docs-v2&company=${companyId}&document=ai-first-docs-ops-log`, { waitUntil: "networkidle" });
  await page.waitForSelector('[data-docs-v2-revision="3"]', { timeout: 20000 });
  const logText = await page.locator('article[data-docs-v2-page="ai-first-docs-ops-log"]').innerText();
  check(logText.includes("session-1") && logText.includes("session-2") && logText.includes("session-3"), "ops log shows all three appended session entries");
  await shot("04-ops-log");

  writeFileSync(
    join(outDir, "capture-run.json"),
    JSON.stringify(
      {
        capture: "docs-v2-real-use-v1",
        date: new Date().toISOString(),
        company_store: ".real-use/harness-home / company ai-first-docs",
        provenance: "all pages created and maintained exclusively through governed CLI page operations during the real-use session",
        pages: ["ai-first-docs-home", "ai-first-docs-spec-brief", "ai-first-docs-roadmap", "ai-first-docs-ops-log"],
        revisions: { "ai-first-docs-home": 1, "ai-first-docs-spec-brief": 1, "ai-first-docs-roadmap": 3, "ai-first-docs-ops-log": 3 },
        screenshots: shots.map((s) => `${s}.png`),
        checks: { pass, fail },
      },
      null,
      2,
    ),
  );
  await page.context().browser().close();
} finally {
  await vite.close();
  server.kill("SIGTERM");
  await new Promise((r) => setTimeout(r, 300));
  if (!server.killed) server.kill("SIGKILL");
}

console.log(`\n  real-use capture: ${pass} pass, ${fail} fail; screenshots: ${shots.join(", ")}`);
process.exit(fail === 0 ? 0 : 1);
