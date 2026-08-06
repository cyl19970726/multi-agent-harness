#!/usr/bin/env node

/**
 * Rendered-DOM acceptance for the operator console closure (spec:
 * docs/design/dashboard-operator-console.md).
 *
 * Proves the creation + chat flows a single-console operator needs, against
 * a mocked live source: durable AgentMember creation, independent team +
 * first run + start, Mission Log append replacing retired Wave writes, and
 * an explicit-response-intent team message. POST bodies are captured and the
 * mock snapshot mutates, so the UI must reflect new rows without reload.
 */

import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = join(dashboardRoot, "fixtures/workbench-layout-v2-native-v1");

let passed = 0;
let failed = 0;
function check(condition, message) {
  if (condition) {
    console.log(`  PASS  ${message}`);
    passed += 1;
  } else {
    console.log(`  FAIL  ${message}`);
    failed += 1;
  }
}

async function jsonl(name) {
  return (await readFile(join(fixtureRoot, `${name}.jsonl`), "utf8"))
    .split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

const workOperations = await jsonl("work_operations");
const worksById = new Map();
const deliveriesById = new Map();
for (const operation of workOperations) {
  worksById.set(operation.work.id, operation.work);
  for (const delivery of operation.deliveries ?? []) deliveriesById.set(delivery.id, delivery);
  for (const update of operation.delivery_updates ?? []) {
    const delivery = deliveriesById.get(update.delivery_id);
    if (delivery) deliveriesById.set(update.delivery_id, { ...delivery, ...update, id: delivery.id });
  }
}

const baseline = {
  generated_at: "2026-07-29T00:00:00Z",
  teams: await jsonl("teams"),
  missions: await jsonl("missions"),
  waves: await jsonl("waves"),
  team_runs: await jsonl("team_runs"),
  member_runs: await jsonl("member_runs"),
  team_messages: await jsonl("team_messages"),
  works: [...worksById.values()],
  work_events: workOperations.map((operation) => operation.event),
  work_deliveries: [...deliveriesById.values()],
  member_actions: await jsonl("member_actions"),
  delegation_runs: await jsonl("delegation_runs"),
  team_run_events: await jsonl("team_run_events"),
  evidence: await jsonl("evidence"),
  members: [], messages: [], events: [], provider_child_threads: [],
  workflow_runs: [], workflow_steps: [], workflow_patches: [],
  workflow_artifact_manifests: [], team_supervisor_leases: [],
  team_member_close_requests: [], agent_message_routes: [],
  pending_interactions: [], company_os: {},
};

const missionId = baseline.missions[0]?.id;
const fixtureRunId = baseline.team_runs.find((run) => run.member_run_ids?.length)?.id;

const state = {
  members: [],
  teams: [],
  teamRuns: [],
  memberRuns: [
    // Closed terminal member with a resumable native session: exercises the
    // capability-labelled Resume control on the Member focus page.
    {
      id: "member-console-resume",
      team_run_id: fixtureRunId,
      slot_id: "resume",
      name: "Resume Candidate",
      role: "implementer",
      provider: "kimi",
      model: "K2.5",
      status: "completed",
      coordination_status: "closed",
      native_session: {
        provider: "kimi",
        execution_mode: "kimi_acp",
        native_session_id: "session_resume_fixture",
        native_locator_kind: "kimi_code_session",
        adapter_contract_version: "kimi-acp-v1",
        availability: "available",
        supports_resume: true,
      },
      owned_paths: [],
      started_at: "2026-07-19T10:00:00Z",
      last_event_at: "2026-07-19T11:00:00Z",
      finished_at: "2026-07-19T11:00:00Z",
    },
  ],
  teamMessages: [],
  missionLog: [],
  posts: [],
  // Console follow-up state: attention lifecycle + mission brief/links.
  attentions: [
    {
      id: "attention-console-1",
      team_run_id: fixtureRunId,
      kind: "work_review_requested",
      work_id: baseline.works[0]?.id ?? "work-fixture",
      work_version: 2,
      source_event_ref: "event-fixture",
      member_run_id: null,
      status: "actionable",
      attempt: 1,
      claim_id: null,
      created_at: "2026-07-29T00:00:00Z",
      updated_at: "2026-07-29T00:00:00Z",
    },
  ],
  missionContext: null,
  linkedMissionTeamIds: new Set(),
};

function buildSnapshot() {
  return {
    ...baseline,
    generated_at: new Date().toISOString(),
    missions: baseline.missions.map((mission) => mission.id === missionId
      ? {
          ...mission,
          agent_team_ids: [...state.linkedMissionTeamIds],
          context: state.missionContext ?? mission.context,
        }
      : mission),
    teams: [...baseline.teams, ...state.teams],
    members: [...baseline.members, ...state.members],
    team_runs: [...baseline.team_runs, ...state.teamRuns].map((run) => run.id === fixtureRunId
      ? { ...run, member_run_ids: [...(run.member_run_ids ?? []), "member-console-resume"] }
      : run),
    member_runs: [...baseline.member_runs, ...state.memberRuns],
    team_messages: [...baseline.team_messages, ...state.teamMessages],
    mission_log: [...state.missionLog],
  };
}

const vite = await createServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 0 },
  logLevel: "error",
});
await vite.listen();
const base = `http://127.0.0.1:${vite.httpServer.address().port}`;
const browser = await chromium.launch();

