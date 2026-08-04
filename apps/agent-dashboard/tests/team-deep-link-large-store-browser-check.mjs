#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";

const dashboardRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = join(dashboardRoot, "fixtures/workbench-layout-v2-native-v1");

async function jsonl(name) {
  return (await readFile(join(fixtureRoot, `${name}.jsonl`), "utf8"))
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
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

const snapshot = {
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
  agent_message_routes: [],
  pending_interactions: [],
  company_os: {},
};
const teamRunId = snapshot.team_runs.find((run) => run.member_run_ids?.length)?.id;
if (!teamRunId) throw new Error("fixture does not contain a current TeamRun with members");
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
    return route.fulfill({ status: 200, contentType: "application/json", body: '{"projects":[],"current":""}' });
  }
  if (url.pathname === "/v1/spaces") {
    return route.fulfill({ status: 200, contentType: "application/json", body: '{"spaces":[],"current":""}' });
  }
  if (url.pathname === "/v1/companies") {
    return route.fulfill({ status: 200, contentType: "application/json", body: '{"companies":[],"current":""}' });
  }
  if (url.pathname === "/v1/workflows") {
    return route.fulfill({ status: 200, contentType: "application/json", body: '{"workflows":[]}' });
  }
  if (url.pathname === "/v1/events") {
    return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
  }
  if (url.pathname === "/v1/meta") {
    // The persistent provenance footer (issue #307) polls this on its own;
    // stub it like every other endpoint so the fixture run stays free of
    // console-logged network errors. Its rev is deliberately not the real
    // build rev — this check does not assert anything about the footer/
    // banner, only that the rest of the surface renders correctly.
    return route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        git_rev: "fixture0",
        built_at: null,
        store_root: fixtureRoot,
        latest_op_seq: workOperations.length,
        server_version: "0.0.0-fixture",
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
  await page.getByTestId("team-works-board").getByText("Validate responsive Team UX", { exact: true }).first().waitFor({ timeout: 15_000 });
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
