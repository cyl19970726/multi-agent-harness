#!/usr/bin/env node
// Native Mission/Wave Agent Team selector checks. This imports the real
// TypeScript implementation rather than duplicating its ordering semantics.

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
let passed = 0;
let failed = 0;

function ok(message) {
  console.log(`  PASS  ${message}`);
  passed += 1;
}

function bad(message) {
  console.log(`  FAIL  ${message}`);
  failed += 1;
}

async function loadSelectors() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "team-selectors-"));
  try {
    const source = await readFile(join(here, "..", "src", "model", "teamSelectors.ts"), "utf8");
    const output = ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const path = join(directory, "teamSelectors.mjs");
    await writeFile(path, output, "utf8");
    return await import(pathToFileURL(path).href);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

function fixture() {
  return {
    missions: [{ id: "mission-1", title: "Build console", objective: "ship" }],
    waves: [
      { id: "wave-2", mission_id: "mission-1", index: 2, title: "Second", objective: "second", executor_kind: "agent_team" },
      { id: "wave-1", mission_id: "mission-1", index: 1, title: "First", objective: "first", executor_kind: "agent_team", executor_run_ids: ["run-2", "run-1"] },
    ],
    team_runs: [
      { id: "run-1", mission_id: "mission-1", wave_id: "wave-1", member_run_ids: ["member-1"], created_at: "2026-07-19T00:00:02Z" },
      { id: "run-2", mission_id: "mission-1", wave_id: "wave-1", member_run_ids: ["member-2"], created_at: "2026-07-19T00:00:01Z" },
    ],
    member_runs: [
      { id: "member-1", team_run_id: "run-1", name: "Worker", status: "waiting" },
      { id: "member-2", team_run_id: "run-2", name: "Critic", status: "blocked" },
    ],
    team_messages: [
      { id: "message-progress", team_run_id: "run-1", from_member_id: "member-1", kind: "progress", correlation_id: "corr-1", created_at: "2026-07-19T00:00:03Z" },
      { id: "message-assignment", team_run_id: "run-1", from_member_id: "host", to_member_ids: ["member-1"], kind: "assignment", correlation_id: "corr-1", created_at: "2026-07-19T00:00:01Z", deliveries: [{ member_id: "member-1", status: "delivered" }] },
      { id: "message-review", team_run_id: "run-1", from_member_id: "member-1", kind: "review_request", created_at: "2026-07-19T00:00:04Z" },
    ],
    member_actions: [{ id: "action-1", team_run_id: "run-1", member_run_id: "member-1", seq: 3, title: "Inspect", started_at: "2026-07-19T00:00:02Z" }],
    team_run_events: [{ id: "event-1", team_run_id: "run-1", member_run_id: "member-1", seq: 2, occurred_at: "2026-07-19T00:00:02Z" }],
    live_member_activity: {
      "member-1": { team_run_id: "run-1", member_run_id: "member-1", provider: "kimi", kind: "thinking", preview: "not durable", revision: 1, emitted_at: "2026-07-19T00:00:02Z", expires_at: "2026-07-19T00:00:12Z" },
    },
  };
}

async function main() {
  console.log("== Dashboard native Agent Team selector checks ==");
  const selectors = await loadSelectors();
  const snapshot = fixture();

  const waves = selectors.selectOrderedWaves(snapshot, "mission-1");
  if (waves.map((wave) => wave.id).join(",") === "wave-1,wave-2") {
    ok("Mission waves use their explicit ordered index");
  } else {
    bad(`Mission wave order was ${waves.map((wave) => wave.id).join(",")}`);
  }

  const attempts = selectors.selectWaveAttempts(snapshot, "wave-1");
  if (attempts.map((run) => run.id).join(",") === "run-2,run-1") {
    ok("Wave attempts honor explicit executor_run_ids order over creation time");
  } else {
    bad(`Wave attempt order was ${attempts.map((run) => run.id).join(",")}`);
  }

  const member = selectors.selectMemberRunContext(snapshot, "member-1");
  if (member?.run.id === "run-1" && member.member.id === "member-1" && member.liveActivity?.preview === "not durable") {
    ok("MemberRun context retains run identity and exposes transient live preview separately");
  } else {
    bad("MemberRun context did not preserve its TeamRun relationship or live preview");
  }

  if (member?.assignments.length === 1 && member.assignments[0].relatedMessages.map((message) => message.id).join(",") === "message-assignment,message-progress") {
    ok("Assignment ownership and correlation lineage remain separate and chronologically stable");
  } else {
    bad("Assignment correlation did not produce the expected anchored lineage");
  }

  if (member?.needsYou.total === 3 && member.needsYou.approvals[0]?.id === "message-review") {
    ok("Needs-you rolls up review request, waiting member, and unacknowledged delivery");
  } else {
    bad(`Needs-you rollup was ${member?.needsYou.total}`);
  }

  const pressureMessage = selectors.selectMemberPressureMessage(
    [
      { id: "qa-blocker", kind: "blocker", from_member_id: "member-2", body: "QA is waiting for screenshots", created_at: "2026-07-19T00:00:03Z" },
      { id: "later-review", kind: "review_request", from_member_id: "member-1", to_member_ids: ["member-2"], body: "Review a different report", created_at: "2026-07-19T00:00:04Z" },
    ],
    { id: "member-2", status: "blocked" },
  );
  if (pressureMessage?.id === "qa-blocker") {
    ok("Needs-you explains a blocked member with their own blocker before a later review request");
  } else {
    bad(`Blocked-member pressure resolved to ${pressureMessage?.id ?? "none"}`);
  }

  const activity = member?.activity ?? [];
  if (activity.map((item) => item.id).join(",") === "message:message-assignment,event:event-1,action:action-1,message:message-progress,message:message-review") {
    ok("Stable activity has deterministic chronological tie-breaking and excludes thinking");
  } else {
    bad(`Stable activity was ${activity.map((item) => item.id).join(",")}`);
  }

  console.log(`\n   team selector checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`team selector check crashed: ${error.stack || error}`);
  process.exit(1);
});