async function mockRoutes(page) {
  await page.route("**/v1/**", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (request.method() === "POST") {
      const body = request.postDataJSON();
      state.posts.push({ path: url.pathname, body });
      if (url.pathname === "/v1/agents") {
        state.members.push({
          id: `agent-console-${state.members.length + 1}`,
          name: body.name,
          role: body.role,
          provider: body.provider ?? "codex",
          model: body.model ?? null,
          status: "idle",
          native_session: null,
        });
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, result: state.members.at(-1) }) });
      }
      if (url.pathname === "/v1/teams") {
        state.teams.push({
          id: `team-console-${state.teams.length + 1}`,
          name: body.name,
          description: body.description ?? "",
          owner_agent_id: body.lead_agent_id ?? "host",
          member_ids: body.member ?? [],
          status: "active",
        });
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, result: state.teams.at(-1) }) });
      }
      if (url.pathname === "/v1/team-runs") {
        const run = {
          id: `teamrun-console-${state.teamRuns.length + 1}`,
          objective: body.objective,
          agent_team_id: body.agent_team_id ?? null,
          mission_id: body.mission_id ?? null,
          status: "planning",
          attempt: 1,
          member_run_ids: [],
          execution_root: body.execution_root ?? null,
          created_at: new Date().toISOString(),
        };
        state.teamRuns.push(run);
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, result: { team_run: run, member_runs: [] } }) });
      }
      const start = url.pathname.match(/^\/v1\/team-runs\/([^/]+)\/start$/);
      if (start) {
        const run = state.teamRuns.find((candidate) => candidate.id === decodeURIComponent(start[1]));
        if (run) run.status = "running";
        return route.fulfill({ status: 202, contentType: "application/json", body: JSON.stringify({ ok: true, result: { started: Boolean(run) } }) });
      }
      const messages = url.pathname.match(/^\/v1\/team-runs\/([^/]+)\/messages$/);
      if (messages) {
        state.teamMessages.push({
          id: `msg-console-${state.teamMessages.length + 1}`,
          team_run_id: decodeURIComponent(messages[1]),
          kind: body.kind ?? "message",
          body: body.body ?? "",
          from_member_id: body.sender_id ?? "host",
          to_member_ids: body.to_member_ids ?? [],
          response_intent: body.response_intent ?? null,
          created_at: new Date().toISOString(),
        });
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, result: state.teamMessages.at(-1) }) });
      }
      const log = url.pathname.match(/^\/v1\/missions\/([^/]+)\/log$/);
      if (log) {
        state.missionLog.push({
          id: `mission-log-${state.missionLog.length + 1}`,
          mission_id: decodeURIComponent(log[1]),
          revision: state.missionLog.length + 1,
          kind: body.kind,
          body: body.body,
          actor: body.actor ?? "host",
          created_at: new Date().toISOString(),
        });
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, result: state.missionLog.at(-1) }) });
      }
      const contextRoute = url.pathname.match(/^\/v1\/missions\/([^/]+)\/context$/);
      if (contextRoute) {
        state.missionContext = body.context ?? "";
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, result: { context: state.missionContext } }) });
      }
      const linkRoute = url.pathname.match(/^\/v1\/missions\/([^/]+)\/link-team$/);
      if (linkRoute) {
        state.linkedMissionTeamIds.add(body.team_id);
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true }) });
      }
      const unlinkRoute = url.pathname.match(/^\/v1\/missions\/([^/]+)\/unlink-team$/);
      if (unlinkRoute) {
        state.linkedMissionTeamIds.delete(body.team_id);
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true }) });
      }
      const ackRoute = url.pathname.match(/^\/v1\/host-attentions\/([^/]+)\/ack$/);
      if (ackRoute) {
        const attention = state.attentions.find((row) => row.id === decodeURIComponent(ackRoute[1]));
        if (attention) attention.status = "acknowledged";
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true, result: { attention, idempotent: false } }) });
      }
      const resumeRoute = url.pathname.match(/^\/v1\/team-runs\/([^/]+)\/members\/([^/]+)\/resume$/);
      if (resumeRoute) {
        return route.fulfill({ status: 202, contentType: "application/json", body: JSON.stringify({ ok: true, result: { via: "resume", member_run: { id: decodeURIComponent(resumeRoute[2]), coordination_status: "active" } } }) });
      }
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ ok: true }) });
    }
    if (url.pathname === "/v1/host-attentions") {
      const wanted = url.searchParams.get("team_run_id");
      const rows = state.attentions.filter((row) => row.team_run_id === wanted);
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ attentions: rows }) });
    }
    if (url.pathname === `/v1/team-runs/${encodeURIComponent(fixtureRunId)}/snapshot`
      || /^\/v1\/team-runs\/[^/]+\/snapshot$/.test(url.pathname)
      || url.pathname === "/v1/snapshot") {
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(buildSnapshot()) });
    }
    if (url.pathname === "/v1/projects") return route.fulfill({ status: 200, contentType: "application/json", body: '{"projects":[],"current":""}' });
    if (url.pathname === "/v1/spaces") return route.fulfill({ status: 200, contentType: "application/json", body: '{"spaces":[],"current":""}' });
    if (url.pathname === "/v1/companies") return route.fulfill({ status: 200, contentType: "application/json", body: '{"companies":[],"current":""}' });
    if (url.pathname === "/v1/workflows") return route.fulfill({ status: 200, contentType: "application/json", body: '{"workflows":[]}' });
    if (url.pathname === "/v1/events") return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
    if (url.pathname === "/v1/meta") {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ git_rev: "fixture0", built_at: null, store_root: "/fixture/store", latest_op_seq: 0, server_version: "0.0.0-fixture" }),
      });
    }
    return route.fulfill({ status: 404, contentType: "application/json", body: '{"error":"not_found"}' });
  });
}

