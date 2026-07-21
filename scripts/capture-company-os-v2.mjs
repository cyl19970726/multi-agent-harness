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

function flag(name) {
  return process.argv.includes(name);
}

function hash(buffer) {
  return `sha256:${createHash("sha256").update(buffer).digest("hex")}`;
}

function latestRecords(value) {
  return Array.isArray(value) ? value.map((item) => item?.record && typeof item.record === "object" ? { ...item.record, ...item } : item) : [];
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
  const approvalActionToken = argument("--approval-action-token");
  if (approvalActionToken && dataMode !== "live") throw new Error("--approval-action-token requires --data-mode live");
  const workItemActionToken = argument("--workitem-action-token");
  if (workItemActionToken && dataMode !== "live") throw new Error("--workitem-action-token requires --data-mode live");
  const approvalActionDecision = argument("--approval-action-decision", "approved");
  if (!new Set(["approved", "rejected"]).has(approvalActionDecision)) throw new Error("--approval-action-decision must be approved or rejected");
  const outputRoot = resolve(argument("--output", join(repoRoot, ".visual-evidence/company-os-v2", runId)));
  const viewportWidth = Number(argument("--viewport-width", "1536"));
  const viewportHeight = Number(argument("--viewport-height", "1024"));
  const viewportName = argument("--viewport-name", `desktop-${viewportWidth}x${viewportHeight}`);
  if (!Number.isInteger(viewportWidth) || !Number.isInteger(viewportHeight) || viewportWidth < 320 || viewportHeight < 480) throw new Error("invalid capture viewport");
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
    const context = await browser.newContext({ viewport: { width: viewportWidth, height: viewportHeight }, deviceScaleFactor: 1, reducedMotion: "reduce" });
    if (dataMode === "fixture") await context.addInitScript((value) => { window.__COMPANY_OS_FIXTURE__ = value; }, fixture);
    const page = await context.newPage();
    const results = [];
    const workViewResults = [];
    for (const item of flag("--work-views-only") ? [] : cases) {
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
      await page.screenshot({ path, fullPage: false, timeout: 60_000 });
      const bytes = await readFile(path);
      results.push({ ...item, viewport: viewportName, final_url: page.url(), file: relative(repoRoot, path), sha256: hash(bytes), status: "captured" });
    }
    if (flag("--capture-work-views")) {
      const workRoot = join(outputRoot, "work-views");
      await mkdir(workRoot, { recursive: true });
      const url = new URL("/?surface=work", base);
      url.searchParams.set("api", dataMode === "live" ? base : "http://127.0.0.1:9");
      if (liveSource) url.searchParams.set("project", liveSource.project_id);
      await page.goto(url.toString(), { waitUntil: "domcontentloaded", timeout: 15_000 });
      const root = page.locator('[data-company-os-page="workboard"][data-company-os-ready="true"]').first();
      await root.waitFor({ state: "visible", timeout: 15_000 });
      const views = [
        ["overview", "Overview"], ["board", "Board"], ["all", "All Work"],
        ["milestones", "Milestones"], ["timeline", "Timeline"], ["workload", "Workload"],
      ];
      for (const [id, label] of views) {
        await root.getByRole("button", { name: label, exact: true }).click();
        await root.locator(`[data-work-view="${id}"]`).waitFor({ state: "visible" });
        const overflow = await page.evaluate(() => ({ width: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
        if (overflow.scroll > overflow.width) throw new Error(`work-${id} has horizontal overflow: ${JSON.stringify(overflow)}`);
        const path = join(workRoot, `work-${id}--${viewportName}.png`);
        await page.screenshot({ path, fullPage: false, timeout: 60_000 });
        workViewResults.push({ id, label, viewport: viewportName, file: relative(repoRoot, path), sha256: hash(await readFile(path)), status: "captured" });
      }
    }
    let workItemAction;
    if (workItemActionToken) {
      const actionRoot = join(outputRoot, "workitem-action");
      await mkdir(actionRoot, { recursive: true });
      const workItemId = "workitem-trademark-filing-brand-a";
      const url = new URL(`/?surface=work&workItem=${workItemId}`, base);
      url.searchParams.set("api", base);
      url.searchParams.set("project", liveSource.project_id);
      await page.goto(url.toString(), { waitUntil: "domcontentloaded", timeout: 15_000 });
      let root = page.locator('[data-company-os-page="work-item-focus"][data-company-os-ready="true"][data-company-os-data-mode="store-live"]').first();
      await root.waitFor({ state: "visible", timeout: 15_000 });
      await root.locator('[data-company-os-action-state="available"]').waitFor({ state: "visible", timeout: 15_000 });
      const waitingPath = join(actionRoot, "workitem-waiting--before.png");
      await page.screenshot({ path: waitingPath, fullPage: false, timeout: 60_000 });
      await root.locator("[data-company-os-work-note]").fill("Trademark preparation started by the assigned Standing Agent.");
      await root.locator("[data-company-os-action-token]").fill(workItemActionToken);
      await root.getByRole("button", { name: "Start preparation", exact: true }).click();
      await page.locator(`[data-company-os-ref="${workItemId}"][data-work-item-status="in_progress"]`).waitFor({ state: "visible", timeout: 15_000 });
      const progressPath = join(actionRoot, "workitem-in-progress--after-start.png");
      await page.screenshot({ path: progressPath, fullPage: false, timeout: 60_000 });
      root = page.locator('[data-company-os-page="work-item-focus"]').first();
      await root.locator("[data-company-os-work-note]").fill("Filing package and evidence are ready for accountable review.");
      await root.locator("[data-company-os-action-token]").fill(workItemActionToken);
      await root.getByRole("button", { name: "Submit result", exact: true }).click();
      await page.locator(`[data-company-os-ref="${workItemId}"][data-work-item-status="in_review"]`).waitFor({ state: "visible", timeout: 15_000 });
      const reviewPath = join(actionRoot, "workitem-in-review--after-submit.png");
      await page.screenshot({ path: reviewPath, fullPage: false, timeout: 60_000 });
      const snapshot = await readJson(liveSource.snapshot_endpoint, "post-submit WorkItem snapshot");
      const workItem = latestRecords(snapshot.company_os.work_items).find((record) => record.id === workItemId);
      const commands = latestRecords(snapshot.company_os.action_commands).filter((record) => record.command_name === "work_item.transition");
      const payments = latestRecords(snapshot.company_os.financial_records).filter((record) => record.type === "payment");
      if (workItem?.status !== "in_review" || workItem?.requested_by?.actor_type !== "human") throw new Error("browser WorkItem submission did not preserve Store truth and responsibility");
      if (commands.length < 2 || commands.some((command) => command.status !== "executed")) throw new Error("browser WorkItem start/submit lacks executed ActionCommands");
      if (payments.length !== 0) throw new Error("work_item.transition created a Payment");
      workItemAction = {
        status: "in_review",
        work_item_id: workItemId,
        action_command_ids: commands.map((command) => command.id),
        payment_count: payments.length,
        capability_storage: "browser-session-memory-only; omitted from evidence",
        waiting: { file: relative(repoRoot, waitingPath), sha256: hash(await readFile(waitingPath)) },
        in_progress: { file: relative(repoRoot, progressPath), sha256: hash(await readFile(progressPath)) },
        in_review: { file: relative(repoRoot, reviewPath), sha256: hash(await readFile(reviewPath)) },
      };
    }
    let approvalAction;
    if (approvalActionToken) {
      const actionRoot = join(outputRoot, "approval-action");
      await mkdir(actionRoot, { recursive: true });
      const approvalId = "approval-trademark-filing-fee-cn-2026-018";
      const decisionButton = approvalActionDecision === "approved" ? "Approve" : "Reject";
      let dispatchedBody;
      page.on("request", (request) => {
        if (new URL(request.url()).pathname === "/v1/company-os/actions/dispatch" && request.method() === "POST") {
          dispatchedBody = request.postDataJSON();
        }
      });
      const url = new URL(`/?surface=approvals&approval=${approvalId}`, base);
      url.searchParams.set("api", base);
      url.searchParams.set("project", liveSource.project_id);
      await page.goto(url.toString(), { waitUntil: "domcontentloaded", timeout: 15_000 });
      const root = page.locator('[data-company-os-page="approval-focus"][data-company-os-ready="true"][data-company-os-data-mode="store-live"]').first();
      await root.waitFor({ state: "visible", timeout: 15_000 });
      await root.locator('[data-company-os-action-state="available"]').waitFor({ state: "visible", timeout: 15_000 });
      const beforePath = join(actionRoot, "approval-requested--before.png");
      await page.screenshot({ path: beforePath, fullPage: false, timeout: 60_000 });
      await root.locator("[data-company-os-decision-note]").fill(`${approvalActionDecision === "approved" ? "Approved" : "Rejected"} in Store-live browser acceptance; no Payment is authorized.`);
      await root.locator("[data-company-os-action-token]").fill("invalid-browser-capability");
      await root.getByRole("button", { name: decisionButton, exact: true }).click();
      await page.getByText("missing or invalid Company OS transport capability", { exact: false }).waitFor({ state: "visible", timeout: 15_000 });
      const deniedSnapshot = await readJson(liveSource.snapshot_endpoint, "post-denial snapshot");
      const deniedApproval = latestRecords(deniedSnapshot.company_os.approvals).find((record) => record.id === approvalId);
      if (deniedApproval?.status !== "requested") throw new Error("invalid browser capability mutated the Approval");
      const denialPath = join(actionRoot, "approval-denied-invalid-capability.png");
      await page.screenshot({ path: denialPath, fullPage: false, timeout: 60_000 });
      await root.locator("[data-company-os-action-token]").fill(approvalActionToken);
      await root.getByRole("button", { name: decisionButton, exact: true }).click();
      await root.getByText(`Decision recorded: ${approvalActionDecision}.`, { exact: false }).waitFor({ state: "visible", timeout: 15_000 }).catch(async (error) => {
        const failurePath = join(actionRoot, "approval-action-failure.png");
        await page.screenshot({ path: failurePath, fullPage: false, timeout: 60_000 });
        const visible = (await page.locator("body").innerText()).replace(/\s+/g, " ").slice(0, 2_000);
        throw new Error(`${error.message}\nVisible browser state: ${visible}\nFailure screenshot: ${failurePath}`);
      });
      const afterPath = join(actionRoot, `approval-${approvalActionDecision}--after.png`);
      await page.screenshot({ path: afterPath, fullPage: false, timeout: 60_000 });
      const snapshot = await readJson(liveSource.snapshot_endpoint, "post-decision snapshot");
      const companyOs = snapshot.company_os;
      const approval = latestRecords(companyOs.approvals).find((record) => record.id === approvalId);
      const commands = latestRecords(companyOs.action_commands).filter((record) => record.command_name === "approval.decide");
      const command = commands.at(-1);
      const audits = latestRecords(companyOs.audit_events).filter((record) => record.action_command_id === command?.id);
      const commitment = latestRecords(companyOs.financial_records).find((record) => record.type === "commitment");
      const payments = latestRecords(companyOs.financial_records).filter((record) => record.type === "payment");
      if (approval?.status !== approvalActionDecision || approval?.decided_by?.[0]?.actor_type !== "human") throw new Error("browser decision did not persist the named Human decision");
      if (command?.status !== "executed" || command?.requested_by?.actor_type !== "human") throw new Error("browser decision lacks an executed Human ActionCommand");
      if (audits.length < 2) throw new Error("browser decision lacks authorization and execution audit events");
      if (commitment?.status !== "pending_approval") throw new Error("approval.decide incorrectly changed the Commitment");
      if (payments.length !== 0) throw new Error("approval.decide created a Payment");
      if (!dispatchedBody) throw new Error("browser decision request body was not observed");
      const replayResponse = await fetch(`${apiBaseUrl}/v1/company-os/actions/dispatch?project=${encodeURIComponent(liveSource.project_id)}`, {
        method: "POST",
        headers: { "content-type": "application/json", "x-harness-company-os-token": approvalActionToken },
        body: JSON.stringify(dispatchedBody),
      });
      const replayBody = await replayResponse.json();
      if (!replayResponse.ok || replayBody?.result?.idempotent_replay !== true) throw new Error(`approval decision replay was not idempotent: ${JSON.stringify(replayBody)}`);
      await writeFile(join(actionRoot, "post-decision-snapshot.json"), `${JSON.stringify(snapshot, null, 2)}\n`);
      approvalAction = {
        status: "passed",
        approval_id: approvalId,
        approval_status: approval.status,
        decided_by: approval.decided_by,
        action_command_id: command.id,
        action_command_status: command.status,
        audit_event_refs: audits.map((event) => event.id),
        commitment_status: commitment.status,
        payment_count: payments.length,
        idempotent_replay: true,
        capability_storage: "browser-session-memory-only; omitted from evidence",
        denied_invalid_capability: { approval_status: deniedApproval.status, file: relative(repoRoot, denialPath), sha256: hash(await readFile(denialPath)) },
        before: { file: relative(repoRoot, beforePath), sha256: hash(await readFile(beforePath)) },
        after: { file: relative(repoRoot, afterPath), sha256: hash(await readFile(afterPath)) },
        snapshot: relative(repoRoot, join(actionRoot, "post-decision-snapshot.json")),
      };
    }
    if (workItemActionToken && approvalActionDecision === "approved" && approvalAction?.status === "passed") {
      const actionRoot = join(outputRoot, "workitem-action");
      const workItemId = workItemAction.work_item_id;
      let completedBody;
      page.on("request", (request) => {
        if (new URL(request.url()).pathname !== "/v1/company-os/actions/dispatch" || request.method() !== "POST") return;
        const body = request.postDataJSON();
        if (body?.command_name === "work_item.transition" && body?.payload?.record?.status === "completed") completedBody = body;
      });
      const url = new URL(`/?surface=work&workItem=${workItemId}`, base);
      url.searchParams.set("api", base);
      url.searchParams.set("project", liveSource.project_id);
      await page.goto(url.toString(), { waitUntil: "domcontentloaded", timeout: 15_000 });
      const root = page.locator('[data-company-os-page="work-item-focus"][data-company-os-ready="true"][data-company-os-data-mode="store-live"]').first();
      await root.locator("[data-company-os-work-note]").fill("Accountable owner accepted the linked result after Human approval.");
      await root.locator("[data-company-os-action-token]").fill(workItemActionToken);
      await root.getByRole("button", { name: "Complete", exact: true }).click();
      await page.locator(`[data-company-os-ref="${workItemId}"][data-work-item-status="completed"]`).waitFor({ state: "visible", timeout: 15_000 });
      const completedPath = join(actionRoot, "workitem-completed--after-approval.png");
      await page.screenshot({ path: completedPath, fullPage: false, timeout: 60_000 });
      const snapshot = await readJson(liveSource.snapshot_endpoint, "completed WorkItem snapshot");
      const workItem = latestRecords(snapshot.company_os.work_items).find((record) => record.id === workItemId);
      const payments = latestRecords(snapshot.company_os.financial_records).filter((record) => record.type === "payment");
      if (workItem?.status !== "completed" || !workItem?.completed_at || !workItem?.outcome_summary) throw new Error("browser WorkItem completion lacks durable result state");
      if (payments.length !== 0) throw new Error("WorkItem completion created a Payment");
      if (!completedBody) throw new Error("browser WorkItem completion request body was not observed");
      const replayResponse = await fetch(`${apiBaseUrl}/v1/company-os/actions/dispatch?project=${encodeURIComponent(liveSource.project_id)}`, {
        method: "POST",
        headers: { "content-type": "application/json", "x-harness-company-os-token": workItemActionToken },
        body: JSON.stringify(completedBody),
      });
      const replayBody = await replayResponse.json();
      if (!replayResponse.ok || replayBody?.result?.idempotent_replay !== true) throw new Error(`WorkItem completion replay was not idempotent: ${JSON.stringify(replayBody)}`);
      workItemAction = {
        ...workItemAction,
        status: "completed",
        completed_at: workItem.completed_at,
        outcome_summary: workItem.outcome_summary,
        payment_count: payments.length,
        idempotent_replay: true,
        completed: { file: relative(repoRoot, completedPath), sha256: hash(await readFile(completedPath)) },
      };
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
      ...(workViewResults.length ? { work_views: workViewResults } : {}),
      ...(approvalAction ? { approval_action: approvalAction } : {}),
      ...(workItemAction ? { work_item_action: workItemAction } : {}),
    };
    await writeFile(join(outputRoot, "capture-run.json"), `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(JSON.stringify(manifest, null, 2));
  } finally {
    await browser?.close().catch(() => {});
    await stopProcess(vite);
  }
}

main().catch((error) => { console.error(error.stack || error.message); process.exit(1); });
