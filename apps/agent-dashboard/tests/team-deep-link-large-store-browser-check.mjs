#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = join(dashboardRoot, "fixtures/workbench-layout-v2-native-v1");
const roleFixtureRoot = join(dashboardRoot, "fixtures/wave4-local-agentfirm-v1");
const buildSha = execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();

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
const workDeliveries = await jsonl("current_work_deliveries");
const worksById = new Map();
for (const operation of workOperations) {
  worksById.set(operation.work.id, operation.work);
}

const snapshot = {
  generated_at: "2026-07-29T00:00:00Z",
  teams: await jsonl("teams"),
  missions: await jsonl("missions"),
  legacy_waves: await jsonl("waves"),
  team_runs: await jsonl("team_runs"),
  member_runs: await jsonl("member_runs"),
  team_messages: await jsonl("team_messages"),
  works: [...worksById.values()],
  work_events: workOperations.map((operation) => operation.event),
  work_deliveries: workDeliveries,
  member_actions: await jsonl("member_actions"),
  delegation_runs: await jsonl("delegation_runs"),
  team_run_events: await jsonl("team_run_events"),
  evidence: await jsonl("evidence"),
  members: [],
  messages: [],
  events: [],
  provider_child_threads: [],
  team_supervisor_leases: [],
  team_member_close_requests: [],
  company_os: {},
};
const teamRunId = snapshot.team_runs.find((run) => run.member_run_ids?.length)?.id;
if (!teamRunId) throw new Error("fixture does not contain a current TeamRun with members");
const team = snapshot.teams.find((candidate) => candidate.id === snapshot.team_runs.find((run) => run.id === teamRunId)?.agent_team_id);
if (!team) throw new Error("fixture does not contain the selected TeamRun's Team");
const roleWorks = snapshot.works.filter((work) => work.team_run_id === teamRunId).map((work) => ({
  work_id: work.id, work_revision: work.version, team_id: team.id, mission_id: team.mission_id ?? "",
  accountable_team_id: team.id, assignee_membership_id: null,
  assignee_kind: work.owner_member_id ? "member" : "unassigned",
  assignee_ref: { kind: work.owner_member_id ? "agent_member" : "unassigned", membership_id: null, membership_state: null, agent_member_id: work.owner_member_id, display_name: null },
  migration_state: "canonical", title: work.title, context_markdown: work.context_markdown,
  completion_criteria_markdown: work.completion_criteria_markdown, claim_mode: work.claim_mode,
  eligible_member_ids: work.eligible_member_ids, prerequisite_work_ids: work.prerequisite_work_ids,
  successor_work_ids: [], readiness: { state: "ready", reason_codes: [], unsatisfied_prerequisite_work_ids: [], failed_or_cancelled_prerequisite_work_ids: [] },
  blocker_reason: work.blocker_reason, result_summary: work.result_summary, artifact_refs: work.artifact_refs,
  check_refs: work.check_refs, latest_event: null, owner_actor_ref: null, current_member_run_ref: work.active_member_run_id,
  phase: work.phase, condition: work.condition, resolution: work.resolution, priority: work.priority,
  module_refs: [], gate_summary: { required: 0, passed: 0, failed: 0, pending: 0, waived: 0, stale: 0 },
  latest_report_ref: null, latest_finding_refs: [], latest_failure_ref: null, delivery_summary: {},
  runtime_summary: { state: "idle", generation: null, freshness: "current", work_execution_binding_id: null, agent_session_id: null, agent_session_generation: null, provider: null, native_session_id: null },
  workspace_summary: { binding_id: null, cwd: null, lifecycle: "unbound", safety: "unknown" },
  delegation_summary: {}, updated_at: work.updated_at,
}));
const teamWorkspace = await json("team-workspace");
teamWorkspace.source_execution_space_id = "fixture-space";
teamWorkspace.data.team = { ...teamWorkspace.data.team, team_id: team.id, display_name: team.name, mission_id: team.mission_id ?? "", host_agent_id: team.host_agent_id, node_id: team.node_id, latest_run: { id: teamRunId, status: "running" } };
teamWorkspace.data.works = roleWorks;
teamWorkspace.data.work_graph = { nodes: roleWorks, edges: [], ready_work_ids: roleWorks.map((work) => work.work_id), attention_work_ids: [] };
teamWorkspace.data.page = { as_of_event_sequence: workOperations.length, item_count: roleWorks.length, next_cursor: null };
let scopedReads = 0;
let globalReads = 0;

const vite = await createServer({
  configFile: join(dashboardRoot, "vite.config.ts"),
  server: { host: "127.0.0.1", port: 0 },
  logLevel: "silent",
});
await vite.listen();
const address = vite.httpServer.address();
const base = `http://127.0.0.1:${address.port}`;
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();
const browserErrors = [];
page.on("console", (message) => { if (message.type() === "error") browserErrors.push(message.text()); });
page.on("pageerror", (error) => browserErrors.push(error.message));

