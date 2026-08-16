#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer as createViteServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = join(dashboardRoot, "fixtures/workbench-layout-v2-native-v1");
const roleFixtureRoot = join(dashboardRoot, "fixtures/wave4-local-agentfirm-v1");

async function json(name) {
  return JSON.parse(await readFile(join(roleFixtureRoot, `${name}.json`), "utf8"));
}

async function jsonl(name) {
  return (await readFile(join(fixtureRoot, `${name}.jsonl`), "utf8"))
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

const workOperations = await jsonl("work_operations");
const baseSnapshot = {
  generated_at: "baseline",
  teams: await jsonl("teams"),
  missions: await jsonl("missions"),
  legacy_waves: await jsonl("waves"),
  team_runs: await jsonl("team_runs"),
  member_runs: await jsonl("member_runs"),
  team_messages: await jsonl("team_messages"),
  works: workOperations.map((operation) => operation.work),
  work_events: workOperations.map((operation) => operation.event),
  work_deliveries: workOperations.flatMap((operation) => operation.deliveries ?? []),
  member_actions: await jsonl("member_actions"),
  delegation_runs: await jsonl("delegation_runs"),
  team_run_events: await jsonl("team_run_events"),
  evidence: await jsonl("evidence"),
  members: [],
  messages: [],
  events: [],
  provider_child_threads: [],
  workflow_runs: [],
  workflow_steps: [],
  workflow_patches: [],
  workflow_artifact_manifests: [],
  team_supervisor_leases: [],
  team_member_close_requests: [],
  execution_nodes: [{
    id: "node-dev33",
    display_name: "DEV-33 Node",
    status: "active",
  }],
};

function snapshot(label) {
  const template = baseSnapshot.works[0];
  const executionNodes = baseSnapshot.execution_nodes.map((node) => ({
    ...node,
    display_name: `${node.display_name} · ${label}`,
  }));
  return {
    ...structuredClone(baseSnapshot),
    generated_at: label,
    execution_nodes: executionNodes,
    works: template ? [...baseSnapshot.works, { ...template, id: `dev33-${label}`, title: label }] : [],
  };
}

const interruptAction = {
  kind: "interrupt_member_run",
  target_ref: { kind: "member_run", id: "member-run-1" },
  required_version: 1,
  disabled_reason: null,
};
const sendMessageAction = {
  kind: "send_message",
  target_ref: { kind: "team_run", id: "teamrun-mission-current" },
  required_version: 1,
  disabled_reason: null,
};
const teamWorkspace = await json("team-workspace");
teamWorkspace.source_execution_space_id = "fixture-space";
teamWorkspace.data.team.team_id = "teamrun-mission-current";
teamWorkspace.data.team.latest_run = { id: "teamrun-mission-current", status: "running" };
const hostConsole = await json("host-console");
hostConsole.source_execution_space_id = "fixture-space";
hostConsole.allowed_actions = [interruptAction, sendMessageAction];
hostConsole.data.team_ref = "teamrun-mission-current";
hostConsole.data.all_works = teamWorkspace.data.works;
const agentWorkspace = await json("agent-workspace");
agentWorkspace.source_execution_space_id = "fixture-space";
agentWorkspace.allowed_actions = [interruptAction];
hostConsole.data.member_capacity = agentWorkspace.data.roster.filter((member) => !member.is_host);
hostConsole.data.member_runtime = hostConsole.data.member_capacity;

const vite = await createViteServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 0 },
  logLevel: "silent",
});
await vite.listen();
const appBase = `http://127.0.0.1:${vite.httpServer.address().port}`;
const apiBase = "http://dev33.fixture";
const browser = await chromium.launch({ headless: true });
const unexpectedFixturePaths = [];

function delay(ms) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, ms));
}

