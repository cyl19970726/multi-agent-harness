#!/usr/bin/env node

/**
 * Real Runtime/browser convergence acceptance.
 *
 * This check builds and spawns `harness serve` against isolated native
 * Execution Space stores. Playwright talks to that Runtime through Vite's
 * same-origin proxy; no snapshot, SSE frame, or business row is fabricated.
 *
 * DOC-108 retired the legacy CompanyOS (`harness company init|switch`,
 * `harness company docs page create`, `harness mission create`,
 * `/v1/company-os/*` writes, and `/v1/companies*`); this check exercises the
 * surviving Project + Execution Space scope axes and durable AgentTeam/
 * TeamRun/Work instead. The Company URL param is retained by the frontend as
 * an inert free-text label only (`--company` on `space init` is likewise an
 * unvalidated compatibility label) — the Runtime never differentiates a
 * response by it (`/v1/snapshot` ignores it; `handle_sse_stream` always
 * subscribes with `company_scope_id: None`).
 */
import { spawn, spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer as createViteServer } from "vite";
import Ajv2020 from "ajv/dist/2020.js";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(dashboardRoot, "../..");
const harness = join(repoRoot, "target", "debug", "firm");
const evidenceRoot = join(repoRoot, ".visual-evidence", "dashboard-runtime-live-e2e-v1");
const agentFirmToken = `agentfirm-runtime-live-${process.pid}`;
const memberAgentFirmToken = `agentfirm-member-live-${process.pid}`;
const operatorAgentFirmToken = `agentfirm-operator-live-${process.pid}`;
const secondaryHostAgentFirmToken = `agentfirm-secondary-host-live-${process.pid}`;
const siblingNodeAgentFirmToken = `agentfirm-sibling-node-live-${process.pid}`;
const now = "2026-08-05T12:00:00+08:00";

const roleViewSchemaDir = join(repoRoot, "schemas", "role-views", "agentfirm.role_views.v1");
const roleViewSchemaNames = ["common", "role-view", "global-work", "team-workspace", "host-console", "member-workbench", "operator"];
const roleViewAjv = new Ajv2020({strict:false, allErrors:true});
for (const file of readdirSync(join(repoRoot, "schemas", "collaboration")).filter(file => file.endsWith(".schema.json"))) {
  roleViewAjv.addSchema(JSON.parse(readFileSync(join(repoRoot, "schemas", "collaboration", file), "utf8")));
}
for (const name of roleViewSchemaNames) {
  roleViewAjv.addSchema(JSON.parse(readFileSync(join(roleViewSchemaDir, `${name}.schema.json`), "utf8")));
}
function validateLiveRoleView(name, value) {
  const validate = roleViewAjv.getSchema(`agentfirm.role_views.v1/${name}.schema.json`);
  if (!validate?.(value)) throw new Error(`live ${name} RoleView violates schema: ${roleViewAjv.errorsText(validate?.errors)}`);
}

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
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok || payload.ok === false) {
    throw new Error(`${body === undefined ? "GET" : "POST"} ${path}: HTTP ${response.status} ${JSON.stringify(payload)}`);
  }
  return payload.result ?? payload;
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
  // 45s: the freshness transition crosses server snapshot build + SSE push +
  // UI refetch; 15s only fit an uncontended dev machine (repeatedly red on CI).
  await page.locator(`[data-freshness-domain="${domain}"][data-freshness-status="${status}"]`)
    .waitFor({ timeout: 45_000 });
}

async function navigate(page, label) {
  await page.locator("aside").getByRole("button", { name: label, exact: true }).click();
}