await page.route("**/v1/**", async (route) => {
  const url = new URL(route.request().url());
  if (url.pathname === `/v1/team-runs/${encodeURIComponent(teamRunId)}/snapshot`) {
    scopedReads += 1;
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(snapshot) });
  }
  if (url.pathname === "/v1/snapshot") {
    globalReads += 1;
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ ...snapshot, irrelevant_history: "x".repeat(40_500_613) }),
    });
  }
  if (url.pathname === "/v1/projects") {
    return route.fulfill({ status: 200, contentType: "application/json", body: '{"projects":[{"id":"fixture-project","is_current":true}],"current":"fixture-project"}' });
  }
  if (url.pathname === "/v1/spaces") {
    return route.fulfill({ status: 200, contentType: "application/json", body: '{"spaces":[{"id":"fixture-space","is_current":true}],"current":"fixture-space"}' });
  }
  if (url.pathname === "/v1/companies") {
    return route.fulfill({ status: 200, contentType: "application/json", body: '{"companies":[],"current":""}' });
  }
  if (url.pathname === "/v1/events") {
    return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
  }
  if (url.pathname === "/v1/views/viewer-context") {
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({
      view_kind: "viewer_context", schema_version: "agentfirm.role_views.v1", source_execution_space_id: "fixture-space",
      source_store_identity: "large-store-fixture", as_of_event_sequence: workOperations.length,
      generated_at: snapshot.generated_at, freshness: "current", attention: [], allowed_actions: [],
      data: { viewer_actor_ref: { kind: "agent_member", id: team.host_agent_id }, teams: [{ team_id: team.id, display_name: team.name, viewer_role: "host", viewer_agent_member_id: team.host_agent_id, default_conversation: "host", latest_run_id: teamRunId, team_run_ids: [teamRunId], current_member_run_id: null }] },
    }) });
  }
  if (url.pathname.startsWith("/v1/views/team-workspace/")) {
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(teamWorkspace) });
  }
  if (url.pathname.startsWith("/v1/views/team-inbox/")) {
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({
      view_kind: "team_inbox", schema_version: "agentfirm.role_views.v1", source_execution_space_id: "fixture-space",
      source_store_identity: "large-store-fixture", as_of_event_sequence: workOperations.length,
      generated_at: snapshot.generated_at, freshness: "current", attention: [], allowed_actions: [],
      data: { team: { team_id: team.id, display_name: team.name, team_revision: 1, node_id: team.node_id, status: "active" }, subscription: null, items: [], page: { as_of_event_sequence: workOperations.length, item_count: 0, next_cursor: null } },
    }) });
  }
  if (url.pathname === "/v1/meta") {
    // The persistent provenance footer (issue #307) polls this on its own;
    // stub the current capability envelope so this fixture reaches the
    // bounded TeamRun snapshot behavior it is intended to exercise.
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        git_rev: buildSha,
        build_sha: buildSha,
        built_at: null,
        store_root: fixtureRoot,
        latest_op_seq: workOperations.length,
        server_version: "0.0.0-fixture",
        protocol_version: "agentfirm-member-trust/1",
        schema_version: "agentfirm.role_views.v1",
        action_manifest_version: "agentfirm.role_actions.v1",
        capability_auth: "x-agentfirm-token",
      }),
    });
  }
  return route.fulfill({ status: 404, contentType: "application/json", body: '{"error":"not_found"}' });
});

try {
  const query = new URLSearchParams({ api: base, surface: "team", team: teamRunId });
  await page.goto(`${base}/?${query}`, { waitUntil: "domcontentloaded", timeout: 15_000 });
  try {
    await page.getByText("Shared Works", { exact: true }).waitFor({ timeout: 15_000 });
  } catch (error) {
    const body = (await page.locator("body").innerText()).slice(0, 2_000);
    throw new Error(`${error.message}\nBrowser errors: ${browserErrors.join(" | ")}\nBody: ${body}`);
  }
  await page.getByTestId("role-view-team-works").getByText("Validate responsive Team UX", { exact: true }).first().waitFor({ timeout: 15_000 });
  if (await page.getByText("Team attempt not found", { exact: true }).count()) {
    throw new Error("deep link rendered Team attempt not found");
  }
  if (scopedReads < 1) throw new Error("TeamRun-scoped snapshot was not requested");
  if (globalReads !== 0) throw new Error(`deep link requested ${globalReads} global snapshots`);
  if (snapshot.works.length !== 6 || snapshot.work_events.length !== workOperations.length || snapshot.work_deliveries.length < 1) {
    throw new Error("TeamRun-scoped snapshot omitted Work, WorkEvent, or WorkDelivery projections");
  }
  console.log(`validated large-store Team deep link via ${scopedReads} scoped read(s), 0 global reads`);
} finally {
  await browser.close();
  await vite.close();
}
