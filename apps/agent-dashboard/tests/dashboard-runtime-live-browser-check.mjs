#!/usr/bin/env node

/**
 * Real Runtime/browser convergence acceptance.
 *
 * This check builds and spawns `harness serve` against isolated native
 * Execution/Company stores. Playwright talks to that Runtime through Vite's
 * same-origin proxy; no snapshot, SSE frame, or business row is fabricated.
 */
import { spawn, spawnSync } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer as createViteServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(dashboardRoot, "../..");
const harness = join(repoRoot, "target", "debug", "harness");
const evidenceRoot = join(repoRoot, ".visual-evidence", "dashboard-runtime-live-e2e-v1");
const token = `dashboard-runtime-live-${process.pid}`;
const now = "2026-08-05T12:00:00+08:00";
const actorRef = { actor_type: "human", actor_id: "human-live-owner" };

let passed = 0;
let failed = 0;
const check = (condition, message) => {
  console.log(`  ${condition ? "PASS" : "FAIL"}  ${message}`);
  if (condition) passed += 1; else failed += 1;
};

async function freePort() {
  return await new Promise((resolvePort, reject) => {
    const server = createNetServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      server.close((error) => error ? reject(error) : resolvePort(address.port));
    });
  });
}

async function waitFor(predicate, message, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  let lastError;
  while (Date.now() < deadline) {
    try {
      if (await predicate()) return;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 50));
  }
  throw new Error(`timed out: ${message}${lastError ? ` (${lastError.message})` : ""}`);
}