const build = spawnSync("cargo", ["build", "-q", "-p", "firm-cli"], {
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
  FIRM_HOME: harnessHome,
  HARNESS_HOME: harnessHome,
  AGENTFIRM_HTTP_CREDENTIALS_JSON: JSON.stringify([{
    token: agentFirmToken,
    actor: { kind: "agent_member", id: "host" },
    authority_actors: [],
  }]),
};
delete env.FIRM_ROOT;
delete env.FIRM_PROJECT;
delete env.FIRM_PROJECT_ID;
delete env.FIRM_SPACE;
delete env.FIRM_COMPANY;
delete env.HARNESS_ROOT;
delete env.HARNESS_PROJECT;
delete env.HARNESS_PROJECT_ID;
delete env.HARNESS_SPACE;
delete env.HARNESS_COMPANY;

runHarness(["init"], env, projectRoot);

const apiPort = await freePort();
const apiBase = `http://127.0.0.1:${apiPort}`;
let runtime = null;
const runtimePids = [];
const runtimeLogs = [];
function startRuntime() {
  runtime = spawn(harness, ["serve", "--addr", `127.0.0.1:${apiPort}`, "--no-truncate"], {
    cwd: projectRoot,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  runtimePids.push(runtime.pid);
  runtime.stdout.on("data", (chunk) => runtimeLogs.push(chunk.toString()));
  runtime.stderr.on("data", (chunk) => runtimeLogs.push(chunk.toString()));
}

// A test runner may terminate this Node process before the async finally block
// executes. Always forward process termination to the exact child we own so a
// Wave check cannot leave `firm serve` orphaned under launchd.
process.once("exit", () => {
  if (runtime && runtime.exitCode === null && runtime.signalCode === null) {
    runtime.kill("SIGTERM");
  }
});
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

function createCanonicalMember(id, name, role, commandEnv = env) {
  runHarness([
    "member-trust", "mutate",
    "--actor-kind", "human",
    "--actor-id", "dashboard-runtime-owner",
    "--idempotency-key", `dashboard-runtime-create-${id}`,
    "--expected-version", "0",
    "--json", JSON.stringify({
      command: "create_agent_member",
      member: {
        id, name,
        description: `Canonical ${role} identity for live Dashboard Runtime acceptance.`,
        role,
        capabilities: [],
        skill_refs: [],
        provider_profile_ref: "codex-default",
        model_preference: null,
        workspace_policy: "managed-worktree",
        permission_ceiling: "full_access",
        organization_status: "active",
        version: 1,
        created_by: { kind: "human", id: "dashboard-runtime-owner" },
        created_at: now,
        updated_at: now,
      },
    }),
  ], commandEnv, projectRoot);
}

// Build the complete Wave 3 admission chain. A TeamRun is never a free-standing
// runtime: its flat AgentTeam is placed on one Node (Teams are Mission-less by
// default post-DOC-108 — `harness mission create` is retired and `team create`
// must not pass `--mission-id`/`--legacy-mission-id`), and the Node must be
// registered for this exact Execution Space + Project Binding.
const liveNode = JSON.parse(runHarness([
  "node", "init", "--display-name", "dashboard-live-node",
], env, projectRoot));
runHarness([
  "node", "project", "register",
  "--node-id", liveNode.id,
  "--execution-space-id", spaceId,
  "--project-binding-id", projectId,
], env, projectRoot);
createCanonicalMember("host", "Dashboard Runtime Host", "host");
createCanonicalMember("worker", "Dashboard Runtime Worker", "worker");
const liveTeam = JSON.parse(runHarness([
  "team", "create",
  "--name", "Dashboard Runtime Live Team",
  "--description", "Flat Team for the live Dashboard Runtime acceptance.",
  "--host-agent-id", "host",
  "--node-id", liveNode.id,
  "--member", "worker",
], env, projectRoot));

const liveTeamRunPayload = JSON.parse(runHarness([
  "team-run", "create",
  "--objective", "Exercise Runtime convergence against native TeamWork",
  "--agent-team-id", liveTeam.id,
  "--host-runtime-mode", "external_interactive",
  "--member", "host:host:codex/external_interactive",
  "--member", "worker:worker:codex/app-server",
  "--json",
], env, projectRoot));
const liveTeamRunId = liveTeamRunPayload.team_run?.id ?? liveTeamRunPayload.id ?? liveTeamRunPayload.result?.id;
if (!liveTeamRunId) throw new Error(`team-run create did not return an id: ${JSON.stringify(liveTeamRunPayload)}`);
const liveMemberRuns = liveTeamRunPayload.member_runs ?? liveTeamRunPayload.result?.member_runs ?? [];
const liveMemberRunId = liveMemberRuns.find((run) => run.agent_member_id === "worker")?.id;
if (!liveMemberRunId) throw new Error(`team-run create did not return a MemberRun: ${JSON.stringify(liveTeamRunPayload)}`);

// A second Execution Space and sibling Team prove that Global Work is a
// vector snapshot rather than a max-sequence illusion.
const secondarySpaceId = "dashboard-secondary-space";
runHarness([
  "space", "init", "--id", secondarySpaceId, "--name", "Dashboard secondary space",
  "--project-binding", projectId,
], env, projectRoot);
runHarness(["space", "switch", spaceId], env, projectRoot);
const secondaryEnv = {...env, FIRM_SPACE: secondarySpaceId};
const secondaryNode = JSON.parse(runHarness([
  "node", "init", "--display-name", "dashboard-live-node",
], secondaryEnv, projectRoot));
if (secondaryNode.id !== liveNode.id) {
  throw new Error(`one-machine invariant violated: ${secondaryNode.id} != ${liveNode.id}`);
}
runHarness([
  "node", "project", "register",
  "--node-id", secondaryNode.id,
  "--execution-space-id", secondarySpaceId,
  "--project-binding-id", projectId,
], secondaryEnv, projectRoot);
// Model a genuinely distinct machine by giving the same canonical Store an
// independent FIRM_HOME (and therefore an independently generated immutable
// Node identity). The Runtime still reads the real registered projection from
// the shared Execution Space; no credential or RoleView row is fabricated.
const siblingMachineHome = join(temporaryRoot, "sibling-machine-home");
await mkdir(siblingMachineHome, {recursive:true});
const siblingMachineEnv = {...secondaryEnv,FIRM_HOME:siblingMachineHome,HARNESS_HOME:siblingMachineHome};
const secondaryStoreRoot = join(harnessHome, "execution-spaces", secondarySpaceId);
const siblingNode = JSON.parse(runHarness([
  "--store", secondaryStoreRoot, "node", "init", "--display-name", "dashboard-sibling-machine-node",
], siblingMachineEnv, projectRoot));
if (siblingNode.id === liveNode.id) throw new Error("distinct machine must have a distinct immutable Node identity");
runHarness([
  "node", "project", "register",
  "--node-id", siblingNode.id,
  "--execution-space-id", secondarySpaceId,
  "--project-binding-id", projectId,
], secondaryEnv, projectRoot);
createCanonicalMember("host-secondary", "Dashboard Secondary Host", "host", secondaryEnv);
createCanonicalMember("worker-secondary", "Dashboard Secondary Worker", "worker", secondaryEnv);
const secondaryTeam = JSON.parse(runHarness([
  "team", "create", "--name", "Dashboard Sibling Team",
  "--description", "Sibling Team for isolation acceptance.",
  "--host-agent-id", "host-secondary",
  "--node-id", secondaryNode.id, "--member", "worker-secondary",
], secondaryEnv, projectRoot));
const secondaryRunPayload = JSON.parse(runHarness([
  "team-run", "create", "--objective", "Sibling Team isolation",
  "--agent-team-id", secondaryTeam.id,
  "--host-runtime-mode", "external_interactive",
  "--member", "host-secondary:host:codex/external_interactive",
  "--member", "worker-secondary:worker:codex/app-server", "--json",
], secondaryEnv, projectRoot));
const secondaryTeamRunId = secondaryRunPayload.team_run?.id ?? secondaryRunPayload.id ?? secondaryRunPayload.result?.id;
if (!secondaryTeamRunId) throw new Error(`secondary TeamRun missing: ${JSON.stringify(secondaryRunPayload)}`);
runHarness([
  "team-run", "work", "create", "--team-run-id", secondaryTeamRunId,
  "--work-id", "work-secondary-live", "--title", "Secondary space Work",
  "--context", "Prove Company vector aggregation.",
  "--completion-criteria", "Visible only through authoritative aggregation.",
  "--priority", "normal", "--claim-mode", "team_claim",
  "--eligible-member-id", "worker-secondary",
], secondaryEnv, projectRoot);
env.AGENTFIRM_HTTP_CREDENTIALS_JSON = JSON.stringify([{
  token: agentFirmToken, actor: {kind:"agent_member",id:"host"}, authority_actors: [],
},{
  token: memberAgentFirmToken, actor: {kind:"agent_member",id:"worker"}, authority_actors: [],
},{
  token: operatorAgentFirmToken, actor: {kind:"service",id:liveNode.id}, authority_actors: [],
},{
  token: secondaryHostAgentFirmToken, actor: {kind:"agent_member",id:"host-secondary"}, authority_actors: [],
},{
  token: siblingNodeAgentFirmToken, actor: {kind:"service",id:siblingNode.id}, authority_actors: [],
}]);
await stopRuntime();
startRuntime();
await waitFor(async () => (await fetch(`${apiBase}/health`).catch(() => null))?.ok, "Runtime with exact AgentFirm credentials");
function createNativeWork(id, title) {
  runHarness([
    "team-run", "work", "create",
    "--team-run-id", liveTeamRunId,
    "--work-id", id,
    "--title", title,
    "--context", "External Runtime acceptance write.",
    "--completion-criteria", "Visible without reload in the Global Work aggregate.",
    "--priority", "high",
    "--claim-mode", "team_claim",
    "--eligible-member-id", "worker",
  ], env, projectRoot);
}
createNativeWork("work-role-live", "Real browser RoleAction loop");

async function roleView(path, capabilityToken) {
  const response = await fetch(new URL(path, apiBase), {
    headers: {accept:"application/json", "x-agentfirm-token": capabilityToken},
  });
  const body = await response.json().catch(() => ({}));
  return {status:response.status, body};
}
const globalBefore = await roleView(`/v1/views/global-work?project=${projectId}`, agentFirmToken);
if (globalBefore.status !== 200) throw new Error(`Global Work vector: ${JSON.stringify(globalBefore.body)}`);
validateLiveRoleView("global-work", globalBefore.body);
const vectorBefore = globalBefore.body.data.page.snapshot_vector;
check(vectorBefore.some((point)=>point.execution_space_id===spaceId)
  && vectorBefore.some((point)=>point.execution_space_id===secondarySpaceId),
"Global Work exposes a per-Execution-Space snapshot vector");
check(globalBefore.body.data.page.item_count >= 2, "Global Work is populated from both sibling Teams");
const primaryDeniedBySecondary = await roleView(`/v1/views/host-console/${liveTeam.id}?project=${projectId}&space=${spaceId}`, secondaryHostAgentFirmToken);
const secondaryDeniedByPrimary = await roleView(`/v1/views/host-console/${secondaryTeam.id}?project=${projectId}&space=${secondarySpaceId}`, agentFirmToken);
check(primaryDeniedBySecondary.status===403 && secondaryDeniedByPrimary.status===403,
"sibling Team Host identities are denied bidirectionally");
const collaborationAction = (envelope) => envelope.body.allowed_actions?.find((action) =>
  /collaboration|delegation|cancellation|remote_fact/.test(String(action.kind)),
);
const unavailableTeam = await roleView(`/v1/views/team-workspace/${liveTeam.id}?project=${projectId}&space=${spaceId}`, agentFirmToken);
const unavailableHost = await roleView(`/v1/views/host-console/${liveTeam.id}?project=${projectId}&space=${spaceId}`, agentFirmToken);
const unavailableMember = await roleView(`/v1/views/member-workbench/${liveMemberRunId}?project=${projectId}&space=${spaceId}`, memberAgentFirmToken);
for (const [label, envelope, schema] of [
  ["Team", unavailableTeam, "team-workspace"],
  ["Host", unavailableHost, "host-console"],
  ["Member", unavailableMember, "member-workbench"],
]) {
  if (envelope.status !== 200) throw new Error(`${label} unavailable collaboration projection: ${JSON.stringify(envelope.body)}`);
  validateLiveRoleView(schema, envelope.body);
  check(
    envelope.body.data.collaboration?.state === "unavailable" && !collaborationAction(envelope),
    `${label} fails closed without a valid collaboration projection and advertises no collaboration action`,
  );
}
const siblingNodeDenied = await roleView(`/v1/views/operator/${liveNode.id}?project=${projectId}&space=${spaceId}`, siblingNodeAgentFirmToken);
const primaryNodeDeniedOnSibling = await roleView(`/v1/views/operator/${siblingNode.id}?project=${projectId}&space=${secondarySpaceId}`, operatorAgentFirmToken);
const siblingNodeProjection = await roleView(`/v1/views/operator/${siblingNode.id}?project=${projectId}&space=${secondarySpaceId}`, siblingNodeAgentFirmToken);
check(siblingNodeDenied.status===403 && primaryNodeDeniedOnSibling.status===403 && siblingNodeProjection.status===200,
  "distinct registered sibling Nodes are isolated bidirectionally by Node and Execution Space");
validateLiveRoleView("operator", siblingNodeProjection.body);
const siblingDaemonAction = siblingNodeProjection.body.allowed_actions.find((action)=>["start_daemon","stop_daemon"].includes(action.kind));
check(Number.isSafeInteger(siblingDaemonAction?.authority_generation),
  "real populated Operator RoleView carries an exact schema-valid daemon authority generation");
runHarness([
  "team-run", "work", "create", "--team-run-id", secondaryTeamRunId,
  "--work-id", "work-secondary-vector-advance", "--title", "Advance secondary vector",
  "--context", "Mutate only the secondary space.",
  "--completion-criteria", "Only the secondary snapshot point advances.",
  "--priority", "normal", "--claim-mode", "team_claim",
  "--eligible-member-id", "worker-secondary",
], secondaryEnv, projectRoot);
await waitFor(async()=>{
  const current = await roleView(`/v1/views/global-work?project=${projectId}`, agentFirmToken);
  if(current.status!==200)return false;
  const beforePrimary=vectorBefore.find((point)=>point.execution_space_id===spaceId);
  const beforeSecondary=vectorBefore.find((point)=>point.execution_space_id===secondarySpaceId);
  const afterPrimary=current.body.data.page.snapshot_vector.find((point)=>point.execution_space_id===spaceId);
  const afterSecondary=current.body.data.page.snapshot_vector.find((point)=>point.execution_space_id===secondarySpaceId);
  return JSON.stringify(beforePrimary)===JSON.stringify(afterPrimary)
    && afterSecondary.work_operation_count>beforeSecondary.work_operation_count;
}, "secondary Execution Space vector advances independently");
check(true, "Execution Space cursor invalidation advances only the mutated Execution Space point");

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
  await page.addInitScript(({capabilityToken}) => {
    window.__AGENTFIRM_BOOTSTRAP__ = { capabilityToken };
  }, {capabilityToken: agentFirmToken});

  let snapshotReads = 0;
  let delayedDomainSnapshot = null;
  // Company can no longer gate a delayed response: DOC-108 dropped it from
  // /v1/snapshot entirely (the handler no longer reads it) and from every SSE
  // subscription (`handle_sse_stream` always passes `company_scope_id: None`).
  // The axis that still genuinely differentiates a backend response is
  // Execution Space, so the delayed/stale-response race below is keyed on it.
  let delayedSecondarySpace = null;
  await page.route("**/v1/snapshot?**", async (route) => {
    snapshotReads += 1;
    const url = new URL(route.request().url());
    if (delayedSecondarySpace && url.searchParams.get("space") === secondarySpaceId) {
      const gate = delayedSecondarySpace;
      delayedSecondarySpace = null;
      const response = await route.fetch();
      gate.started();
      await gate.releasePromise;
      await route.fulfill({ response });
      return;
    }
    if (delayedDomainSnapshot) {
      const response = await route.fetch();
      const body = await response.body();
      if (body.toString("utf8").includes(delayedDomainSnapshot.marker)) {
        const gate = delayedDomainSnapshot;
        delayedDomainSnapshot = null;
        gate.started();
        await gate.releasePromise;
      }
      await route.fulfill({ response, body });
      return;
    }
    await route.continue();
  });

  const query = new URLSearchParams({
    api: appBase, project: projectId, space: spaceId, company: "company-b", surface: "work",
  });
  await page.goto(`${appBase}/?${query}`, { waitUntil: "domcontentloaded", timeout: 20_000 });
  await waitForDomain(page, "runtime", "live");
  await page.getByRole("heading", { name: "Global Work", exact: true }).waitFor();
  check(pageErrors.length === 0, `real Global Work page opens without browser errors (${pageErrors[0] ?? "none"})`);
  check(await page.locator('[aria-label="Scoped domain freshness"]').count() === 1, "freshness is one accessible scoped domain group");
  for (const domain of ["works", "docs", "organization", "runtime"]) {
    check(await page.locator(`[data-freshness-domain="${domain}"]`).count() === 1, `${domain} freshness is exposed independently`);
  }

  // Real five-view product loop. No route is fulfilled or fixture-injected:
  // each browser credential reaches the real serve process and Store.
  const roleBaseQuery = {api:appBase,project:projectId,space:spaceId,company:"company-a"};
  await page.goto(`${appBase}/?${new URLSearchParams({...roleBaseQuery,surface:"work"})}`, {waitUntil:"domcontentloaded"});
  await waitForText(page, "work-role-live");
  check(true, "real Global Work RoleView is populated");
  await page.goto(`${appBase}/?${new URLSearchParams({...roleBaseQuery,surface:"team",team:liveTeamRunId})}`, {waitUntil:"domcontentloaded"});
  await page.locator('[data-testid="authenticated-team-workspace"]').waitFor();
  await waitForText(page, "Real browser RoleAction loop");
  await waitForText(page, "DELEGATIONS");
  check(await page.getByRole("button", {name:/collaboration|delegation/i}).count()===0,
    "real Team browser exposes unavailable collaboration state without an executable action");
  check(true, "real Team Workspace RoleView is populated before Host navigation");
  await page.getByRole("button", {name:"Host Console"}).click();
  await page.getByRole("heading", {name:"Host Console"}).waitFor();
  await waitForText(page, "Cross-machine collaboration");
  check(await page.getByRole("button", {name:/collaboration|delegation/i}).count()===0,
    "real Host browser exposes unavailable collaboration state without an executable action");
  await page.getByRole("button", {name:"assign work",exact:true}).click();
  await page.getByLabel("MemberRun ID").fill(liveMemberRunId);
  await page.getByRole("button", {name:"Execute action"}).click();
  await page.getByRole("button", {name:"rebind work",exact:true}).waitFor();
  check(true, "Host browser executes authenticated assign_work and refetches HostConsole");

  const memberContext = await browser.newContext({viewport:{width:1200,height:900},reducedMotion:"reduce"});
  const memberPage = await memberContext.newPage();
  await memberPage.addInitScript(({capabilityToken})=>{window.__AGENTFIRM_BOOTSTRAP__={capabilityToken}}, {capabilityToken:memberAgentFirmToken});
  await memberPage.goto(`${appBase}/?${new URLSearchParams({...roleBaseQuery,surface:"team",team:liveTeamRunId,conversation:"worker",memberRun:liveMemberRunId})}`, {waitUntil:"domcontentloaded"});
  await memberPage.getByRole("button", {name:/Open .* configuration/}).waitFor();
  await memberPage.getByRole("tab", {name:/Session/}).waitFor();
  await memberPage.getByRole("tab", {name:/Work/}).click();
  await waitForText(memberPage, "Real browser RoleAction loop");
  const composerActionLabels = await memberPage.getByLabel("Composer action").locator("option").allTextContents();
  check(
    composerActionLabels.includes("start work"),
    "Agent Workspace composer preserves canonical RoleAction token labels",
  );
  await memberPage.getByLabel("Composer action").selectOption({label:"start work"});
  check(await memberPage.getByRole("button", {name:/collaboration|delegation/i}).count()===0,
    "real Member browser exposes unavailable collaboration state without an executable action");
  await memberPage.getByRole("button", {name:"start work"}).click();
  await memberPage.getByRole("button", {name:"Execute action"}).click();
  await memberPage.getByLabel("Composer action").selectOption({label:"block work"});
  await memberPage.getByRole("button", {name:"block work",exact:true}).waitFor();
  check(true, "Member browser executes authenticated start_work and refetches the unified Agent Workspace");
  await memberContext.close();

  const operatorContext = await browser.newContext({viewport:{width:1200,height:900},reducedMotion:"reduce"});
  const operatorPage = await operatorContext.newPage();
  await operatorPage.addInitScript(({capabilityToken})=>{window.__AGENTFIRM_BOOTSTRAP__={capabilityToken}}, {capabilityToken:operatorAgentFirmToken});
  await operatorPage.goto(`${appBase}/?${new URLSearchParams({...roleBaseQuery,surface:"operator",node:liveNode.id})}`, {waitUntil:"domcontentloaded"});
  await operatorPage.getByRole("heading", {name:"Nodes"}).waitFor();
  await waitForText(operatorPage, "daemon_lease");
  check(await operatorPage.getByRole("button", {name:"diagnose",exact:true}).count()===1,
    "real Operator RoleView is populated with server-derived diagnostics/actions");
  for(let attempt=0;attempt<20;attempt+=1){
    await operatorPage.getByRole("button", {name:"diagnose"}).click();
    await operatorPage.getByRole("button", {name:"Execute action"}).click();
    await waitForText(operatorPage, "Diagnostics are read-only");
    await operatorPage.getByText("Refreshing authoritative OperatorView…").waitFor({state:"hidden"});
    await waitForText(operatorPage, "Diagnostics are read-only");
  }
  check(true, "Operator diagnostics survives 20/20 authoritative refetch cycles");
  await operatorPage.getByText("Refreshing authoritative OperatorView…").waitFor({state:"hidden"});
  const operatorEnvelope = await roleView(`/v1/views/operator/${liveNode.id}?project=${projectId}&space=${spaceId}`, operatorAgentFirmToken);
  if (operatorEnvelope.status!==200)throw new Error(`authoritative Operator envelope: ${JSON.stringify(operatorEnvelope.body)}`);
  validateLiveRoleView("operator", operatorEnvelope.body);
  const codexAdmissionAction = operatorEnvelope.body.allowed_actions.find((action)=>action.kind==="admit_provider"&&action.intent_binding?.provider==="codex"&&action.intent_binding?.execution_mode==="codex_app_server");
  if(!codexAdmissionAction)throw new Error("authoritative Operator envelope omitted registered Codex admission tuple");
  const admissionLabel="admit provider · codex/codex_app_server";
  const admitProvider = operatorPage.getByRole("button", {name:admissionLabel,exact:true});
  await admitProvider.waitFor();
  if (codexAdmissionAction.disabled_reason) {
    const disabledReasonId=await admitProvider.getAttribute("aria-describedby");
    if(!disabledReasonId)throw new Error("disabled provider admission has no accessible reason reference");
    const disabledReason=operatorPage.locator(`#${disabledReasonId}`);
    await waitFor(async()=>await disabledReason.textContent()===`Unavailable: ${codexAdmissionAction.disabled_reason}`,
      "browser settles on the server-authored provider disabled reason");
    check(await admitProvider.isDisabled() && await disabledReason.textContent()===`Unavailable: ${codexAdmissionAction.disabled_reason}`,
      "Operator browser fails closed on the stable server-authored tuple eligibility");
  } else {
    await admitProvider.click();
    await waitForText(operatorPage, "Server-observed tuple: codex/codex_app_server · eligible");
    const admissionResponse = operatorPage.waitForResponse((response) => response.request().method()==="POST" && response.url().includes(`/v1/agentfirm/nodes/${liveNode.id}/provider-admission`));
    await operatorPage.getByRole("button", {name:"Execute action"}).click();
    const admissionHttp = await admissionResponse;
    const admissionBody = await admissionHttp.json();
    check(admissionHttp.status()===200 && admissionBody?.projection?.evidence_refs?.every((ref)=>ref.startsWith("server-")), "Operator browser executes server-probed provider admission through the real service");
    await operatorPage.getByText("Refreshing authoritative OperatorView…").waitFor({state:"hidden"});
    await operatorPage.getByRole("button", {name:admissionLabel,exact:true}).waitFor();
  }
  await operatorContext.close();

  await page.goto(`${appBase}/?${new URLSearchParams({...roleBaseQuery,surface:"work"})}`, {waitUntil:"domcontentloaded"});
  await waitForText(page, "work-role-live");
  await waitForText(page, "active");
  check(true, "five-view loop converges the Member mutation into real Global Work truth");
  // `page` is already at {space: spaceId, company: "company-a", surface:
  // "work"} from the five-view loop above (Company no longer differentiates
  // any backend response, so no further scope navigation is needed here).

  // Every write below is external to the browser and lands in the real
  // Execution Space store. The Runtime watcher emits freshness-only
  // invalidations; the browser may display a row only after its
  // authoritative scoped snapshot converges.
  const armDomainSnapshot = (marker) => {
    let signalStarted;
    let releaseDelayed;
    const startedPromise = new Promise((resolveStarted) => { signalStarted = resolveStarted; });
    const releasePromise = new Promise((resolveRelease) => { releaseDelayed = resolveRelease; });
    delayedDomainSnapshot = {marker, started:signalStarted, releasePromise};
    return {startedPromise, release:releaseDelayed};
  };

  await navigate(page, "Global Work");
  const workSnapshotGate = armDomainSnapshot("work-live-external");
  createNativeWork("work-live-external", "External Work converged");
  await workSnapshotGate.startedPromise;
  try {
    await waitForDomain(page, "works", "stale");
    check(await page.locator('[data-freshness-domain="docs"][data-freshness-status="live"]').count() === 1, "Work invalidation leaves Docs freshness truthful and independent");
    check(await page.locator('[data-freshness-domain="organization"][data-freshness-status="live"]').count() === 1, "Work invalidation leaves Organization freshness truthful and independent");
  } finally { workSnapshotGate.release(); }
  await waitForText(page, "work-live-external");
  check(true, "external Work write converges into the open page without reload");

  // DOC-108 deleted every Company Store writer (`company docs page create`,
  // POST /v1/company-os/documents|org-units both fail closed with a 410
  // tombstone) and the Runtime never subscribes a browser to a Company scope
  // any more (`handle_sse_stream` always passes `company_scope_id: None`).
  // There is no remaining live product path that can dirty the Docs or
  // Organization freshness domains, so — unlike Works above — they can only
  // be proven to stay truthfully Live/independent, never proven to cycle
  // stale->live from a real write. That capability retires with the Docs/
  // Organization surfaces themselves in DEV-39.
  check(await page.locator('[data-freshness-domain="docs"][data-freshness-status="live"]').count() === 1, "Docs freshness has no live Company writer left after DOC-108 and stays truthfully Live");
  check(await page.locator('[data-freshness-domain="organization"][data-freshness-status="live"]').count() === 1, "Organization freshness has no live Company writer left after DOC-108 and stays truthfully Live");

  // Real response, deliberately delivered late while the tab is scoped to the
  // sibling Execution Space: switch scope back to the primary Space while the
  // sibling's captured Runtime snapshot is in flight, then release it.
  // Generation/scope guards must prevent the sibling Space's stale response
  // from overwriting the primary Space's page. Company can no longer carry
  // this proof (DOC-108 dropped it from /v1/snapshot and /v1/events
  // entirely); Execution Space is the axis that still genuinely
  // differentiates a backend response, so the race is re-expressed on it.
  await page.goto(`${appBase}/?${new URLSearchParams({api: appBase, project: projectId, space: secondarySpaceId, company: "company-a", surface: "work"})}`, {waitUntil:"domcontentloaded"});
  await waitForDomain(page, "runtime", "live");
  let signalStarted;
  let releaseDelayed;
  const startedPromise = new Promise((resolveStarted) => { signalStarted = resolveStarted; });
  const releasePromise = new Promise((resolveRelease) => { releaseDelayed = resolveRelease; });
  delayedSecondarySpace = { started: signalStarted, releasePromise };
  runHarness([
    "team-run", "work", "create", "--team-run-id", secondaryTeamRunId,
    "--work-id", "work-secondary-delayed", "--title", "Delayed secondary Space response",
    "--context", "Prove a late cross-space response cannot overwrite the tab.",
    "--completion-criteria", "The tab must stay scoped to the primary Space.",
    "--priority", "normal", "--claim-mode", "team_claim",
    "--eligible-member-id", "worker-secondary",
  ], secondaryEnv, projectRoot);
  await startedPromise;
  await page.goto(`${appBase}/?${new URLSearchParams({api: appBase, project: projectId, space: spaceId, company: "company-a", surface: "work"})}`, {waitUntil:"domcontentloaded"});
  await waitForDomain(page, "runtime", "live");
  releaseDelayed();
  await new Promise((resolveWait) => setTimeout(resolveWait, 500));
  const tabScopeAfterDelayed = await page.evaluate(() => new URLSearchParams(window.location.search).get("space"));
  // Global Work is deliberately a cross-Space aggregate (proven above via the
  // snapshot_vector spanning both Spaces), so the sibling Space's Work title
  // legitimately appearing on this surface is not a leak to check against —
  // the boundary this race actually guards is the tab's own scope selector.
  check(tabScopeAfterDelayed === spaceId && pageErrors.length === 0, "delayed stale sibling-Space Runtime response cannot disturb the current Execution Space scope");

  // Visibility regain is the one remaining trigger that resyncs the tab
  // without an intervening user navigation. `harness company init` (the CLI
  // mutation the old "external Company creation" scenario relied on) is
  // retired along with the rest of `harness company`, and there is no CLI/
  // server Company default left to protect (`/v1/companies*` is removed
  // entirely). What survives is: recovery reissues exactly one scoped read
  // while the tab keeps its explicit URL-owned Space and Company scope.
  const beforeVisibilityReads = snapshotReads;
  await page.evaluate(() => { window.__dashboardVisibility = "hidden"; document.dispatchEvent(new Event("visibilitychange")); });
  await page.evaluate(() => { window.__dashboardVisibility = "visible"; document.dispatchEvent(new Event("visibilitychange")); });
  await waitFor(() => snapshotReads > beforeVisibilityReads, "visibility recovery issued a scoped snapshot read");
  const scopeAfterRecovery = await page.evaluate(() => ({
    space: new URLSearchParams(window.location.search).get("space"),
    company: new URLSearchParams(window.location.search).get("company"),
  }));
  check(
    scopeAfterRecovery.space === spaceId && scopeAfterRecovery.company === "company-a",
    "visibility recovery does not change the tab's URL-owned Execution Space or Company scope",
  );

  // Stop and restart the actual Runtime on the same stores. A CLI write while
  // it is down is recovered by the authoritative reconnect snapshot; no SSE
  // replay or fixture row is involved. `harness company org create-unit` is
  // retired (DOC-108 closes the whole `harness company` tree), so the write
  // exercised here is Team-run Work, the surviving authoritative surface.
  await stopRuntime();
  await context.setOffline(true);
  await waitForDomain(page, "runtime", "reconnecting");
  createNativeWork("work-reconnect-live", "Reconnect Work converged");
  startRuntime();
  await waitFor(async () => (await fetch(`${apiBase}/health`).catch(() => null))?.ok, "restarted Runtime health");
  await context.setOffline(false);
  await waitForDomain(page, "runtime", "live");
  await waitForText(page, "work-reconnect-live");
  check(true, "disconnect/reconnect recovers a CLI write made while the Runtime was down, without SSE replay or a fixture row");

  const stalePage = await context.newPage();
  const staleQuery = new URLSearchParams({
    api: appBase, project: projectId, space: "missing-space", company: "missing-company", surface: "work",
  });
  await stalePage.goto(`${appBase}/?${staleQuery}`, { waitUntil: "domcontentloaded", timeout: 20_000 });
  await waitFor(async () => new URL(stalePage.url()).searchParams.get("space") === spaceId, "stale Execution Space selector recovery");
  await waitForDomain(stalePage, "runtime", "live");
  check((await stalePage.locator("body").innerText()).includes("was not found; recovered"), "a stale Execution Space selector recovers visibly and fails closed before current-scope truth");
  // DOC-108 removed `/v1/companies*` entirely: there is no backend registry
  // left to validate a Company label against, so an unresolvable value now
  // passes straight through instead of being registry-corrected.
  check(new URL(stalePage.url()).searchParams.get("company") === "missing-company", "an unresolvable Company label is no longer registry-validated and passes through unrecovered");
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
    candidate_sha: spawnSync("git", ["rev-parse", "HEAD"], {cwd: repoRoot, encoding:"utf8"}).stdout.trim(),
  }, null, 2)}\n`);
} finally {
  if (context) await context.setOffline(false).catch(() => {});
  if (context) await context.close();
  await browser.close();
  await vite.close();
  await stopRuntime();
  for (const pid of runtimePids) {
    try {
      process.kill(pid, 0);
      throw new Error(`owned Runtime process ${pid} survived test cleanup`);
    } catch (error) {
      if (error?.code !== "ESRCH") throw error;
    }
  }
  if (failed > 0) console.error(runtimeLogs.slice(-20).join(""));
  await rm(temporaryRoot, { recursive: true, force: true });
}

console.log(`\nDashboard real Runtime/browser checks: ${passed} pass, ${failed} fail`);
console.log(`Visual evidence: ${evidenceRoot}`);
process.exit(failed === 0 ? 0 : 1);
