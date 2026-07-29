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

const snapshot = {
  generated_at: "2026-07-29T00:00:00Z",
  teams: await jsonl("teams"),
  missions: await jsonl("missions"),
  waves: await jsonl("waves"),
  team_runs: await jsonl("team_runs"),
  member_runs: await jsonl("member_runs"),
  team_messages: await jsonl("team_messages"),
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
const teamRunId = snapshot.team_runs[0].id;
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
  return route.fulfill({ status: 404, contentType: "application/json", body: '{"error":"not_found"}' });
});

try {
  const query = new URLSearchParams({ api: base, surface: "team", team: teamRunId });
  await page.goto(`${base}/?${query}`, { waitUntil: "domcontentloaded", timeout: 15_000 });
  await page.getByText("Team Activity", { exact: true }).waitFor({ timeout: 15_000 });
  if (await page.getByText("Team attempt not found", { exact: true }).count()) {
    throw new Error("deep link rendered Team attempt not found");
  }
  if (scopedReads < 1) throw new Error("TeamRun-scoped snapshot was not requested");
  if (globalReads !== 0) throw new Error(`deep link requested ${globalReads} global snapshots`);
  console.log(`validated large-store Team deep link via ${scopedReads} scoped read(s), 0 global reads`);
} finally {
  await browser.close();
  await vite.close();
}
