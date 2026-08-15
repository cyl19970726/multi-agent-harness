#!/usr/bin/env node

import { createServer as createHttpServer } from "node:http";
import { mkdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer as createViteServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(dashboardRoot, "../..");
const fixtureRoot = join(dashboardRoot, "fixtures/workbench-layout-v2-native-v1");
const evidenceRoot = join(repoRoot, ".visual-evidence/dashboard-self-heal-v1");

async function jsonl(name) {
  return (await readFile(join(fixtureRoot, `${name}.jsonl`), "utf8"))
    .split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

const workOperations = await jsonl("work_operations");
const works = workOperations.map((operation) => operation.work);
const fixtureSnapshot = {
  generated_at: "2026-08-05T08:00:00Z",
  teams: await jsonl("teams"),
  missions: await jsonl("missions"),
  legacy_waves: await jsonl("waves"),
  team_runs: await jsonl("team_runs"),
  member_runs: await jsonl("member_runs"),
  team_messages: await jsonl("team_messages"),
  works,
  work_events: workOperations.map((operation) => operation.event),
  work_deliveries: workOperations.flatMap((operation) => operation.deliveries ?? []),
  member_actions: await jsonl("member_actions"),
  delegation_runs: await jsonl("delegation_runs"),
  team_run_events: await jsonl("team_run_events"),
  evidence: await jsonl("evidence"),
  members: [], messages: [], events: [], provider_child_threads: [],
  workflow_runs: [], workflow_steps: [], workflow_patches: [],
  workflow_artifact_manifests: [], team_supervisor_leases: [],
  team_member_close_requests: [],
};
const teamRunId = fixtureSnapshot.team_runs[0]?.id;

function scopeKey(space, company) {
  return `${space || "default"}/${company || "default"}`;
}

function buildSnapshot(space, company, title) {
  return {
    ...fixtureSnapshot,
    // Diagnostics renders this field verbatim, giving the browser test an
    // observable authoritative version without reaching into React internals.
    generated_at: title,
    works: [
      ...fixtureSnapshot.works,
      {
        ...fixtureSnapshot.works[0],
        id: `self-heal-${space}-${company}`,
        title,
        version: 99,
      },
    ],
    company_os: {
      snapshot_contract: "company-os-v1",
      projection_kind: "live_company_os",
      source: {
        kind: "harness_store",
        authoritative: true,
        project_id: company,
        store_root: `/tmp/company-os/${company}`,
        schema: "company-os/v1",
        revision: "fnv1a64:0123456789abcdef",
        projection: "latest_row_wins",
      },
      documents: [], approvals: [], financial_records: [],
      human_members: [], agent_members: [], org_units: [], business_modules: [],
      typed_records: [], relations: [], metric_observations: [],
    },
  };
}

const state = {
  titles: new Map(),
  reads: [],
  boundedReads: 0,
  responsePlan: [],
  snapshotBarriers: new Map(),
  streamBarriers: new Map(),
  streams: new Set(),
  streamSerial: 0,
  registryEmpty: false,
  switchPaths: [],
};
const titleFor = (space, company) => state.titles.get(scopeKey(space, company))
  ?? `authoritative ${space}/${company}`;

function jsonResponse(response, status, value) {
  response.writeHead(status, {
    "access-control-allow-origin": "*",
    "access-control-allow-methods": "GET,POST,OPTIONS",
    "access-control-allow-headers": "content-type",
    "content-type": "application/json",
  });
  response.end(JSON.stringify(value));
}

const api = createHttpServer((request, response) => {
  const url = new URL(request.url, "http://127.0.0.1");
  if (request.method === "OPTIONS") return jsonResponse(response, 204, {});
  if (url.pathname === "/v1/projects") return jsonResponse(response, 200, {
    current: "project-a",
    projects: [{ id: "project-a", project_root: "/tmp/project-a", kind: "repo", is_git_repo: true, is_current: true }],
  });
  if (url.pathname === "/v1/spaces") return jsonResponse(response, 200, {
    current: state.registryEmpty ? "" : "space-a",
    spaces: state.registryEmpty ? [] : ["space-a", "space-b"].map((id) => ({ id, name: id, store_root: `/tmp/${id}`, is_current: id === "space-a" })),
  });
  if (url.pathname === "/v1/companies") return jsonResponse(response, 200, {
    current: state.registryEmpty ? "" : "company-a",
    companies: state.registryEmpty ? [] : ["company-a", "company-b"].map((id) => ({ id, name: id, store_root: `/tmp/${id}`, is_current: id === "company-a" })),
  });
  if (url.pathname === "/v1/workflows") return jsonResponse(response, 200, []);
  if (url.pathname === "/v1/meta") return jsonResponse(response, 200, {
    git_rev: "unknown",
    built_at: null,
    store_root: "/tmp/dashboard-self-heal",
    latest_op_seq: 0,
    server_version: "test",
    schema_version: "agentfirm.role_views.v1",
    protocol_version: "agentfirm-member-trust/1",
    action_manifest_version: "agentfirm.role_actions.v1",
    capability_auth: "x-agentfirm-token",
    build_sha: "fbc401646f66b69a0269622c489441cfe643b54f",
  });
  if (url.pathname.startsWith("/v1/views/team-workspace/")) return jsonResponse(response, 200, {
    view_kind: "team_workspace",
    schema_version: "agentfirm.role_views.v1",
    source_execution_space_id: url.searchParams.get("space") || "space-a",
    source_store_identity: "self-heal-fixture-store",
    as_of_event_sequence: 1,
    generated_at: new Date().toISOString(),
    freshness: "current",
    data: {
      team: {team_id: fixtureSnapshot.team_runs[0]?.agent_team_id, display_name: "Self-heal Team", team_revision: 1, mission_id: fixtureSnapshot.missions[0]?.id, host_agent_id: "host-fixture", viewer_role: "member", node_id: "node-fixture", placement_generation: 1, status: "active", latest_run: null},
      works: [], members: [], messages: [], activity: [], activity_truncated: false,
      pressure_summary: {active_turns:0,ready_members:0,total_members:0,ready_work:0,review_work:0,blocked_work:0},
      reports: [], findings: [], failures: [],
      gate_requirements: [], gate_evaluations: [], gate_waivers: [], workspace_attention: [], delegation_provenance: [],
      page: {as_of_event_sequence: 1, item_count: 0, next_cursor: null},
      runtime_fabric: {agent_identities:[],agent_sessions:[],team_memberships:[],work_execution_bindings:[],messages:[],message_deliveries:[]},
    },
    attention: [], allowed_actions: [],
  });
  if (/^\/v1\/(spaces|companies|projects)\/switch$/.test(url.pathname)) {
    state.switchPaths.push(url.pathname);
    return jsonResponse(response, 200, { ok: true });
  }
  if (url.pathname === "/v1/snapshot" || /\/v1\/team-runs\/[^/]+\/snapshot$/.test(url.pathname)) {
    const space = url.searchParams.get("space") || "space-a";
    const company = url.searchParams.get("company") || "company-a";
    if (!["space-a", "space-b"].includes(space) || !["company-a", "company-b"].includes(company)) {
      state.reads.push({ path: url.pathname, space, company, status: 404 });
      return jsonResponse(response, 404, {
        error: !["space-a", "space-b"].includes(space)
          ? "execution_space_not_found"
          : "company_not_found",
      });
    }
    const captured = buildSnapshot(space, company, titleFor(space, company));
    if (url.pathname !== "/v1/snapshot") {
      captured.generated_at = `bounded:${captured.generated_at}`;
      captured.company_os = {};
    }
    const plan = state.responsePlan.shift() ?? { delay: 0, status: 200 };
    state.reads.push({ path: url.pathname, space, company, status: plan.status });
    if (url.pathname !== "/v1/snapshot") state.boundedReads += 1;
    const barrier = state.snapshotBarriers.get(scopeKey(space, company));
    barrier?.markStarted();
    const respond = () => jsonResponse(response, plan.status, plan.status === 200 ? captured : { error: "planned failure" });
    const gates = [plan.gate, barrier?.gate].filter(Boolean);
    if (gates.length > 0) {
      Promise.all(gates).then(() => { if (plan.delay) setTimeout(respond, plan.delay); else respond(); });
      return;
    }
    return setTimeout(respond, plan.delay);
  }
  if (url.pathname === "/v1/events") {
    const space = url.searchParams.get("space") || "space-a";
    const company = url.searchParams.get("company") || "company-a";
    response.writeHead(200, {
      "access-control-allow-origin": "*",
      "cache-control": "no-cache",
      connection: "keep-alive",
      "content-type": "text/event-stream",
    });
    const stream = { response, space, company, id: ++state.streamSerial };
    state.streams.add(stream);
    const openStream = () => {
      if (response.destroyed || response.writableEnded) return;
      response.write(`event: snapshot\ndata: ${JSON.stringify({
        generated_at: new Date().toISOString(),
        execution_space_id: space,
        company_scope_id: company,
        stream_epoch: `epoch-${stream.id}`,
      })}\n\n`);
    };
    const barrier = state.streamBarriers.get(scopeKey(space, company));
    if (barrier) {
      barrier.markStarted();
      barrier.gate.then(openStream);
    } else {
      openStream();
    }
    request.on("close", () => state.streams.delete(stream));
    return;
  }
  jsonResponse(response, 404, { error: "not_found" });
});
await new Promise((resolveListen) => api.listen(0, "127.0.0.1", resolveListen));
const apiBase = `http://127.0.0.1:${api.address().port}`;

function emitInvalidation({ space = "space-a", company = "company-a", revision, scope = "execution_space", malformed = false }) {
  const payload = malformed ? { bad: true } : {
    scope,
    scope_id: scope === "company" ? company : space,
    ledger: scope === "company" ? "company_os_documents.jsonl" : "work_operations.jsonl",
    revision,
    reason: "append",
    stream_epoch: "epoch-client",
  };
  for (const stream of state.streams) {
    if (stream.space === space && stream.company === company) {
      stream.response.write(`event: projection_invalidated\ndata: ${JSON.stringify(payload)}\n\n`);
    }
  }
}

function closeStreams(space, company) {
  for (const stream of [...state.streams]) {
    if (stream.space === space && stream.company === company) stream.response.end();
  }
}

function holdScope(map, space = "space-a", company = "company-a") {
  const key = scopeKey(space, company);
  if (map.has(key)) throw new Error(`scope already gated: ${key}`);
  let releaseGate;
  let acknowledgeStart;
  const gate = new Promise((resolveGate) => { releaseGate = resolveGate; });
  const started = new Promise((resolveStarted) => { acknowledgeStart = resolveStarted; });
  const barrier = {
    gate,
    started,
    startedCount: 0,
    markStarted() {
      this.startedCount += 1;
      acknowledgeStart();
    },
    release() {
      if (map.get(key) === barrier) map.delete(key);
      releaseGate();
    },
  };
  map.set(key, barrier);
  return barrier;
}
const holdSnapshots = (space, company) => holdScope(state.snapshotBarriers, space, company);
const holdStreams = (space, company) => holdScope(state.streamBarriers, space, company);

const vite = await createViteServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 0 },
  logLevel: "silent",
});
await vite.listen();
const appBase = `http://127.0.0.1:${vite.httpServer.address().port}`;
const browser = await chromium.launch({ headless: true });
let passed = 0;
let failed = 0;
function check(condition, message) {
  console.log(`  ${condition ? "PASS" : "FAIL"}  ${message}`);
  if (condition) passed += 1; else failed += 1;
}
async function waitFor(predicate, message, timeout = 8_000) {
  const started = Date.now();
  while (Date.now() - started < timeout) {
    if (await predicate()) return;
    await new Promise((resolveWait) => setTimeout(resolveWait, 25));
  }
  throw new Error(`timed out: ${message}`);
}
async function freshness(page, value) {
  try {
    await waitFor(async () => {
      const statuses = await page.locator("[data-freshness-domain]").evaluateAll((nodes) =>
        nodes.map((node) => node.getAttribute("data-freshness-status")));
      return statuses.length === 4 && (value === "stale" || value === "reconnecting"
        ? statuses.some((status) => status === value)
        : statuses.every((status) => status === value));
    }, `all freshness domains become ${value}`);
  } catch (error) {
    const observed = await page.locator("[data-dashboard-freshness]").evaluateAll((nodes) =>
      nodes.map((node) => node.getAttribute("data-dashboard-freshness")));
    const body = (await page.locator("body").innerText()).slice(0, 1_000);
    throw new Error(`${error.message}\nObserved freshness: ${observed.join(", ")}\nRecent reads: ${JSON.stringify(state.reads.slice(-5))}\nBody: ${body}`);
  }
}