function lastPost(pathPattern) {
  return [...state.posts].reverse().find((post) => pathPattern.test(post.path));
}

async function newPage(query) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
    deviceScaleFactor: 1,
    reducedMotion: "reduce",
  });
  const page = await context.newPage();
  await mockRoutes(page);
  await page.goto(`${base}/?${new URLSearchParams({ api: base, ...query })}`, { waitUntil: "domcontentloaded", timeout: 20_000 });
  return { context, page };
}

try {
  // ── Flow A: create a durable Agent Member from the Agents directory ──
  {
    const { context, page } = await newPage({ surface: "agents" });
    await page.getByRole("button", { name: "New Agent Member" }).click();
    await page.getByLabel("Name").fill("console-agent");
    await page.getByLabel("Role").fill("implementer");
    await page.getByRole("button", { name: "Create member" }).click();
    await page.getByText("console-agent").first().waitFor({ timeout: 10_000 }).catch(() => {});
    const post = lastPost(/^\/v1\/agents$/);
    check(Boolean(post), "create member posts /v1/agents");
    check(post?.body?.name === "console-agent" && post?.body?.role === "implementer" && post?.body?.provider === "kimi",
      "create member body carries name, role, and the registered default provider");
    check(await page.getByText("console-agent").first().isVisible().catch(() => false),
      "created member appears in the directory without reload");
    await context.close();
  }

  // ── Flow B: independent team → first run → start, all from the console ──
  {
    const { context, page } = await newPage({ surface: "team" });
    await page.getByRole("button", { name: "New Agent Team" }).click();
    await page.getByLabel("Team name").fill("Console Team");
    await page.getByLabel("Description").fill("Created end-to-end from the console");
    await page.getByRole("button", { name: "Create team" }).click();
    await page.getByText("Console Team").first().waitFor({ timeout: 10_000 }).catch(() => {});
    check(await page.getByText("Console Team").first().isVisible().catch(() => false),
      "created team is visible even before it has a run");
    const teamCreate = lastPost(/^\/v1\/teams$/);
    check(Boolean(teamCreate) && teamCreate.body.name === "Console Team" && teamCreate.body.lead_agent_id === "host",
      "create team posts /v1/teams with host lead default");

    // Scope to the run-less teams section and match the exact button name:
    // attempt cards wrap in a role=button whose accessible name also contains
    // "New run", which a loose match would hit and navigate instead.
    await page.getByRole("region", { name: "Teams without runs" })
      .getByRole("button", { name: "New run", exact: true }).click();
    await page.getByLabel("Objective").fill("First console-driven attempt");
    await page.getByRole("button", { name: "Create run" }).click();
    await page.waitForTimeout(800);
    const runCreate = lastPost(/^\/v1\/team-runs$/);
    check(Boolean(runCreate) && runCreate.body.objective === "First console-driven attempt",
      "create run posts /v1/team-runs with the attempt objective");
    check(Boolean(runCreate?.body?.agent_team_id),
      "console-created run is linked to its Agent Team definition");

    const startButton = page.getByRole("button", { name: "Start now" });
    await startButton.waitFor({ state: "visible", timeout: 10_000 }).catch(() => {});
    await startButton.waitFor({ state: "attached" }).then(async () => {
      // Start stays disabled until the refreshed snapshot reveals the new run id.
      for (let attempt = 0; attempt < 20 && await startButton.isDisabled().catch(() => true); attempt += 1) {
        await page.waitForTimeout(300);
      }
    }).catch(() => {});
    await startButton.click().catch(() => {});
    await page.waitForTimeout(600);
    const start = lastPost(/\/start$/);
    check(Boolean(start) && /^\/v1\/team-runs\/[^/]+\/start$/.test(start.path),
      "Start now posts /v1/team-runs/{id}/start without leaving the dialog");
    await context.close();
  }

  // ── Flow C: Mission Log replaces retired Wave writes ──
  {
    const { context, page } = await newPage({ surface: "missions", mission: missionId });
    await page.getByRole("button", { name: "Append Host judgment" }).click();
    const body = "Console acceptance: advance the current Wave from recorded evidence.";
    await page.getByLabel("Entry").fill(body).catch(async () => {
      // Fall back to the dialog's textarea if the field label differs.
      await page.getByRole("dialog").getByRole("textbox").last().fill(body);
    });
    await page.getByRole("button", { name: "Append log entry" }).click();
    await page.getByText(body).first().waitFor({ timeout: 10_000 }).catch(() => {});
    const post = lastPost(new RegExp(`^/v1/missions/${missionId}/log$`));
    check(Boolean(post) && post.body.kind === "judgment" && post.body.actor === "operator",
      "append judgment posts /v1/missions/{id}/log as operator judgment");
    check(await page.getByText(body).first().isVisible().catch(() => false),
      "appended Mission Log entry renders without reload");
    check(await page.getByRole("button", { name: "Add Wave" }).count().then((count) => count === 0).catch(() => false),
      "retired Add Wave control is gone from the Mission canvas");
    await context.close();
  }

  // ── Flow D: team chat with explicit response intent ──
  {
    const { context, page } = await newPage({ surface: "team", team: fixtureRunId });
    await page.getByText("Shared Works", { exact: true }).waitFor({ timeout: 20_000 }).catch(() => {});
    await page.getByLabel("Response intent").selectOption("informational");
    await page.getByLabel("Team message").fill("Console chat acceptance: informational context only.");
    await page.getByRole("button", { name: "Send" }).first().click();
    await page.waitForTimeout(800);
    const message = lastPost(new RegExp(`^/v1/team-runs/${fixtureRunId}/messages$`));
    check(Boolean(message) && message.body.body === "Console chat acceptance: informational context only.",
      "team composer posts the message to the run's message route");
    check(message?.body?.response_intent === "informational",
      "team composer carries the explicit response intent");
    await context.close();
  }

  // ── Flow E: Host attention console surface ──
  {
    const { context, page } = await newPage({ surface: "team", team: fixtureRunId });
    await page.getByText("Shared Works", { exact: true }).waitFor({ timeout: 20_000 }).catch(() => {});
    const attentionModule = page.getByRole("region", { name: "Host attention" });
    await attentionModule.waitFor({ timeout: 10_000 }).catch(() => {});
    check(await attentionModule.isVisible().catch(() => false),
      "Host attention module renders in the War Room when a Work review is requested");
    await page.getByRole("button", { name: "Ack" }).first().click();
    await page.waitForTimeout(800);
    const ack = lastPost(/^\/v1\/host-attentions\/attention-console-1\/ack$/);
    check(Boolean(ack) && ack.body.acknowledged_by === "operator",
      "Ack posts /v1/host-attentions/{id}/ack as operator");
    await context.close();
  }

  // ── Flow F: Mission brief edit + team link/unlink ──
  {
    const { context, page } = await newPage({ surface: "missions", mission: missionId });
    await page.getByRole("button", { name: "Edit context" }).click();
    await page.getByRole("textbox", { name: "Context", exact: true }).fill("Console acceptance: durable brief rewritten from the console.");
    await page.getByRole("button", { name: "Save context" }).click();
    await page.waitForTimeout(800);
    const contextPost = lastPost(new RegExp(`^/v1/missions/${missionId}/context$`));
    check(Boolean(contextPost) && String(contextPost.body.context).includes("durable brief rewritten"),
      "Edit context posts /v1/missions/{id}/context with the rewritten brief");

    await page.getByLabel("Team to link").selectOption("team-platform-foundation");
    await page.getByRole("button", { name: "Link team" }).click();
    await page.waitForTimeout(800);
    check(Boolean(lastPost(new RegExp(`^/v1/missions/${missionId}/link-team$`))),
      "Link team posts /v1/missions/{id}/link-team");
    await page.getByRole("button", { name: "Unlink" }).first().waitFor({ timeout: 8_000 }).catch(() => {});
    await page.getByRole("button", { name: "Unlink" }).first().click();
    await page.waitForTimeout(800);
    check(Boolean(lastPost(new RegExp(`^/v1/missions/${missionId}/unlink-team$`))),
      "Unlink posts /v1/missions/{id}/unlink-team");
    await context.close();
  }

  // ── Flow G: capability-labelled resume for a closed resumable member ──
  {
    const { context, page } = await newPage({ surface: "team", team: fixtureRunId, memberRun: "member-console-resume" });
    const resumeButton = page.getByRole("button", { name: "Resume session" });
    await resumeButton.waitFor({ timeout: 15_000 }).catch(() => {});
    check(await resumeButton.isVisible().catch(() => false),
      "closed resumable member renders a capability-labelled Resume session control");
    await resumeButton.click().catch(() => {});
    await page.waitForTimeout(800);
    check(Boolean(lastPost(new RegExp(`^/v1/team-runs/${fixtureRunId}/members/member-console-resume/resume$`))),
      "Resume session posts the standalone member resume route");
    await context.close();
  }
} finally {
  await browser.close();
  await vite.close();
}

console.log(`\noperator console checks: ${passed} pass, ${failed} fail`);
if (failed > 0) process.exit(1);