function responseFor(url) {
  if (url.pathname === "/v1/projects") return {
    current: "fixture-project",
    projects: [{ id: "fixture-project", project_root: "/tmp/dev33", kind: "repo", is_git_repo: true, is_current: true }],
  };
  if (url.pathname === "/v1/spaces") return {
    current: "fixture-space",
    spaces: [{ id: "fixture-space", name: "Fixture Space", store_root: "/tmp/dev33", is_current: true }],
  };
  if (url.pathname === "/v1/companies") return { current: "", companies: [] };
  if (url.pathname === "/v1/workflows") return [];
  if (url.pathname === "/v1/meta") return {
    git_rev: "dev33",
    built_at: null,
    store_root: "/tmp/dev33",
    latest_op_seq: 1,
    server_version: "test",
    schema_version: "agentfirm.role_views.v1",
    protocol_version: "agentfirm-member-trust/1",
    action_manifest_version: "agentfirm.role_actions.v1",
    capability_auth: "x-agentfirm-token",
    build_sha: "fbc401646f66b69a0269622c489441cfe643b54f",
  };
  if (url.pathname.startsWith("/v1/views/team-workspace/")) return teamWorkspace;
  if (url.pathname.startsWith("/v1/views/host-console/")) return hostConsole;
  if (url.pathname.startsWith("/v1/views/agent-workspace/")) return agentWorkspace;
  return null;
}

async function installCommonRoutes(page, snapshotHandler, writes) {
  await page.addInitScript(() => {
    window.__AGENTFIRM_BOOTSTRAP__ = { capabilityToken: "dev33-token" };
    class QuietEventSource {
      addEventListener(kind, listener) {
        if (kind === "snapshot") {
          setTimeout(() => listener({ data: JSON.stringify({
            generated_at: new Date().toISOString(),
            execution_space_id: "fixture-space",
            stream_epoch: "dev33-stream",
          }) }), 0);
        }
      }
      close() {}
    }
    Object.defineProperty(window, "EventSource", { configurable: true, value: QuietEventSource });
  });
  await page.route("**/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.method() === "POST") {
      writes.push({ path: url.pathname, at: Date.now() });
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ snapshot: snapshot("role write committed") }),
      });
    }
    if (url.pathname === "/v1/snapshot" || /\/v1\/team-runs\/[^/]+\/snapshot$/.test(url.pathname)) {
      return snapshotHandler(route, url);
    }
    const body = responseFor(url);
    if (body === null) unexpectedFixturePaths.push(`${request.method()} ${url.pathname}`);
    return route.fulfill({
      status: body === null ? 404 : 200,
      contentType: "application/json",
      body: JSON.stringify(body ?? { error: { message: url.pathname } }),
    });
  });
}

async function executeInterrupt(page, label) {
  const action = page.getByRole("button", { name: "interrupt member run" });
  await action.waitFor({ timeout: 8_000 });
  assert.equal(await action.isEnabled(), true, `${label} RoleView action was disabled by ambient snapshot latency`);
  await action.click();
  await page.getByLabel("Interrupt reason").fill(`DEV-33 ${label} independent gate`);
  await page.getByRole("button", { name: "Execute action" }).click();
}

async function waitFor(predicate, message, timeout = 8_000) {
  const started = Date.now();
  while (Date.now() - started < timeout) {
    if (predicate()) return;
    await delay(25);
  }
  throw new Error(`timed out: ${message}`);
}

const query = (extra = {}) => new URLSearchParams({
  api: apiBase,
  project: "fixture-project",
  space: "fixture-space",
  ...extra,
});

