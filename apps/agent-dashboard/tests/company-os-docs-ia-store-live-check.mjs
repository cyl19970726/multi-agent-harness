#!/usr/bin/env node
/**
 * Store-live browser acceptance for Docs IA M1/M2.
 *
 * Unlike company-os-docs-check.mjs this needs a REAL Company Store, so it is not part
 * of the deterministic check suite. Point it at a running `harness serve`:
 *
 *   harness serve --addr 127.0.0.1:8787
 *   node apps/agent-dashboard/tests/company-os-docs-ia-store-live-check.mjs
 *
 * Override the source with COMPANY_OS_SNAPSHOT_URL (a /v1/company-os/snapshot route)
 * or COMPANY_OS_SNAPSHOT_FILE (a saved response body).
 */
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const snapshotUrl = process.env.COMPANY_OS_SNAPSHOT_URL ?? "http://127.0.0.1:8787/v1/company-os/snapshot";
const snapshotFile = process.env.COMPANY_OS_SNAPSHOT_FILE;
let companyOs;
try {
  const body = snapshotFile
    ? JSON.parse(await readFile(snapshotFile, "utf8"))
    : await (await fetch(snapshotUrl)).json();
  companyOs = body.result ?? body;
} catch (error) {
  console.error(`Could not read a live Company OS snapshot from ${snapshotFile ?? snapshotUrl}.`);
  console.error("Start `harness serve` first, or set COMPANY_OS_SNAPSHOT_URL / COMPANY_OS_SNAPSHOT_FILE.");
  console.error(String(error.message ?? error));
  process.exit(2);
}
if (!companyOs?.documents?.length) {
  console.error("The snapshot contains no Company OS Documents; this check needs a seeded Store.");
  process.exit(2);
}
const snapshot = { generated_at: "2026-08-01T00:00:00Z", teams: [], missions: [], waves: [], team_runs: [], member_runs: [], team_messages: [], member_actions: [], delegation_runs: [], team_run_events: [], evidence: [], members: [], messages: [], events: [], provider_child_threads: [], workflow_runs: [], workflow_steps: [], workflow_patches: [], workflow_artifact_manifests: [], team_supervisor_leases: [], team_member_close_requests: [], agent_message_routes: [], pending_interactions: [], company_os: companyOs };

const vite = await createServer({ configFile: join(dashboardRoot, "vite.config.ts"), server: { host: "127.0.0.1", port: 0 }, logLevel: "silent" });
await vite.listen();
const base = `http://127.0.0.1:${vite.httpServer.address().port}`;
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1536, height: 1024 } });
await page.route("**/v1/**", async (r) => {
  const p = new URL(r.request().url()).pathname;
  const j = (b) => r.fulfill({ status: 200, contentType: "application/json", body: b });
  if (p === "/v1/snapshot") return j(JSON.stringify(snapshot));
  if (p === "/v1/events") return r.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
  if (["/v1/projects", "/v1/spaces", "/v1/companies"].includes(p)) return j('{"projects":[],"spaces":[],"companies":[],"current":""}');
  if (p === "/v1/workflows") return j('{"workflows":[]}');
  return r.fulfill({ status: 404, contentType: "application/json", body: '{"error":"x"}' });
});
let pass = 0, fail = 0;
const check = (c, m) => { console.log(`  ${c ? "PASS" : "FAIL"}  ${m}`); c ? pass++ : fail++; };
const open = async (q) => { await page.goto(`${base}/?surface=docs&${q}`, { waitUntil: "networkidle" }); await page.waitForSelector("article[data-docs-v2-page]", { timeout: 20000 }); };

// M1 — Store-live regression for document-wcw-root
await open("document=document-wcw-root");
check(await page.locator('[data-docs-empty-document="true"]').count() === 1, "M1 document-wcw-root renders the honest empty-document state from the live Store");
const wcwText = await page.locator("article[data-docs-v2-page]").last().innerText();
check(!wcwText.includes("What this plan coordinates") && !wcwText.includes("Why this context matters"), "M1 no synthesized narrative headings appear under the real Document id");
await page.screenshot({ path: "/tmp/m1-wcw-root-empty.png" });

// M1 control — a Store Document that does declare Blocks is untouched
await open("document=document-agentos-01-agentos-dogfood");
check(await page.locator('[data-docs-empty-document="true"]').count() === 0, "M1 control a Store Document with Blocks still renders them, not the empty state");

// M2 — Store-live related work
await open("document=document-agentos-03-org-work-doc-loop");
const panel = page.locator('[data-docs-related-work="true"]').first();
check(await panel.count() === 1, "M2 the Related work panel renders");
check(await panel.getAttribute("data-docs-related-work-count") === "17", `M2 panel reports 17 distinct related WorkItems (got ${await panel.getAttribute("data-docs-related-work-count")})`);
const chips = await page.evaluate(() => [...document.querySelectorAll('[data-docs-related-work="true"] [data-company-os-ref]')].map((n) => ({ ref: n.getAttribute("data-company-os-ref"), text: n.innerText })));
check(new Set(chips.map((c) => c.ref)).size === 17 && chips.length === 17, `M2 exactly 17 distinct chips render, none duplicated (got ${chips.length} chips / ${new Set(chips.map((c) => c.ref)).size} distinct)`);
const multi = chips.filter((c) => c.text.includes("·"));
check(multi.length === 3 && multi.every((c) => /Source document|Context reference|Result document/.test(c.text)), `M2 the 3 multi-reason WorkItems show both reasons on one chip (got ${multi.length})`);
check(chips.every((c) => /Source document|Context reference|Result document/.test(c.text)), "M2 every related WorkItem states why it is related");
const heading = await page.locator('[data-docs-related-work="true"] h2').first().innerText();
check(heading.trim().toLowerCase() === "related work", `M2 the panel is named for what it holds ("${heading.trim()}")`);
await page.screenshot({ path: "/tmp/m2-related-work.png" });

// Preserved behaviour
check(await page.locator('[data-docs-breadcrumbs="true"]').count() >= 1, "preserved breadcrumbs still render");
await open("document=document-agentos-root");
const railLabels = await page.evaluate(() => [...document.querySelectorAll("article[data-docs-v2-page] aside h2")].map((n) => n.innerText.trim().toLowerCase()));
check(railLabels.includes("child pages") && railLabels.includes("related work"), `preserved child-page rail still renders alongside related work (${railLabels.join(", ")})`);
await open("document=document-does-not-exist-xyz");
check(await page.locator('[data-docs-document-not-found="true"]').count() === 1, "preserved honest not-found route still renders");
await page.goto(`${base}/?surface=docs`, { waitUntil: "networkidle" });
await page.waitForSelector('[data-company-os-page="docs-workspace"][data-company-os-ready="true"]');
check(await page.locator('[data-docs-archive-view="explicit"]').count() >= 1 && await page.locator('aside[aria-label="Document tree"]').count() === 1, "preserved tree and explicit archive still render");
await page.setViewportSize({ width: 768, height: 1024 });
await open("document=document-agentos-03-org-work-doc-loop");
check(await page.locator('[data-docs-related-work="true"]').count() >= 1, "preserved narrow width still reaches related work at 768px");
await page.screenshot({ path: "/tmp/m2-related-work-narrow.png" });

console.log(`\n  Store-live browser check: ${pass} pass, ${fail} fail`);
await browser.close(); await vite.close();
process.exit(fail === 0 ? 0 : 1);
