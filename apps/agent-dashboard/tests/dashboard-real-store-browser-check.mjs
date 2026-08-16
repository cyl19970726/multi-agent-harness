#!/usr/bin/env node
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const config = {
  api: process.env.DASHBOARD_REAL_STORE_API,
  space: process.env.DASHBOARD_REAL_STORE_SPACE,
  project: process.env.DASHBOARD_REAL_STORE_PROJECT,
  teamRun: process.env.DASHBOARD_REAL_STORE_TEAM_RUN,
  teamName: process.env.DASHBOARD_REAL_STORE_TEAM_NAME,
  host: process.env.DASHBOARD_REAL_STORE_HOST,
  hostName: process.env.DASHBOARD_REAL_STORE_HOST_NAME,
  hostToken: process.env.DASHBOARD_REAL_STORE_HOST_TOKEN,
  member: process.env.DASHBOARD_REAL_STORE_MEMBER,
  memberName: process.env.DASHBOARD_REAL_STORE_MEMBER_NAME,
  memberRun: process.env.DASHBOARD_REAL_STORE_MEMBER_RUN,
  memberToken: process.env.DASHBOARD_REAL_STORE_MEMBER_TOKEN,
  node: process.env.DASHBOARD_REAL_STORE_NODE,
  nodeToken: process.env.DASHBOARD_REAL_STORE_NODE_TOKEN,
};
for (const [key, value] of Object.entries(config)) {
  assert.ok(value, `missing real-store browser setting: ${key}`);
}

process.env.HARNESS_CAPTURE_API_PROXY = config.api;
const evidenceDir = resolve(
  process.env.DASHBOARD_REAL_STORE_EVIDENCE_DIR
    ?? join(dashboardRoot, "..", "..", ".visual-evidence", "dev33-real-store"),
);
await mkdir(evidenceDir, { recursive: true });

const vite = await createServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 0 },
  logLevel: "silent",
});
await vite.listen();
const base = `http://127.0.0.1:${vite.httpServer.address().port}`;
const browser = await chromium.launch({ headless: true });

const consoleErrors = [];
const pageErrors = [];
const httpErrors = [];
const snapshotStarts = new Map();
const snapshotLatenciesMs = [];
let snapshotRequestCount = 0;
let inFlightSnapshots = 0;
let maxInFlightSnapshots = 0;
const surfaceLatenciesMs = {};

function isSnapshot(request) {
  return new URL(request.url()).pathname === "/v1/snapshot";
}

async function waitFor(predicate, label, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    assert.ok(Date.now() < deadline, `timed out waiting for ${label}`);
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 50));
  }
}

