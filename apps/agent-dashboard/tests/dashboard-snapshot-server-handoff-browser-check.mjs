#!/usr/bin/env node

/**
 * DEV-33 server-side snapshot handoff acceptance.
 *
 * Unlike the response-delay fixture, this check runs the real `firm serve`
 * process with a pause inside the synchronous full-snapshot Store build. It
 * changes Project and Execution Space while prior builds are active and
 * reads the serve-process test metrics to prove Store computation itself
 * never overlaps.
 *
 * DOC-108 retired `harness company init|switch` and `harness company docs
 * page create`; the durable rows that give the paused Store build real
 * content to process are seeded on the surviving Project + Execution Space
 * axes instead (a mission-less AgentTeam, a TeamRun, and one Team-run Work).
 * The `company` URL param is kept as the inert free-text scope label the
 * frontend still reads at bootstrap — the Runtime itself no longer
 * differentiates any response by it.
 */
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer as createViteServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(dashboardRoot, "../..");
const harness = join(repoRoot, "target", "debug", "firm");

function runHarness(args, env, cwd) {
  const result = spawnSync(harness, args, { cwd, env, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(`harness ${args.join(" ")} failed: ${result.stderr || result.stdout}`);
  }
  return result.stdout.trim();
}

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

async function waitFor(predicate, message, timeout = 30_000) {
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

async function requestMetrics(apiBase) {
  const response = await fetch(`${apiBase}/v1/test/dashboard-snapshot-builds`);
  if (!response.ok) throw new Error(`snapshot metrics HTTP ${response.status}`);
  return await response.json();
}

const build = spawnSync("cargo", ["build", "-q", "-p", "firm-cli"], {
  cwd: repoRoot,
  stdio: "inherit",
});
if (build.status !== 0) process.exit(build.status ?? 1);

const temporaryRoot = await mkdtemp(join(tmpdir(), "dashboard-snapshot-handoff-"));
const harnessHome = join(temporaryRoot, "harness-home");
const projectRootA = join(temporaryRoot, "project-a");
const projectRootB = join(temporaryRoot, "project-b");
await Promise.all([
  mkdir(harnessHome, { recursive: true }),
  mkdir(projectRootA, { recursive: true }),
  mkdir(projectRootB, { recursive: true }),
]);

const env = {
  ...process.env,
  FIRM_HOME: harnessHome,
  HARNESS_HOME: harnessHome,
  FIRM_TEST_DASHBOARD_SNAPSHOT_BUILD_PAUSE_MS: "800",
  // RoleView reads (the default Global Work surface included) require a
  // member-trust credential; register one operator identity for this run.
  AGENTFIRM_HTTP_CREDENTIALS_JSON: JSON.stringify([
    { token: "dev33-handoff-operator", actor: { kind: "human", id: "operator:dev33-handoff" }, authority_actors: [] },
  ]),
};
for (const name of [
  "FIRM_ROOT", "FIRM_PROJECT", "FIRM_PROJECT_ID", "FIRM_SPACE", "FIRM_COMPANY",
  "HARNESS_ROOT", "HARNESS_PROJECT", "HARNESS_PROJECT_ID", "HARNESS_SPACE", "HARNESS_COMPANY",
]) delete env[name];

runHarness(["init"], env, projectRootA);
const projectA = JSON.parse(runHarness(["project", "current"], env, projectRootA));
const spaceA = JSON.parse(runHarness(["space", "current"], env, projectRootA));
const projectB = JSON.parse(runHarness(["project", "add", projectRootB], env, projectRootA));
const spaceB = JSON.parse(runHarness([
  "space", "init", "--id", "dev33-scope-space-b", "--name", "DEV-33 scope space B",
  "--project-binding", projectB.id,
], env, projectRootA));
runHarness(["space", "switch", spaceA.id], env, projectRootA);

// Company writers are retired (`harness company ...` fails closed with a
// DOC-108 retirement error on every subcommand); seed cheap durable rows on
// the surviving Project + Execution Space axes instead, so the paused Store
// snapshot build has real content, not an empty projection.
const dev33Node = JSON.parse(runHarness(["node", "init", "--display-name", "dev33-handoff-node"], env, projectRootA));
runHarness([
  "node", "project", "register",
  "--node-id", dev33Node.id,
  "--execution-space-id", spaceA.id,
  "--project-binding-id", projectA.id,
], env, projectRootA);
for (const [id, role] of [["dev33-host", "host"], ["dev33-worker", "worker"]]) {
  runHarness([
    "member-trust", "mutate",
    "--actor-kind", "human",
    "--actor-id", "dev33-handoff-owner",
    "--idempotency-key", `dev33-handoff-create-${id}`,
    "--expected-version", "0",
    "--json", JSON.stringify({
      command: "create_agent_member",
      member: {
        id, name: `DEV-33 handoff ${role}`,
        description: "Durable identity seeded for the snapshot handoff acceptance.",
        role, capabilities: [], skill_refs: [],
        provider_profile_ref: "codex-default", model_preference: null,
        workspace_policy: "managed-worktree", permission_ceiling: "full_access",
        organization_status: "active", version: 1,
        created_by: { kind: "human", id: "dev33-handoff-owner" },
        created_at: "2026-08-05T12:00:00+08:00", updated_at: "2026-08-05T12:00:00+08:00",
      },
    }),
  ], env, projectRootA);
}
const dev33Team = JSON.parse(runHarness([
  "team", "create",
  "--name", "DEV-33 Handoff Team",
  "--description", "Flat mission-less Team seeded for the snapshot handoff acceptance.",
  "--host-agent-id", "dev33-host",
  "--node-id", dev33Node.id,
  "--member", "dev33-worker",
], env, projectRootA));
const dev33TeamRunPayload = JSON.parse(runHarness([
  "team-run", "create",
  "--objective", "Exercise the snapshot handoff Store build under scope pressure",
  "--agent-team-id", dev33Team.id,
  "--host-runtime-mode", "external_interactive",
  "--member", "dev33-host:host:codex/external_interactive",
  "--member", "dev33-worker:worker:codex/app-server",
  "--json",
], env, projectRootA));
const dev33TeamRunId = dev33TeamRunPayload.team_run?.id ?? dev33TeamRunPayload.id ?? dev33TeamRunPayload.result?.id;
if (!dev33TeamRunId) throw new Error(`team-run create did not return an id: ${JSON.stringify(dev33TeamRunPayload)}`);
runHarness([
  "team-run", "work", "create",
  "--team-run-id", dev33TeamRunId,
  "--work-id", "dev33-handoff-work",
  "--title", "DEV-33 scope handoff Work",
  "--context", "Seed content so the paused Store snapshot build is non-trivial.",
  "--completion-criteria", "Visible in the Global Work aggregate.",
  "--priority", "normal", "--claim-mode", "team_claim",
  "--eligible-member-id", "dev33-worker",
], env, projectRootA);

const apiPort = await freePort();
const apiBase = `http://127.0.0.1:${apiPort}`;
let runtime = spawn(harness, ["serve", "--addr", `127.0.0.1:${apiPort}`, "--no-truncate"], {
  cwd: projectRootA,
  env,
  stdio: ["ignore", "pipe", "pipe"],
});
const runtimeLogs = [];
runtime.stdout.on("data", (chunk) => runtimeLogs.push(chunk.toString()));
runtime.stderr.on("data", (chunk) => runtimeLogs.push(chunk.toString()));
process.once("exit", () => {
  if (runtime && runtime.exitCode === null && runtime.signalCode === null) runtime.kill("SIGTERM");
});

let vite;
let browser;
let page;
try {
  await waitFor(async () => (await fetch(`${apiBase}/health`).catch(() => null))?.ok, "real Runtime health");
  vite = await createViteServer({
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
  browser = await chromium.launch({ headless: true });
  page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  await page.addInitScript(() => {
    window.__AGENTFIRM_BOOTSTRAP__ = { capabilityToken: "dev33-handoff-operator" };
  });

  const consoleErrors = [];
  const pageErrors = [];
  const httpErrors = [];
  const snapshotRequests = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("response", (response) => {
    if (response.status() >= 400) httpErrors.push(`${response.status()} ${response.url()}`);
  });
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.pathname === "/v1/snapshot") {
      snapshotRequests.push({
        at: Date.now(),
        space: url.searchParams.get("space"),
        project: url.searchParams.get("project"),
        company: url.searchParams.get("company"),
      });
    }
  });

  const query = new URLSearchParams({
    api: appBase,
    project: projectA.id,
    space: spaceA.id,
    company: "dev33-company-a",
    surface: "work",
  });
  await page.goto(`${appBase}/?${query}`, { waitUntil: "domcontentloaded", timeout: 20_000 });
  await Promise.all([
    page.getByLabel("Active project").locator(`option[value="${projectB.id}"]`).waitFor({ state: "attached" }),
    page.getByLabel("Active execution space").locator(`option[value="${spaceB.id}"]`).waitFor({ state: "attached" }),
  ]);
  await waitFor(async () => (await requestMetrics(apiBase)).active === 1, "first backend Store snapshot build to be active");

  const changeScope = async (label, value) => {
    const before = snapshotRequests.length;
    await page.getByLabel(label).selectOption(value);
    await waitFor(() => snapshotRequests.length > before, `${label} ${value} snapshot request`);
  };


  // Each change happens while the previous synchronous Store build is still
  // active or queued. This is true backend pressure, not a held route response.
  // Company no longer has an in-place switcher: the URL-owned param is read
  // at bootstrap. Scope pressure is exercised across the retained Project and
  // Execution Space dimensions; every request still carries the fixed Company
  // scope, and the settled proof below includes it.
  await changeScope("Active project", projectB.id);
  await changeScope("Active execution space", spaceB.id);
  await changeScope("Active project", projectA.id);
  await changeScope("Active execution space", spaceA.id);
  await changeScope("Active project", projectB.id);
  await changeScope("Active execution space", spaceB.id);

  // The settled scope is proven by the snapshot request stream itself: every
  // read after the final switches must carry project B + space B + company B.
  await page.getByRole("heading", { name: "Global Work", exact: true }).waitFor({ timeout: 45_000 });
  const lastRead = snapshotRequests.at(-1);
  assert.equal(lastRead?.project, projectB.id, "final settled snapshot read carries project B");
  assert.equal(lastRead?.space, spaceB.id, "final settled snapshot read carries space B");
  assert.equal(lastRead?.company, "dev33-company-a", "final settled snapshot read carries the URL-owned Company scope");
  await waitFor(async () => {
    const metrics = await requestMetrics(apiBase);
    return metrics.active === 0 && metrics.completed === metrics.started;
  }, "all backend Store snapshot builds to settle", 45_000);
  const metrics = await requestMetrics(apiBase);

  assert.equal(metrics.max_active, 1, "true scope changes overlapped backend Store snapshot builds");
  assert.ok(metrics.started >= 4, "repeated true scope changes did not reach the real backend builder");
  assert.equal(metrics.completed, metrics.started, "an abandoned backend Store snapshot build did not settle");
  assert.equal(consoleErrors.length, 0, `browser console errors: ${consoleErrors.join(" | ")}`);
  assert.equal(pageErrors.length, 0, `browser page errors: ${pageErrors.join(" | ")}`);
  assert.equal(httpErrors.length, 0, `browser HTTP errors: ${httpErrors.join(" | ")}`);

  console.log("Dashboard server snapshot handoff browser check: PASS");
  console.log(JSON.stringify({
    backend_pause_ms: 800,
    browser_snapshot_request_count: snapshotRequests.length,
    backend_snapshot_build_started: metrics.started,
    backend_snapshot_build_completed: metrics.completed,
    max_active_backend_snapshot_builds: metrics.max_active,
    final_scope: { project: projectB.id, company: "dev33-company-a", space: spaceB.id },
    console_errors: consoleErrors,
    page_errors: pageErrors,
    http_errors: httpErrors,
  }, null, 2));
} catch (error) {
  console.error(runtimeLogs.slice(-30).join(""));
  throw error;
} finally {
  if (page) await page.close().catch(() => {});
  if (browser) await browser.close().catch(() => {});
  if (vite) await vite.close().catch(() => {});
  if (runtime && runtime.exitCode === null && runtime.signalCode === null) {
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
  await rm(temporaryRoot, { recursive: true, force: true });
}
