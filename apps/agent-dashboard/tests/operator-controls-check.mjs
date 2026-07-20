#!/usr/bin/env node

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const dashboardRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
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

async function loadActions() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "operator-controls-"));
  try {
    const source = await readFile(join(dashboardRoot, "src/api/actions.ts"), "utf8");
    const output = join(directory, "actions.mjs");
    await writeFile(output, ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
    }).outputText, "utf8");
    return await import(pathToFileURL(output).href);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

async function main() {
  console.log("== Dashboard operator control checks ==");
  const actions = await loadActions();
  const ack = actions.acknowledgeTeamMessage("run/a", "message/b", "host");
  check(
    ack.path === "/v1/team-runs/run%2Fa/messages/message%2Fb/ack"
      && ack.body.member_id === "host",
    "ACK action is TeamRun-scoped and recipient-explicit",
  );
  const close = actions.closeMission({ missionId: "mission/a", outcome: "done", completedBy: "lead" });
  check(
    close.path === "/v1/missions/mission%2Fa/close"
      && close.body.outcome === "done"
      && close.body.completed_by === "lead",
    "Mission closeout action carries durable outcome and actor",
  );
  check(
    actions.startTeamRun("run/a").path === "/v1/team-runs/run%2Fa/start",
    "Start action targets the selected TeamRun",
  );

  const [teamSource, missionSource] = await Promise.all([
    readFile(join(dashboardRoot, "src/surfaces/TeamRuns.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/Missions.tsx"), "utf8"),
  ]);
  check(
    teamSource.includes('delivery.member_id === "host" && delivery.status === "delivered"')
      && teamSource.includes("acknowledgeTeamMessage(run.id, message.id, \"host\")"),
    "Dashboard offers ACK only for delivered Host recipient rows",
  );
  check(
    teamSource.includes('starting ? "Starting…" : "Start orchestration"'),
    "TeamRun start has an explicit pending state",
  );
  check(
    missionSource.includes("readyToClose")
      && missionSource.includes("MissionCloseDialog")
      && missionSource.includes('const requiresRun = wave.executor_kind !== "host"'),
    "Mission closeout and executor-aware Wave Gate controls are rendered",
  );

  console.log(`\n   operator control checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error);
  process.exit(1);
});