try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  await page.addInitScript((token) => {
    window.__AGENTFIRM_BOOTSTRAP__ = { capabilityToken: token };
  }, config.hostToken);
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("response", (response) => {
    if (response.status() >= 400) httpErrors.push(`${response.status()} ${response.url()}`);
  });
  page.on("request", (request) => {
    if (!isSnapshot(request)) return;
    snapshotStarts.set(request, Date.now());
    snapshotRequestCount += 1;
    inFlightSnapshots += 1;
    maxInFlightSnapshots = Math.max(maxInFlightSnapshots, inFlightSnapshots);
  });
  const settleSnapshot = (request) => {
    if (!isSnapshot(request)) return;
    const startedAt = snapshotStarts.get(request);
    if (startedAt != null) snapshotLatenciesMs.push(Date.now() - startedAt);
    snapshotStarts.delete(request);
    inFlightSnapshots -= 1;
  };
  page.on("requestfinished", settleSnapshot);
  page.on("requestfailed", settleSnapshot);

  const scope = `space=${encodeURIComponent(config.space)}&project=${encodeURIComponent(config.project)}`;
  await page.goto(`${base}/?surface=team&${scope}`, { waitUntil: "domcontentloaded" });
  await page.getByText("Loading Agent Teams…", { exact: true }).waitFor();
  await page.getByText("Loading Execution Nodes…", { exact: true }).waitFor();
  assert.equal(await page.getByText("No Agent Team runs", { exact: true }).count(), 0, "slow bootstrap rendered a false empty Team state");
  assert.equal(await page.getByText("No ExecutionNode has been initialized.", { exact: true }).count(), 0, "slow bootstrap rendered a false empty Node state");

  await waitFor(() => snapshotLatenciesMs.length >= 1, "first full snapshot");
  await page.getByText(config.teamName, { exact: true }).first().waitFor();
  const firstSuccessCommittedBeforeFollowUp = snapshotLatenciesMs.length === 1;
  assert.equal(firstSuccessCommittedBeforeFollowUp, true, "first successful full snapshot did not commit before its follow-up completed");
  await waitFor(() => snapshotLatenciesMs.length >= 2, "coalesced full-snapshot follow-up");
  const initialSnapshotRequestCount = snapshotRequestCount;
  assert.equal(initialSnapshotRequestCount, 2, "slow bootstrap must issue one initial full snapshot and one coalesced follow-up");
  assert.equal(maxInFlightSnapshots, 1, "full snapshots overlapped");
  await page.screenshot({ path: join(evidenceDir, "agent-teams-home.png"), animations: "disabled" });

  async function navigate(name, params, ready) {
    const startedAt = Date.now();
    const search = new URLSearchParams({ ...params, space: config.space, project: config.project }).toString();
    await page.evaluate((nextSearch) => {
      window.history.pushState({}, "", `/?${nextSearch}`);
      window.dispatchEvent(new PopStateEvent("popstate"));
    }, search);
    await ready();
    surfaceLatenciesMs[name] = Date.now() - startedAt;
    await page.screenshot({ path: join(evidenceDir, `${name}.png`), animations: "disabled" });
  }

  await navigate("company-work", { surface: "work" }, async () => {
    await page.getByRole("heading", { name: "Company Work", exact: true }).waitFor();
    await page.getByText(/Work items$/).waitFor();
  });
  await navigate("team-works", { surface: "team", team: config.teamRun, teamTab: "works" }, async () => {
    await page.getByRole("heading", { name: config.teamName, exact: true }).waitFor();
    await page.getByRole("tabpanel").waitFor();
    assert.equal(await page.getByRole("tab", { name: /^Works/ }).getAttribute("aria-selected"), "true");
  });
  await navigate("team-activity", { surface: "team", team: config.teamRun, teamTab: "activity" }, async () => {
    await page.getByRole("tab", { name: /^Activity/ }).waitFor();
    assert.equal(await page.getByRole("tab", { name: /^Activity/ }).getAttribute("aria-selected"), "true");
  });
  await navigate("team-members", { surface: "team", team: config.teamRun, teamTab: "members" }, async () => {
    await page.getByRole("tab", { name: /^Members/ }).waitFor();
    assert.equal(await page.getByRole("tab", { name: /^Members/ }).getAttribute("aria-selected"), "true");
  });
  await navigate("host-console", { surface: "team", team: config.teamRun, teamMode: "host" }, async () => {
    await page.getByTestId("authenticated-host-console").waitFor();
    await page.getByText("Lead Inbox, runtime and decisions", { exact: true }).waitFor();
  });
  await navigate("host-agent-workspace", { surface: "team", team: config.teamRun, conversation: config.host }, async () => {
    await page.getByTestId("agent-workspace").waitFor();
    await page.getByTestId("agent-workspace-identity").getByText(config.hostName, { exact: true }).waitFor();
    await page.getByText("Host-owned Session", { exact: true }).waitFor();
  });

  await page.evaluate((token) => {
    window.__AGENTFIRM_BOOTSTRAP__ = { capabilityToken: token };
  }, config.memberToken);
  await navigate("member-agent-workspace", { surface: "team", team: config.teamRun, conversation: config.member, memberRun: config.memberRun }, async () => {
    await page.getByTestId("agent-workspace").waitFor();
    await page.getByTestId("agent-workspace-identity").getByText(config.memberName, { exact: true }).waitFor();
    await page.getByTestId("agent-workspace-modebar").getByText(/Owner-bound Session|Native Session unavailable/).waitFor();
  });

  await page.evaluate((token) => {
    window.__AGENTFIRM_BOOTSTRAP__ = { capabilityToken: token };
  }, config.nodeToken);
  await navigate("nodes", { surface: "operator", node: config.node }, async () => {
    await page.getByRole("heading", { name: "Operator View", exact: true }).waitFor();
    await page.getByText("Daemon generation", { exact: true }).waitFor();
  });

  assert.deepEqual(consoleErrors, [], `console errors: ${consoleErrors.join(" | ")}`);
  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(" | ")}`);
  assert.deepEqual(httpErrors, [], `HTTP errors: ${httpErrors.join(" | ")}`);

  const result = {
    evidence_kind: "canonical_store_live_clone",
    execution_space_id: config.space,
    project_binding_id: config.project,
    initial_snapshot_request_count: initialSnapshotRequestCount,
    total_snapshot_request_count: snapshotRequestCount,
    max_in_flight_full_snapshots: maxInFlightSnapshots,
    full_snapshot_latencies_ms: snapshotLatenciesMs,
    first_success_committed_before_follow_up: firstSuccessCommittedBeforeFollowUp,
    surface_latencies_ms: surfaceLatenciesMs,
    console_errors: consoleErrors,
    page_errors: pageErrors,
    http_errors: httpErrors,
    checked_at: new Date().toISOString(),
  };
  await writeFile(join(evidenceDir, "acceptance.json"), `${JSON.stringify(result, null, 2)}\n`);
  console.log("Dashboard real-store retained surfaces: PASS");
  console.log(JSON.stringify(result, null, 2));
} finally {
  await browser.close();
  await vite.close();
}