try {
  await mkdir(evidenceRoot, { recursive: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, reducedMotion: "reduce" });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
  await page.addInitScript(() => {
    window.__testVisibility = "visible";
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => window.__testVisibility });
  });
  const query = new URLSearchParams({ api: apiBase, project: "project-a", space: "space-a", company: "company-a", surface: "debug" });
  await page.goto(`${appBase}/?${query}`, { waitUntil: "domcontentloaded", timeout: 15_000 });
  await freshness(page, "live");
  check(errors.length === 0, `initial App mount has no browser errors (${errors[0] ?? "none"})`);
  check(state.reads.some((read) => read.space === "space-a" && read.company === "company-a"), "initial authoritative read carries Execution Space and Company");
  await waitFor(
    () => Promise.resolve([...state.streams].some((stream) => stream.space === "space-a" && stream.company === "company-a")),
    "initial Space and Company stream connected",
  );

  // External append: the chip remains Stale until the authoritative response,
  // and the response—not the invalidation payload—introduces the new row.
  const beforeExternal = state.reads.length;
  state.titles.set(scopeKey("space-a", "company-a"), "external Works append healed");
  const g1 = holdSnapshots("space-a", "company-a");
  emitInvalidation({ revision: 1 });
  await waitFor(() => Promise.resolve(g1.startedCount > 0), "external invalidation read held by test gate");
  await freshness(page, "stale");
  check(!(await page.locator("body").innerText()).includes("external Works append healed"), "invalidation is not synthesized as row truth");
  g1.release();
  await freshness(page, "live");
  await page.getByText("external Works append healed", { exact: true }).waitFor({ timeout: 8_000 });
  check(state.reads.length === beforeExternal + 1, "one invalidation performs one authoritative read");

  // A second invalidation during the read occupies the dirty slot. The first
  // captured response cannot win; pass 2 must install the newest snapshot.
  const beforeDirty = state.reads.length;
  state.titles.set(scopeKey("space-a", "company-a"), "dirty pass one");
  const g2 = holdSnapshots("space-a", "company-a");
  emitInvalidation({ revision: 2 });
  await waitFor(() => Promise.resolve(g2.startedCount > 0), "dirty refresh read held by test gate");
  await freshness(page, "stale");
  await waitFor(() => Promise.resolve(state.reads.length === beforeDirty + 1), "first dirty read started");
  state.titles.set(scopeKey("space-a", "company-a"), "dirty follow-up newest");
  emitInvalidation({ revision: 4 }); // intentional gap
  g2.release();
  await freshness(page, "live");
  await page.getByText("dirty follow-up newest", { exact: true }).waitFor({ timeout: 8_000 });
  check(state.reads.length === beforeDirty + 2, "gap during fetch is coalesced into exactly one follow-up read");

  // Failed refresh stays Stale and retries on the bounded backoff.
  const beforeFailure = state.reads.length;
  state.titles.set(scopeKey("space-a", "company-a"), "retry recovered projection");
  state.responsePlan.push({ delay: 0, status: 500 });
  const failedReadGate = holdSnapshots("space-a", "company-a");
  emitInvalidation({ revision: 5 });
  await waitFor(() => Promise.resolve(failedReadGate.startedCount > 0), "failed refresh read held by test gate");
  await freshness(page, "stale");
  failedReadGate.release();
  await waitFor(() => Promise.resolve(state.reads.length >= beforeFailure + 1), "planned failed read");
  await new Promise((resolveWait) => setTimeout(resolveWait, 250));
  check(
    await page.locator('[data-freshness-domain="works"][data-freshness-status="stale"]').count() === 1
      && await page.locator('[data-freshness-domain="runtime"][data-freshness-status="stale"]').count() === 1,
    "failed authoritative Work read keeps Works and Runtime visibly Stale",
  );
  await freshness(page, "live");
  await page.getByText("retry recovered projection", { exact: true }).waitFor({ timeout: 8_000 });
  check(state.reads.length >= beforeFailure + 2, "failed refresh retries and recovers without manual action");

  // Visibility regain and online are edge-triggered and do not start polling.
  const beforeVisibility = state.reads.length;
  await page.evaluate(() => { window.__testVisibility = "hidden"; document.dispatchEvent(new Event("visibilitychange")); });
  await page.evaluate(() => { window.__testVisibility = "visible"; document.dispatchEvent(new Event("visibilitychange")); });
  await waitFor(() => Promise.resolve(state.reads.length === beforeVisibility + 1), "visibility recovery read");
  await new Promise((resolveWait) => setTimeout(resolveWait, 200));
  check(state.reads.length === beforeVisibility + 1, "visibility regain performs exactly one bounded read");

  const beforeOnline = state.reads.length;
  await page.evaluate(() => window.dispatchEvent(new Event("offline")));
  await freshness(page, "reconnecting");
  await page.evaluate(() => window.dispatchEvent(new Event("online")));
  await freshness(page, "live");
  check(state.reads.length === beforeOnline + 1, "online performs exactly one bounded read");

  // A quiet but healthy stream gets one probe. Successful truth restores Live;
  // silence is not treated as permanent staleness.
  const beforeQuiet = state.reads.length;
  const g5 = holdSnapshots("space-a", "company-a");
  await page.evaluate(() => {
    const original = Date.now;
    const shifted = original() + 46_000;
    Date.now = () => shifted;
    window.__restoreDateNow = () => { Date.now = original; };
  });
  await waitFor(() => Promise.resolve(g5.startedCount > 0), "quiet-stream recovery read held by test gate");
  await freshness(page, "stale");
  g5.release();
  await freshness(page, "live");
  await page.evaluate(() => window.__restoreDateNow?.());
  check(state.reads.length === beforeQuiet + 1, "quiet-open stream performs one probe, then returns Live after success");

  // Disconnect exposes Reconnecting and each controlled reconnect attempt gets
  // a bounded HTTP recovery read before the fresh SSE marker is trusted.
  const beforeReconnect = state.reads.length;
  const reconnectRead = holdSnapshots("space-a", "company-a");
  const reconnectStream = holdStreams("space-a", "company-a");
  closeStreams("space-a", "company-a");
  await waitFor(
    () => Promise.resolve(reconnectRead.startedCount > 0 && reconnectStream.startedCount > 0),
    "disconnect read and stream held by test gates",
  );
  await freshness(page, "stale");
  check(
    await page.getByText("Reconnecting", { exact: true }).count() > 0,
    "disconnect exposes Reconnecting while test-owned recovery gates are held",
  );
  await waitFor(() => Promise.resolve(state.reads.length > beforeReconnect), "disconnect fallback read");
  reconnectRead.release();
  reconnectStream.release();
  await freshness(page, "live");
  check(state.streamSerial >= 2, "EventSource reconnects with a new stream epoch");

  // An old-scope response cannot cross a Company boundary. The B snapshot wins
  // while the delayed A response and old stream become harmless.
  state.titles.set(scopeKey("space-a", "company-a"), "late old company A");
  state.titles.set(scopeKey("space-a", "company-b"), "authoritative company B");
  state.responsePlan.push({ delay: 500, status: 200 });
  emitInvalidation({ revision: 6 });
  await freshness(page, "stale");
  await page.getByLabel("Active company").selectOption("company-b");
  await freshness(page, "live");
  await page.getByText("authoritative company B", { exact: true }).waitFor({ timeout: 8_000 });
  await new Promise((resolveWait) => setTimeout(resolveWait, 650));
  check(!(await page.locator("body").innerText()).includes("late old company A"), "late Company A response cannot overwrite Company B");
  check(state.reads.some((read) => read.company === "company-b" && read.space === "space-a"), "Company switch scopes the authoritative request");
  check(!state.switchPaths.includes("/v1/companies/switch"), "Company selection never mutates the server/CLI default");

  state.titles.set(scopeKey("space-b", "company-b"), "authoritative space B");
  await page.getByLabel("Active execution space").selectOption("space-b");
  await freshness(page, "live");
  await page.getByText("authoritative space B", { exact: true }).waitFor({ timeout: 8_000 });
  check(state.reads.some((read) => read.company === "company-b" && read.space === "space-b"), "Execution Space switch scopes both snapshot and stream identity");

  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  check(!overflow, "desktop freshness control does not create horizontal overflow");
  await page.screenshot({ path: join(evidenceRoot, "desktop-live.png"), fullPage: true });
  await context.close();

  const mobile = await browser.newContext({ viewport: { width: 390, height: 844 }, reducedMotion: "reduce" });
  const mobilePage = await mobile.newPage();
  const mobileQuery = new URLSearchParams({ api: apiBase, project: "project-a", space: "space-b", company: "company-b", surface: "debug" });
  await mobilePage.goto(`${appBase}/?${mobileQuery}`, { waitUntil: "domcontentloaded", timeout: 15_000 });
  await freshness(mobilePage, "live");
  const mobileOverflow = await mobilePage.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  check(!mobileOverflow, "mobile freshness control remains visible without horizontal overflow");
  await mobilePage.screenshot({ path: join(evidenceRoot, "mobile-live.png"), fullPage: true });
  await mobile.close();

  // A stale team query on a non-Team route must not choose the bounded endpoint.
  const routeContext = await browser.newContext({ viewport: { width: 1024, height: 800 }, reducedMotion: "reduce" });
  const routePage = await routeContext.newPage();
  const fullBefore = state.reads.filter((read) => read.path === "/v1/snapshot").length;
  const boundedBefore = state.boundedReads;
  const staleRoute = new URLSearchParams({ api: apiBase, project: "project-a", space: "space-a", company: "company-a", surface: "work", team: teamRunId });
  await routePage.goto(`${appBase}/?${staleRoute}`, { waitUntil: "domcontentloaded", timeout: 15_000 });
  await freshness(routePage, "live");
  check(state.reads.filter((read) => read.path === "/v1/snapshot").length > fullBefore, "non-Team stale deep link uses the full snapshot endpoint");
  check(state.boundedReads === boundedBefore, "non-Team stale deep link never uses TeamRun bounded snapshot");
  await routeContext.close();

  // Bounded TeamRun and full Dashboard reads share an SSE scope but not a
  // causal projection. A delayed bounded response after Team→Work must be
  // discarded before it can empty Company OS or launch an old-closure pass 2.
  const boundaryContext = await browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" });
  const boundaryPage = await boundaryContext.newPage();
  boundaryPage.on("pageerror", (error) => console.error(`boundary pageerror: ${error.message}`));
  boundaryPage.on("console", (message) => { if (message.type() === "error") console.error(`boundary console: ${message.text()}`); });
  state.titles.set(scopeKey("space-a", "company-a"), "full projection after team exit");
  const teamRoute = new URLSearchParams({ api: apiBase, project: "project-a", space: "space-a", company: "company-a", surface: "team", team: teamRunId });
  await boundaryPage.goto(`${appBase}/?${teamRoute}`, { waitUntil: "domcontentloaded", timeout: 15_000 });
  await boundaryPage.locator('[data-testid="authenticated-team-workspace"]').waitFor({ timeout: 8_000 });
  await waitFor(
    () => Promise.resolve([...state.streams].some((stream) => stream.space === "space-a" && stream.company === "company-a")),
    "Team stream connected",
  );
  const boundedRaceBefore = state.boundedReads;
  state.responsePlan.push({ delay: 500, status: 200 });
  emitInvalidation({ revision: 51 });
  await waitFor(() => Promise.resolve(state.boundedReads === boundedRaceBefore + 1), "bounded race read started");
  await boundaryPage.evaluate(() => {
    const url = new URL(window.location.href);
    url.searchParams.set("surface", "work");
    history.pushState({}, "", url);
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  await freshness(boundaryPage, "live");
  await boundaryPage.getByText("Company Work", { exact: true }).waitFor({ timeout: 8_000 });
  await boundaryPage.evaluate(() => {
    const url = new URL(window.location.href);
    url.searchParams.set("surface", "debug");
    history.pushState({}, "", url);
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  await boundaryPage.getByText("full projection after team exit", { exact: true }).waitFor({ timeout: 8_000 });
  await new Promise((resolveWait) => setTimeout(resolveWait, 600));
  check(!(await boundaryPage.locator("body").innerText()).includes("bounded:full projection after team exit"), "late bounded Team response cannot overwrite the full projection");
  check(state.reads.at(-1)?.path === "/v1/snapshot", "Team→Work projection handoff finishes on the full endpoint");
  await boundaryContext.close();

  // Stale persisted selectors fail closed on their explicit 404. Registries are
  // available before source=live, so the client atomically adopts both current
  // truth boundaries and performs a new explicitly-scoped read.
  const staleContext = await browser.newContext({ viewport: { width: 1200, height: 800 }, reducedMotion: "reduce" });
  const stalePage = await staleContext.newPage();
  const staleSelectors = new URLSearchParams({ api: apiBase, project: "project-a", space: "missing-space", company: "missing-company", surface: "debug" });
  await stalePage.goto(`${appBase}/?${staleSelectors}`, { waitUntil: "domcontentloaded", timeout: 15_000 });
  await freshness(stalePage, "live");
  await stalePage.getByText("full projection after team exit", { exact: true }).waitFor({ timeout: 8_000 });
  const recoveredUrl = new URL(stalePage.url());
  check(recoveredUrl.searchParams.get("space") === "space-a" && recoveredUrl.searchParams.get("company") === "company-a", "stale Space and Company selectors reconcile atomically to registry current");
  check(state.reads.some((read) => read.space === "missing-space" && read.company === "missing-company" && read.status === 404), "stale selector read fails closed instead of serving default truth");
  check(!state.reads.some((read) => read.space === "missing-space" && read.company === "missing-company" && read.status === 200), "no wrong-store data is labelled with stale selectors");
  await stalePage.getByText(/Execution Space "missing-space" was not found.*Company "missing-company" was not found/).waitFor({ timeout: 8_000 });
  check(true, "selector recovery is exposed to the operator");
  await staleContext.close();

  state.registryEmpty = true;
  const emptyContext = await browser.newContext({ viewport: { width: 1000, height: 760 }, reducedMotion: "reduce" });
  const emptyPage = await emptyContext.newPage();
  await emptyPage.goto(`${appBase}/?${staleSelectors}`, { waitUntil: "domcontentloaded", timeout: 15_000 });
  await freshness(emptyPage, "live");
  const clearedUrl = new URL(emptyPage.url());
  const persisted = await emptyPage.evaluate(() => ({
    space: localStorage.getItem("harness.selectedSpaceId"),
    company: localStorage.getItem("harness.selectedCompanyId"),
  }));
  check(!clearedUrl.searchParams.has("space") && !clearedUrl.searchParams.has("company"), "empty registries clear stale URL selectors before unscoped compatibility read");
  check(persisted.space === null && persisted.company === null, "Company remains tab-local and empty registries remove Space persistence");
  await emptyPage.getByText(/cleared the stale selection/).first().waitFor({ timeout: 8_000 });
  check(true, "empty-registry selector clearing remains operator-visible");
  await emptyContext.close();
  state.registryEmpty = false;

  console.log(`\nDashboard self-heal browser checks: ${passed} pass, ${failed} fail`);
  console.log(`Visual evidence: ${evidenceRoot}`);
} finally {
  for (const stream of state.streams) stream.response.end();
  await browser.close();
  await vite.close();
  await new Promise((resolveClose) => api.close(resolveClose));
}

process.exit(failed === 0 ? 0 : 1);