try {
  const fullPage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  const fullErrors = [];
  fullPage.on("pageerror", (error) => fullErrors.push(`page: ${error.message}`));
  fullPage.on("console", (message) => {
    if (message.type() === "error") fullErrors.push(`console: ${message.text()}`);
  });
  const fullMetrics = { requests: 0, active: 0, maxActive: 0, starts: [], writes: [] };
  await installCommonRoutes(fullPage, async (route, url) => {
    assert.equal(url.pathname, "/v1/snapshot", "slow acceptance must exercise the full snapshot endpoint");
    const index = ++fullMetrics.requests;
    fullMetrics.active += 1;
    fullMetrics.maxActive = Math.max(fullMetrics.maxActive, fullMetrics.active);
    fullMetrics.starts.push(Date.now());
    const label = index === 1 ? "slow first success" : "coalesced dirty follow-up";
    await delay(index === 1 ? 12_000 : 1_200);
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(snapshot(label)) });
    fullMetrics.active -= 1;
  }, fullMetrics.writes);

  const startedAt = Date.now();
  await fullPage.goto(`${appBase}/?${query({ surface: "team" })}`, { waitUntil: "domcontentloaded" });
  await fullPage.getByText("Loading Agent Teams…", { exact: true }).waitFor();
  await fullPage.getByText("Loading Execution Nodes…", { exact: true }).waitFor();
  await delay(4_500);
  assert.equal(fullMetrics.requests, 1, "retry interval started a second full snapshot while the first was pending");
  assert.equal(fullMetrics.maxActive, 1, "more than one full snapshot was in flight");
  assert.equal(await fullPage.getByText("No Agent Team runs", { exact: true }).count(), 0, "loading rendered a false zero-Team claim");
  assert.equal(await fullPage.getByText("No ExecutionNode has been initialized.", { exact: true }).count(), 0, "loading rendered a false zero-Node claim");

  await fullPage.getByText("Platform Foundation Team", { exact: true }).first().waitFor({ timeout: 15_000 });
  const firstLoadLatencyMs = Date.now() - startedAt;
  await fullPage.getByText("DEV-33 Node · slow first success", { exact: true }).waitFor({ timeout: 4_000 });
  await fullPage.getByText("DEV-33 Node · coalesced dirty follow-up", { exact: true }).waitFor({ timeout: 4_000 });
  assert.equal(fullMetrics.requests, 2, "retry ticks did not coalesce into exactly one dirty follow-up");
  assert.equal(fullMetrics.maxActive, 1, "dirty follow-up overlapped the first successful response");
  assert.equal(fullErrors.length, 0, `slow full snapshot produced browser errors: ${fullErrors.join(" | ")}`);

  const rolePageErrors = [];
  const roleHttpErrors = [];
  const collectRoleErrors = (page, label) => {
    page.on("pageerror", (error) => rolePageErrors.push(`${label} page: ${error.message}`));
    page.on("console", (message) => {
      if (message.type() === "error") rolePageErrors.push(`${label} console: ${message.text()}`);
    });
    page.on("response", (response) => {
      if (response.status() >= 400) roleHttpErrors.push(`${label} ${response.status()} ${response.url()}`);
    });
  };

  const teamPage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  collectRoleErrors(teamPage, "Team");
  const teamWrites = [];
  await installCommonRoutes(teamPage, async (route) => {
    await delay(12_000);
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(snapshot("late ambient team snapshot")) });
  }, teamWrites);
  await teamPage.goto(`${appBase}/?${query({ surface: "team", team: "teamrun-mission-current", teamTab: "activity" })}`, { waitUntil: "domcontentloaded" });
  await teamPage.getByRole("button", { name: "Compose team message", exact: true }).click();
  await teamPage.getByRole("heading", { name: "Team message", exact: true }).waitFor();
  await teamPage.getByLabel("Recipient").selectOption(hostConsole.data.member_capacity[0].agent_member_ref.id);
  await teamPage.getByRole("textbox", { name: "Message", exact: true }).fill("DEV-33 Team RoleView independent gate");
  const sendMessage = teamPage.getByRole("button", { name: "Send message", exact: true });
  assert.equal(await sendMessage.isEnabled(), true, "Team RoleView action was disabled by ambient snapshot latency");
  await sendMessage.click();
  await waitFor(() => teamWrites.length === 1, "Team RoleView write");
  assert.equal(teamWrites.length, 1, "current Team RoleView did not execute while ambient snapshot was pending");

  const hostPage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  collectRoleErrors(hostPage, "Host");
  const hostWrites = [];
  await installCommonRoutes(hostPage, async (route) => {
    await delay(12_000);
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(snapshot("late ambient host snapshot")) });
  }, hostWrites);
  await hostPage.goto(`${appBase}/?${query({ surface: "team", team: "teamrun-mission-current", teamMode: "host" })}`, { waitUntil: "domcontentloaded" });
  await hostPage.getByText("MemberRun controls", { exact: false }).click();
  await executeInterrupt(hostPage, "Host");
  await waitFor(() => hostWrites.length === 1, "Host RoleView write");
  assert.equal(hostWrites.length, 1, "current Host RoleView did not execute while ambient snapshot was pending");

  const agentPage = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  collectRoleErrors(agentPage, "Agent");
  const agentWrites = [];
  await installCommonRoutes(agentPage, async (route) => {
    await delay(12_000);
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(snapshot("late ambient agent snapshot")) });
  }, agentWrites);
  await agentPage.goto(`${appBase}/?${query({ surface: "team", team: "teamrun-mission-current", conversation: "agent-member-1", memberRun: "member-run-1" })}`, { waitUntil: "domcontentloaded" });
  await executeInterrupt(agentPage, "Agent");
  await waitFor(() => agentWrites.length === 1, "Agent RoleView write");
  assert.equal(agentWrites.length, 1, "current Agent RoleView did not execute while ambient snapshot was pending");

  const backoffPage = await browser.newPage({ viewport: { width: 1024, height: 800 } });
  const backoffMetrics = { requests: 0, active: 0, maxActive: 0, starts: [], writes: [] };
  await installCommonRoutes(backoffPage, async (route) => {
    backoffMetrics.requests += 1;
    backoffMetrics.active += 1;
    backoffMetrics.maxActive = Math.max(backoffMetrics.maxActive, backoffMetrics.active);
    backoffMetrics.starts.push(Date.now());
    const status = backoffMetrics.requests <= 3 ? 500 : 200;
    await route.fulfill({
      status,
      contentType: "application/json",
      body: JSON.stringify(status === 200 ? snapshot("bounded backoff recovered") : { error: "planned failure" }),
    });
    backoffMetrics.active -= 1;
  }, backoffMetrics.writes);
  await backoffPage.goto(`${appBase}/?${query({ surface: "debug" })}`, { waitUntil: "domcontentloaded" });
  await backoffPage.getByText("bounded backoff recovered", { exact: true }).waitFor({ timeout: 8_000 });
  assert.ok(backoffMetrics.requests >= 4 && backoffMetrics.requests <= 6, "failure recovery exceeded its bounded request envelope");
  assert.equal(backoffMetrics.maxActive, 1, "failure recovery overlapped full snapshots");
  assert.ok(backoffMetrics.starts[2] - backoffMetrics.starts[1] >= 900, "first retry backoff was shorter than one second");
  assert.ok(backoffMetrics.starts[3] - backoffMetrics.starts[2] >= 1_900, "second retry backoff was shorter than two seconds");
  assert.equal(unexpectedFixturePaths.length, 0, `RoleView write surfaces requested unknown fixture paths: ${unexpectedFixturePaths.join(" | ")}`);
  assert.equal(roleHttpErrors.length, 0, `RoleView write surfaces produced HTTP errors: ${roleHttpErrors.join(" | ")}`);
  assert.equal(rolePageErrors.length, 0, `RoleView write surfaces produced browser errors: ${rolePageErrors.join(" | ")}; HTTP: ${roleHttpErrors.join(" | ")}`);

  console.log("Dashboard slow snapshot browser check: PASS");
  console.log(JSON.stringify({
    snapshot_latency_ms: firstLoadLatencyMs,
    full_snapshot_request_count: fullMetrics.requests,
    max_in_flight_full_snapshots: fullMetrics.maxActive,
    coalesced_follow_up_count: fullMetrics.requests - 1,
    failure_request_count: backoffMetrics.requests,
    failure_retry_gaps_ms: [
      backoffMetrics.starts[2] - backoffMetrics.starts[1],
      backoffMetrics.starts[3] - backoffMetrics.starts[2],
    ],
    team_role_writes_during_ambient_latency: teamWrites.length,
    host_role_writes_during_ambient_latency: hostWrites.length,
    agent_role_writes_during_ambient_latency: agentWrites.length,
    console_and_page_errors: [...fullErrors, ...rolePageErrors],
  }, null, 2));
  await Promise.all([fullPage.close(), teamPage.close(), hostPage.close(), agentPage.close(), backoffPage.close()]);
} finally {
  await browser.close();
  await vite.close();
}
