#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFileSync, spawn } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixturePath = join(repoRoot, "docs/design/company-os-v1/fixtures/company-os-trademark-v1.json");
const cases = [
  { id: "home--morning-operating-review--desktop", page: "home", route: "/?surface=home", refs: ["approval-trademark-filing-fee-cn-2026-018", "trademark-application-cn-2026-018", "actor-human-brand-owner"] },
  { id: "docs--company-knowledge-workspace--desktop", page: "docs-workspace", route: "/?surface=docs", refs: ["document-trademark-application-cn-2026-018", "module-trademark-management", "governance-proposal-trademark-management"] },
  { id: "organization--lead-first-company--desktop", page: "agents-organization", route: "/?surface=organization", refs: ["org-company", "org-brand-ip", "actor-human-brand-owner", "actor-agent-ip-lead", "actor-agent-trademark"] },
  { id: "lead-agent--coordinating-direct-reports--desktop", page: "standing-agent-focus", route: "/?surface=organization&agent=actor-agent-ip-lead", expectedText: "IP Lead Agent", refs: ["actor-agent-trademark", "workitem-trademark-filing-brand-a"] },
  { id: "business-module--trademark-operations--desktop", page: "business-module-focus", route: "/?surface=docs&module=module-trademark-management", refs: ["module-trademark-management", "trademark-application-cn-2026-018", "workitem-trademark-filing-brand-a", "financial-commitment-trademark-filing-fee-cn-2026-018"] },
  { id: "work--milestones-and-workitems--desktop", page: "workboard", route: "/?surface=work", refs: ["workitem-trademark-filing-brand-a", "actor-human-brand-owner", "actor-agent-trademark", "approval-trademark-filing-fee-cn-2026-018"] },
];

function argument(name, fallback = "") {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

function hash(buffer) {
  return `sha256:${createHash("sha256").update(buffer).digest("hex")}`;
}

function normalizedHttpBase(value, label) {
  const parsed = new URL(value);
  if (!new Set(["http:", "https:"]).has(parsed.protocol)) throw new Error(`${label} must use HTTP(S)`);
  return parsed.toString().replace(/\/$/, "");
}

async function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createNetServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => error ? reject(error) : resolvePort(address.port));
    });
  });
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

async function readJson(url, label) {
  const response = await fetch(url, { headers: { accept: "application/json" } });
  if (!response.ok) throw new Error(`${label} returned HTTP ${response.status}`);
  return response.json();
}

async function inspectLiveSource(apiBaseUrl, requestedProjectId) {
  const projects = await readJson(`${apiBaseUrl}/v1/projects`, "projects API");
  const projectId = requestedProjectId || projects.current;
  if (!projectId) throw new Error("live capture requires --project-id or one current project");
  const project = projects.projects?.find((entry) => entry.id === projectId);
  if (!project) throw new Error(`project ${projectId} is absent from the live server`);
  const snapshotUrl = `${apiBaseUrl}/v1/snapshot?project=${encodeURIComponent(projectId)}`;
  const snapshot = await readJson(snapshotUrl, "project snapshot");
  const companyOs = snapshot.company_os;
  if (companyOs?.snapshot_contract !== "company-os-v1" || companyOs?.projection_kind !== "live_company_os" || companyOs?.source?.authoritative !== true) {
    throw new Error("live server does not expose an authority-labelled Company OS projection");
  }
  return {
    kind: "harness-store-live",
    api_base_url: apiBaseUrl,
    project_id: projectId,
    project,
    snapshot_endpoint: snapshotUrl,
    source: companyOs.source,
  };
}

function startVite(port, apiProxy = "") {
  const child = spawn(process.execPath, [
    join(repoRoot, "node_modules/vite/bin/vite.js"),
    "--config", "apps/agent-dashboard/vite.config.ts",
    "--host", "127.0.0.1",
    "--port", String(port),
  ], {
    cwd: repoRoot,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, ...(apiProxy ? { HARNESS_CAPTURE_API_PROXY: apiProxy } : {}) },
  });
  let output = "";
  for (const stream of [child.stdout, child.stderr]) stream.on("data", (chunk) => { output = `${output}${chunk}`.slice(-20_000); });
  child.failure = () => output;
  return child;
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