function runHarness(args, env, cwd) {
  const result = spawnSync(harness, args, { cwd, env, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(`harness ${args.join(" ")} failed: ${result.stderr || result.stdout}`);
  }
  return result.stdout.trim();
}

async function requestJson(base, path, { body } = {}) {
  const response = await fetch(new URL(path, base), {
    method: body === undefined ? "GET" : "POST",
    headers: body === undefined ? { accept: "application/json" } : {
      accept: "application/json",
      "content-type": "application/json",
      "x-harness-company-os-token": token,
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok || payload.ok === false) {
    throw new Error(`${body === undefined ? "GET" : "POST"} ${path}: HTTP ${response.status} ${JSON.stringify(payload)}`);
  }
  return payload.result ?? payload;
}

function admin(record) {
  return { mode: "administrative", authority: actorRef, record };
}

function documentRecord(id, title) {
  return {
    id, space_id: "company", parent_document_id: null, title, kind: "page",
    lifecycle_status: "active", block_ids: [], template_ref: null,
    permission_policy_refs: ["company.records.write"], reference_refs: [],
    created_by: actorRef, updated_by: actorRef, created_at: now, updated_at: now,
  };
}

function workRecord(id, title, sourceDocument) {
  return {
    id, title, objective: `Prove ${title} converges in an open browser`,
    description: "External Runtime acceptance write.", acceptance_criteria: ["Visible without reload"],
    context_refs: [], deliverable_refs: [], status: "in_progress",
    source_document_ref: sourceDocument, source_record_refs: [], milestone_ref: null,
    work_type: "development", business_module_ref: null, result_document_ref: null,
    result_record_refs: [], submitted_by: actorRef, requested_by: actorRef,
    accountable_owner: actorRef, assignees: [actorRef], contributors: [], reviewer: actorRef,
    approver: null, execution_mode: "direct", execution_refs: [], approval_refs: [],
    evidence_refs: [], artifact_refs: [], outcome_summary: null, due_at: null,
    priority: "high", risk_level: "low", created_at: now, updated_at: now, completed_at: null,
  };
}

function orgUnitRecord(id, name) {
  return {
    id, organization_id: "company", name, purpose: `Prove ${name} converges in an open browser`,
    parent_unit_id: null, status: "active", human_lead_actor_ref: actorRef,
    agent_lead_actor_ref: null, policy_refs: ["company.records.write"],
    document_space_ref: null, created_at: now, updated_at: now,
  };
}

async function waitForText(page, text) {
  try {
    await waitFor(async () => (await page.locator("body").innerText()).includes(text), `browser text ${text}`);
  } catch (error) {
    const body = (await page.locator("body").innerText()).slice(0, 4_000);
    throw new Error(`${error.message}\nBrowser body:\n${body}`);
  }
}

async function waitForDomain(page, domain, status) {
  await page.locator(`[data-freshness-domain="${domain}"][data-freshness-status="${status}"]`)
    .waitFor({ timeout: 15_000 });
}

async function navigate(page, label) {
  await page.locator("aside").getByRole("button", { name: label, exact: true }).click();
}

const build = spawnSync("cargo", ["build", "-q", "-p", "harness-cli"], {
  cwd: repoRoot,
  stdio: "inherit",
});
if (build.status !== 0) process.exit(build.status ?? 1);

const temporaryRoot = await mkdtemp(join(tmpdir(), "dashboard-runtime-live-"));
const harnessHome = join(temporaryRoot, "harness-home");
const projectRoot = join(temporaryRoot, "project");
await mkdir(harnessHome, { recursive: true });
await mkdir(projectRoot, { recursive: true });
const env = {
  ...process.env,
  HARNESS_HOME: harnessHome,
  HARNESS_COMPANY_OS_TOKEN: token,
};
delete env.HARNESS_ROOT;
delete env.HARNESS_PROJECT;
delete env.HARNESS_PROJECT_ID;
delete env.HARNESS_SPACE;
delete env.HARNESS_COMPANY;

runHarness(["init"], env, projectRoot);
runHarness(["company", "init", "--id", "company-a", "--name", "Company A"], env, projectRoot);
runHarness(["company", "init", "--id", "company-b", "--name", "Company B"], env, projectRoot);
runHarness(["company", "switch", "company-a"], env, projectRoot);

const apiPort = await freePort();
const apiBase = `http://127.0.0.1:${apiPort}`;
let runtime = null;
const runtimeLogs = [];
function startRuntime() {
  runtime = spawn(harness, ["serve", "--addr", `127.0.0.1:${apiPort}`, "--no-truncate"], {
    cwd: projectRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  runtime.stdout.on("data", (chunk) => runtimeLogs.push(chunk.toString()));
  runtime.stderr.on("data", (chunk) => runtimeLogs.push(chunk.toString()));
}
async function stopRuntime() {
  if (!runtime || runtime.exitCode !== null || runtime.signalCode !== null) return;
  const child = runtime;
  await new Promise((resolveExit) => {
    let settled = false;
    const finish = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolveExit();
    };
    child.once("exit", finish);
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      finish();
    }, 5_000);
    if (!child.kill("SIGTERM")) finish();
  });
}

startRuntime();
await waitFor(async () => (await fetch(`${apiBase}/health`).catch(() => null))?.ok, "real Runtime health");
const { current: projectId } = await requestJson(apiBase, "/v1/projects");
const { current: spaceId } = await requestJson(apiBase, "/v1/spaces");

async function postCompany(company, endpoint, record, administrative = true) {
  const query = new URLSearchParams({ company, project: projectId, space: spaceId });
  return await requestJson(apiBase, `/v1/company-os/${endpoint}?${query}`, {
    body: administrative ? admin(record) : record,
  });
}

for (const company of ["company-a", "company-b"]) {
  await postCompany(company, "actors", {
    actor_type: "human",
    actor: {
      id: "human-live-owner", display_name: `Live Owner ${company}`, title: "Owner",
      status: "active", availability: "available", membership_refs: [],
      responsibility_summary: "Owns real Dashboard Runtime acceptance.",
      permission_policy_refs: ["company_os.admin", "company.records.write", "company.work.execute"],
      authority_policy_refs: ["company_os.admin"], created_at: now, updated_at: now,
    },
  }, false);
  await postCompany(company, "documents", documentRecord(`document-seed-${company}`, `Seed document ${company}`));
}

const vite = await createViteServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: {
    host: "127.0.0.1",
    port: 0,
    proxy: {
      "/v1": { target: apiBase, changeOrigin: true },
      "/health": { target: apiBase, changeOrigin: true },
    },
  },
  logLevel: "silent",
});
await vite.listen();
const appBase = `http://127.0.0.1:${vite.httpServer.address().port}`;
const browser = await chromium.launch({ headless: true });
let context;

