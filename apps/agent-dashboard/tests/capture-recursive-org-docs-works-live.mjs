#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const dashboardUrl = process.env.DASHBOARD_URL;
const apiUrl = process.env.HARNESS_API_URL;
if (!dashboardUrl || !apiUrl) {
  console.error("DASHBOARD_URL and HARNESS_API_URL are required; this capture never falls back to a fixture.");
  process.exit(2);
}

const space = process.env.HARNESS_SPACE ?? "company-os-recursive-implementation-v1-20260804";
const project = process.env.HARNESS_PROJECT_ID ?? "multi-agent-harness";
const company = process.env.HARNESS_COMPANY_ID ?? "agent-company";
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const evidenceRoot = resolve(process.env.EVIDENCE_DIR ?? `${repoRoot}/.visual-evidence/company-os-v6/recursive-org-docs-works-v1/live-current`);
const viewports = [
  { width: 1440, height: 1000, label: "desktop-1440x1000" },
  { width: 900, height: 1180, label: "tablet-900x1180" },
  { width: 390, height: 844, label: "mobile-390x844" },
  { width: 320, height: 720, label: "small-320x720" },
];
const cases = [
  { id: "organization", query: "surface=organization&orgView=agent-teams", ready: '[data-agent-team-organization="ready"]' },
  { id: "team-works", query: "surface=work&workView=team-works", ready: '[data-team-works="ready"]' },
];

await mkdir(evidenceRoot, { recursive: true });
const browser = await chromium.launch({ headless: true });
const manifest = { captured_at: new Date().toISOString(), dashboard_url: dashboardUrl, api_url: apiUrl, space, project, company, captures: [] };

try {
  for (const viewport of viewports) {
    const context = await browser.newContext({ viewport });
    for (const capture of cases) {
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (error) => errors.push(String(error)));
      page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
      // Harness intentionally exposes no cross-origin browser API. Proxy each
      // browser read through this Node capture process while preserving the
      // exact live URL/path/query; no fixture or rewritten snapshot is used.
      await page.route("**/v1/**", async (route) => {
        const incoming = new URL(route.request().url());
        if (incoming.pathname === "/v1/events") {
          await route.fulfill({ status: 204, body: "" });
          return;
        }
        const target = new URL(`${incoming.pathname}${incoming.search}`, apiUrl);
        const response = await fetch(target, { method: route.request().method() });
        await route.fulfill({
          status: response.status,
          contentType: response.headers.get("content-type") ?? "application/json",
          body: await response.text(),
        });
      });
      const params = `${capture.query}&api=${encodeURIComponent(apiUrl)}&space=${encodeURIComponent(space)}&project=${encodeURIComponent(project)}&company=${encodeURIComponent(company)}`;
      await page.goto(`${dashboardUrl}/?${params}`, { waitUntil: "domcontentloaded" });
      await page.waitForSelector(capture.ready, { timeout: 20_000 });
      const facts = await page.evaluate(() => ({
        mode: document.querySelector('[data-company-os-data-mode="execution-snapshot"]') ? "execution-snapshot" : "wrong",
        overflowX: document.documentElement.scrollWidth > document.documentElement.clientWidth,
        orgTeams: document.querySelectorAll("[data-org-team-id]").length,
        teamWorks: document.querySelectorAll("[data-team-work-id]").length,
        text: document.body.innerText.slice(0, 700),
      }));
      const file = resolve(evidenceRoot, `${capture.id}-${viewport.label}.png`);
      await page.screenshot({ path: file, fullPage: false });
      manifest.captures.push({ case: capture.id, viewport, file, errors, facts });
      console.log(`${capture.id} ${viewport.label}: mode=${facts.mode} overflowX=${facts.overflowX} orgTeams=${facts.orgTeams} teamWorks=${facts.teamWorks} errors=${errors.length}`);
      if (facts.mode !== "execution-snapshot" || facts.overflowX || errors.length) process.exitCode = 1;
      await page.close();
    }
    await context.close();
  }
} finally {
  await browser.close();
  await writeFile(resolve(evidenceRoot, "capture-run.json"), `${JSON.stringify(manifest, null, 2)}\n`);
}