async function verifyPage(page, root, item, dataMode) {
  if (item.expectedText && !(await root.getByText(item.expectedText, { exact: true }).count())) {
    throw new Error(`${item.id} does not render its route-selected subject: ${item.expectedText}`);
  }
  for (const ref of item.refs) {
    if (await root.locator(`[data-company-os-ref="${ref}"]`).count() === 0) throw new Error(`${item.id} is missing ${ref}`);
  }
  if (await root.locator('[data-financial-record-type="payment"], [data-financial-type="payment"]').count()) {
    throw new Error(`${item.id} invents a Payment before settlement`);
  }
  if (await root.locator("[data-provider-thinking], [data-thinking-persisted]").count()) {
    throw new Error(`${item.id} exposes thinking as durable product state`);
  }
  if (dataMode === "live") {
    if (await page.evaluate(() => typeof window.__COMPANY_OS_FIXTURE__ !== "undefined")) throw new Error("live page contains an injected fixture");
    if (await root.getAttribute("data-company-os-prototype") !== "false") throw new Error("live page is labelled as prototype");
  }
  const overflow = await page.evaluate(() => ({ width: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
  if (overflow.scroll > overflow.width) throw new Error(`${item.id} has horizontal overflow: ${JSON.stringify(overflow)}`);
}

async function main() {
  const runId = argument("--run-id", `v2-${Date.now()}`);
  if (!/^[A-Za-z0-9._-]+$/.test(runId)) throw new Error("unsafe run id");
  const dataMode = argument("--data-mode", "fixture");
  if (!new Set(["fixture", "live"]).has(dataMode)) throw new Error("--data-mode must be fixture or live");
  const apiBaseUrl = dataMode === "live" ? normalizedHttpBase(argument("--api-base-url"), "--api-base-url") : "";
  if (dataMode === "live" && !apiBaseUrl) throw new Error("live capture requires --api-base-url");
  const outputRoot = resolve(argument("--output", join(repoRoot, ".visual-evidence/company-os-v2", runId)));
  const actualRoot = join(outputRoot, dataMode === "live" ? "store-live-actual" : "actual");
  await mkdir(actualRoot, { recursive: true });
  const fixtureText = await readFile(fixturePath, "utf8");
  const fixture = JSON.parse(fixtureText);
  const liveSource = dataMode === "live" ? await inspectLiveSource(apiBaseUrl, argument("--project-id")) : null;
  const port = await freePort();
  const vite = startVite(port, liveSource?.api_base_url);
  const base = `http://127.0.0.1:${port}`;
  let browser;
  try {
    await waitFor(base, "Vite dashboard").catch((error) => { throw new Error(`${error.message}\n${vite.failure()}`); });
    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext({ viewport: { width: 1536, height: 1024 }, deviceScaleFactor: 1, reducedMotion: "reduce" });
    if (dataMode === "fixture") await context.addInitScript((value) => { window.__COMPANY_OS_FIXTURE__ = value; }, fixture);
    const page = await context.newPage();
    const results = [];
    for (const item of cases) {
      const url = new URL(item.route, base);
      url.searchParams.set("api", dataMode === "live" ? base : "http://127.0.0.1:9");
      if (liveSource) url.searchParams.set("project", liveSource.project_id);
      const consoleErrors = [];
      page.on("console", (message) => {
        if (message.type() === "error") {
          const location = message.location();
          consoleErrors.push(`${message.text()}${location.url ? ` @ ${location.url}` : ""}`);
        }
      });
      page.on("pageerror", (error) => consoleErrors.push(error.message));
      page.on("requestfailed", (request) => {
        const errorText = request.failure()?.errorText ?? "request failed";
        const requestUrl = new URL(request.url());
        // Navigating between capture routes closes the previous page's SSE
        // connection. Chromium reports that expected teardown as ERR_ABORTED.
        if (errorText === "net::ERR_ABORTED" && requestUrl.pathname === "/v1/events") return;
        consoleErrors.push(`${errorText} @ ${request.url()}`);
      });
      await page.goto(url.toString(), { waitUntil: "domcontentloaded", timeout: 15_000 });
      const modeSelector = dataMode === "live" ? '[data-company-os-data-mode="store-live"]' : '[data-company-os-prototype="true"]';
      const root = page.locator(`[data-company-os-page="${item.page}"][data-company-os-ready="true"]${modeSelector}`).first();
      await root.waitFor({ state: "visible", timeout: 15_000 });
      await verifyPage(page, root, item, dataMode);
      if (dataMode === "live" && consoleErrors.length) throw new Error(`${item.id} console errors: ${consoleErrors.join(" | ")}`);
      const path = join(actualRoot, `${item.id}.png`);
      await page.screenshot({ path, fullPage: false });
      const bytes = await readFile(path);
      results.push({ ...item, viewport: "desktop-1536x1024", final_url: page.url(), file: relative(repoRoot, path), sha256: hash(bytes), status: "captured" });
    }
    await context.close();
    const manifest = {
      contract: "company-os-v2-implementation-capture-v2",
      run_id: runId,
      status: "passed",
      captured_at: new Date().toISOString(),
      data_mode: dataMode === "live" ? "store-live" : "deterministic-fixture",
      fixture: relative(repoRoot, fixturePath),
      fixture_sha256: hash(Buffer.from(fixtureText)),
      data_source: liveSource ?? { kind: "deterministic-fixture-injection" },
      truth: dataMode === "live"
        ? "Browser-rendered V2.2 implementation evidence from an authority-labelled Harness Store projection."
        : "Browser-rendered implementation evidence from a deterministic fixture; not Store-live product evidence.",
      git_revision: execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim(),
      git_dirty: Boolean(execFileSync("git", ["status", "--porcelain"], { cwd: repoRoot, encoding: "utf8" }).trim()),
      assertions: ["explicit data truth mode", "canonical record refs", "no Payment before settlement", "no persisted thinking", "no horizontal overflow", dataMode === "live" ? "no console errors" : "fixture API errors are non-evidence"],
      results,
    };
    await writeFile(join(outputRoot, "capture-run.json"), `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(JSON.stringify(manifest, null, 2));
  } finally {
    await browser?.close().catch(() => {});
    await stopProcess(vite);
  }
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