try {
  await mkdir(evidenceRoot, { recursive: true });
  context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.addInitScript(() => {
    window.__dashboardVisibility = "visible";
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => window.__dashboardVisibility,
    });
  });

  let snapshotReads = 0;
  let delayNextCompanyBSnapshotMs = 0;
  let delayedCompanyB = null;
  await page.route("**/v1/snapshot?**", async (route) => {
    snapshotReads += 1;
    const url = new URL(route.request().url());
    if (delayedCompanyB && url.searchParams.get("company") === "company-b") {
      const gate = delayedCompanyB;
      delayedCompanyB = null;
      const response = await route.fetch();
      gate.started();
      await gate.releasePromise;
      await route.fulfill({ response });
      return;
    }
    if (delayNextCompanyBSnapshotMs > 0 && url.searchParams.get("company") === "company-b") {
      const delay = delayNextCompanyBSnapshotMs;
      delayNextCompanyBSnapshotMs = 0;
      const response = await route.fetch();
      await new Promise((resolveDelay) => setTimeout(resolveDelay, delay));
      await route.fulfill({ response });
      return;
    }
    await route.continue();
  });

  const query = new URLSearchParams({
    api: appBase, project: projectId, space: spaceId, company: "company-b", surface: "docs",
  });
  await page.goto(`${appBase}/?${query}`, { waitUntil: "domcontentloaded", timeout: 20_000 });
  await waitForDomain(page, "runtime", "live");
  await waitForText(page, "Seed document company-b");
  check(pageErrors.length === 0, `real Runtime page opens without browser errors (${pageErrors[0] ?? "none"})`);
  check(await page.locator('[aria-label="Scoped domain freshness"]').count() === 1, "freshness is one accessible scoped domain group");
  for (const domain of ["works", "docs", "organization", "runtime"]) {
    check(await page.locator(`[data-freshness-domain="${domain}"]`).count() === 1, `${domain} freshness is exposed independently`);
  }

  const defaultBefore = await requestJson(apiBase, "/v1/companies/current");
  await page.getByLabel("Active company").selectOption("company-a");
  await waitForDomain(page, "runtime", "live");
  await waitForText(page, "Seed document company-a");
  const defaultAfter = await requestJson(apiBase, "/v1/companies/current");
  check(defaultBefore.current === "company-a" && defaultAfter.current === "company-a", "ordinary page selection does not mutate the CLI/server Company default");
  await page.getByLabel("Active company").selectOption("company-b");
  await waitForDomain(page, "runtime", "live");

  // Every write below is external to the browser and lands in the real Company
  // Store. The Runtime watcher emits freshness-only invalidations; the browser
  // may display a row only after its authoritative scoped snapshot converges.
  await navigate(page, "Work");
  delayNextCompanyBSnapshotMs = 400;
  await postCompany("company-b", "work-items", workRecord("work-live-external", "External Work converged", "document-seed-company-b"));
  await waitForDomain(page, "works", "stale");
  check(await page.locator('[data-freshness-domain="docs"][data-freshness-status="live"]').count() === 1, "Work invalidation leaves Docs freshness truthful and independent");
  await waitForText(page, "External Work converged");
  check(true, "external Work write converges into the open page without reload");

  await navigate(page, "Docs");
  delayNextCompanyBSnapshotMs = 400;
  await postCompany("company-b", "documents", documentRecord("document-live-external", "External Docs converged"));
  await waitForDomain(page, "docs", "stale");
  check(await page.locator('[data-freshness-domain="works"][data-freshness-status="live"]').count() === 1, "Docs invalidation leaves Works freshness truthful and independent");
  await waitForText(page, "External Docs converged");
  check(true, "external Docs write converges into the open page without reload");

  await navigate(page, "Organization");
  delayNextCompanyBSnapshotMs = 400;
  await postCompany("company-b", "org-units", orgUnitRecord("org-live-external", "External Org converged"));
  await waitForDomain(page, "organization", "stale");
  check(await page.locator('[data-freshness-domain="docs"][data-freshness-status="live"]').count() === 1, "Org invalidation leaves Docs freshness truthful and independent");
  await waitForText(page, "External Org converged");
  check(true, "external Organization write converges into the open page without reload");

  // Real response, deliberately delivered late: switch the scope while B's
  // captured Runtime snapshot is in flight, then release it. Generation/scope
  // guards must prevent B from overwriting A.
  let signalStarted;
  let releaseDelayed;
  const startedPromise = new Promise((resolveStarted) => { signalStarted = resolveStarted; });
  const releasePromise = new Promise((resolveRelease) => { releaseDelayed = resolveRelease; });
  delayedCompanyB = { started: signalStarted, releasePromise };
  await postCompany("company-b", "work-items", workRecord("work-live-delayed", "Delayed Company B response", "document-seed-company-b"));
  await startedPromise;
  await navigate(page, "Docs");
  await page.getByLabel("Active company").selectOption("company-a");
  await waitForText(page, "Seed document company-a");
  releaseDelayed();
  await new Promise((resolveWait) => setTimeout(resolveWait, 500));
  check(!(await page.locator("body").innerText()).includes("Delayed Company B response"), "delayed stale Runtime response cannot overwrite the current Company scope");

  // External registry creation changes the CLI default as a CLI operation, then
  // we restore it. Visibility recovery refreshes the picker while this tab keeps
  // its explicit Company A scope and does not display Company C truth.
  runHarness(["company", "init", "--id", "company-c", "--name", "Externally Created Company"], env, projectRoot);
  runHarness(["company", "switch", "company-a"], env, projectRoot);
  const beforeVisibilityReads = snapshotReads;
  await page.evaluate(() => { window.__dashboardVisibility = "hidden"; document.dispatchEvent(new Event("visibilitychange")); });
  await page.evaluate(() => { window.__dashboardVisibility = "visible"; document.dispatchEvent(new Event("visibilitychange")); });
  await waitFor(() => snapshotReads > beforeVisibilityReads, "visibility recovery snapshot");
  await waitFor(async () => await page.getByLabel("Active company").locator('option[value="company-c"]').count() === 1, "external Company appears in picker");
  check(await page.getByLabel("Active company").inputValue() === "company-a", "external Company creation refreshes picker without changing tab scope");

  // Stop and restart the actual Runtime on the same stores. A CLI write while it
  // is down is recovered by the authoritative reconnect snapshot; no SSE replay
  // or fixture row is involved.
  await stopRuntime();
  await context.setOffline(true);
  await waitForDomain(page, "runtime", "reconnecting");
  runHarness([
    "--company", "company-a", "company", "org", "create-unit",
    "--id", "org-reconnect-live", "--organization", "company",
    "--name", "Reconnect Org Unit", "--purpose", "Written while Runtime is disconnected",
    "--human-lead", "human-live-owner", "--policy", "company.records.write",
    "--authority", "human-live-owner",
  ], env, projectRoot);
  startRuntime();
  await waitFor(async () => (await fetch(`${apiBase}/health`).catch(() => null))?.ok, "restarted Runtime health");
  await context.setOffline(false);
  await navigate(page, "Organization");
  await waitForDomain(page, "runtime", "live");
  await waitForText(page, "Reconnect Org Unit");
  check(true, "disconnect/reconnect recovers external Organization truth through a full scoped snapshot");

  const stalePage = await context.newPage();
  const staleQuery = new URLSearchParams({
    api: appBase, project: projectId, space: "missing-space", company: "missing-company", surface: "docs",
  });
  await stalePage.goto(`${appBase}/?${staleQuery}`, { waitUntil: "domcontentloaded", timeout: 20_000 });
  await waitFor(async () => {
    const url = new URL(stalePage.url());
    return url.searchParams.get("space") === spaceId && url.searchParams.get("company") === "company-a";
  }, "stale selector recovery");
  await waitForDomain(stalePage, "runtime", "live");
  check((await stalePage.locator("body").innerText()).includes("was not found; recovered"), "stale Company/Space selectors recover visibly and fail closed before current-scope truth");
  await stalePage.close();

  await page.screenshot({ path: join(evidenceRoot, "runtime-live-converged.png"), fullPage: true });
  await writeFile(join(evidenceRoot, "evidence.json"), `${JSON.stringify({
    contract: "dashboard-runtime-live-browser-v1",
    runtime: harness,
    api_base: apiBase,
    project_id: projectId,
    space_id: spaceId,
    company_scope: "company-a",
    snapshot_reads: snapshotReads,
    checks: { passed, failed },
  }, null, 2)}\n`);
} finally {
  if (context) await context.setOffline(false).catch(() => {});
  if (context) await context.close();
  await browser.close();
  await vite.close();
  await stopRuntime();
  if (failed > 0) console.error(runtimeLogs.slice(-20).join(""));
  await rm(temporaryRoot, { recursive: true, force: true });
}

console.log(`\nDashboard real Runtime/browser checks: ${passed} pass, ${failed} fail`);
console.log(`Visual evidence: ${evidenceRoot}`);
process.exit(failed === 0 ? 0 : 1);
