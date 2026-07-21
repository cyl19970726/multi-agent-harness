#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const fixtureRoot = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../fixtures/workbench-layout-v2-native-v1",
);
const dashboardRoot = resolve(fixtureRoot, "../..");

let pass = 0;
let fail = 0;
function check(condition, message) {
  if (condition) {
    console.log(`  PASS  ${message}`);
    pass += 1;
  } else {
    console.log(`  FAIL  ${message}`);
    fail += 1;
  }
}

async function rows(name) {
  const text = await readFile(join(fixtureRoot, name), "utf8");
  return text.split(/\r?\n/).filter(Boolean).map((line) => JSON.parse(line));
}

async function main() {
  const manifest = JSON.parse(await readFile(join(fixtureRoot, "fixture-manifest.json"), "utf8"));
  const repoRoot = resolve(dashboardRoot, "../..");
  const [agentTeamsHomeSource, actionsSource, typesSource, missionSource, warRoomSource, memberRunSource, avatarSource, captureSource, executionSource, activitySource, contextSource, cssSource] = await Promise.all([
    readFile(join(dashboardRoot, "src/surfaces/AgentTeamsHome.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/api/actions.ts"), "utf8"),
    readFile(join(dashboardRoot, "src/types.ts"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/Missions.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/TeamWarRoom.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/MemberRuns.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/Avatar.tsx"), "utf8"),
    readFile(join(repoRoot, "scripts/capture-workbench-layout-v2.mjs"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/execution/ExecutionPrimitives.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/activity/ActivityStream.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/context/ContextRail.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/index.css"), "utf8"),
  ]);
  const [missions, waves, runs, members, messages, actions, events] = await Promise.all([
    rows("missions.jsonl"), rows("waves.jsonl"), rows("team_runs.jsonl"),
    rows("member_runs.jsonl"), rows("team_messages.jsonl"),
    rows("member_actions.jsonl"), rows("team_run_events.jsonl"),
  ]);

  const mission = missions.find((item) => item.id === manifest.mission_id);
  const currentWave = waves.find((item) => item.id === manifest.wave_id);
  const priorWave = waves.find((item) => item.id === "wave-foundation");
  const currentRun = runs.find((item) => item.id === manifest.team_run_id);
  const currentMember = members.find((item) => item.id === manifest.member_run_id);

  check(mission?.status === "running" && mission.wave_ids.length === 3, "Mission has three explicit ordered Waves");
  check(priorWave?.status === "completed" && priorWave.gate_status === "accepted" && priorWave.accepted_run_id === "teamrun-wave1-accepted", "Wave 1 is completed and accepted against a completed attempt");
  check(currentWave?.status === "running" && currentWave.gate_status === "pending" && currentWave.executor_run_ids.length === 2, "Wave 2 is running on retry Attempt 2 with a separate pending gate");
  check(currentRun?.status === "running" && currentRun.previous_run_id === "teamrun-wave2-attempt1" && currentRun.member_run_ids.length === 4, "Current TeamRun is a four-member retry attempt");
  check(currentMember?.status === "running" && currentMember.provider_session_id, "Member Focus target is running and linked to observable runtime context");
  check(members.some((item) => item.status === "blocked") && members.some((item) => item.status === "reviewing"), "Member states include blocked and reviewing pressure");
  check(messages.filter((item) => item.kind === "assignment").every((item) => item.correlation_id), "Every assignment has a stable correlation anchor");
  check(messages.some((item) => item.kind === "blocker") && messages.some((item) => item.kind === "review_request"), "Durable activity contains blocker and review request signals");
  check(messages.some((item) => item.deliveries?.some((delivery) => ["queued", "delivered"].includes(delivery.status))), "Fixture includes a concrete unacknowledged delivery");
  check(actions.some((item) => item.evidence_refs?.length) && events.length > 0, "Activity contains evidence-backed actions and folded events");
  check(!actions.some((item) => item.action_type === "thinking"), "No raw thinking is persisted in the fixture");
  check(runs.every((item) => !Object.hasOwn(item, "task_ids")), "Native Wave fixture contains only native execution fields");
  const duplicateWaveField = ["wave", "index"].join("_");
  check(runs.every((item) => !Object.hasOwn(item, duplicateWaveField)), "AgentTeamRun fixture does not duplicate the Wave index");
  check(!actionsSource.includes(duplicateWaveField) && !typesSource.includes(duplicateWaveField), "AgentTeamRun API and type contracts do not carry a duplicate Wave index");
  check(agentTeamsHomeSource.includes("waves.get(run.wave_id)") && agentTeamsHomeSource.includes("wave.index") && !agentTeamsHomeSource.includes(`run.${duplicateWaveField}`), "Agent Team home joins native attempts to Wave labels through wave_id");
  check(
    captureSource.includes("HARNESS_CAPTURE_API_PROXY: apiBase")
      && captureSource.includes("api=${encodeURIComponent(webBase)}")
      && captureSource.includes('manifest.routes["agent-teams-home"]'),
    "Browser capture keeps API and SSE reads on the Vite same-origin proxy and covers the native Agent Team home",
  );
  check(
    missionSource.includes("flex flex-col items-stretch")
      && missionSource.includes("flex w-full flex-wrap items-center"),
    "Mission header gives its title and actions separate mobile layout rows",
  );
  check(
    avatarSource.includes("portraitFor") && avatarSource.includes("rounded-full"),
    "Execution identities reuse the shared portrait system with a text-backed fallback",
  );
  check(
    warRoomSource.includes('terminal ? "Unresolved history" : "QA approval required"'),
    "Terminal Team attempts distinguish unresolved history from active operator pressure",
  );
  check(
    missionSource.includes("WaveJourneyCompact")
      && missionSource.includes("LiveTrace")
      && missionSource.includes("DecisionAnchor"),
    "Mission V3 renders one continuous Wave journey with live and decision anchors",
  );
  check(
    warRoomSource.includes('variant="timeline"')
      && warRoomSource.includes("Team presence")
      && warRoomSource.includes("Review request")
      && warRoomSource.includes("showFullActivity")
      && warRoomSource.includes('prominence === "primary"')
      && activitySource.includes("activity-timeline-row")
      && cssSource.includes(".activity-timeline::before"),
    "Agent Team V3 exposes a presence rail, timestamped semantic timeline, key/full projection, and anchored review action",
  );
  check(
    executionSource.includes('role="progressbar"')
      && executionSource.includes("motion-reduce")
      && cssSource.includes("@media (prefers-reduced-motion: reduce)"),
    "Execution primitives expose semantic readiness and reduced-motion-safe transitions",
  );
  check(
    activitySource.includes('variant?: "rows" | "spine"')
      && contextSource.includes("quiet?: boolean"),
    "Shared activity and context primitives add V3 treatments without changing their defaults",
  );
  check(
    activitySource.includes("SendHorizontal")
      && activitySource.includes("ArrowRightLeft")
      && activitySource.includes("activityIconSurface")
      && warRoomSource.includes("teamMessageGlyph"),
    "Team activity uses distinct assignment, handoff, runtime, evidence, review, and decision glyphs",
  );
  check(
    contextSource.includes("contextIconSurface")
      && contextSource.includes("rounded-full border"),
    "Context modules render semantic icon surfaces instead of uniform low-contrast glyphs",
  );
  check(
    memberRunSource.includes('variant="timeline"')
      && memberRunSource.includes('<ContextRail quiet label="Member context">')
      && memberRunSource.includes('glyph: assignment ? "assignment"')
      && memberRunSource.includes('? "artifact" : "runtime"')
      && memberRunSource.includes('tone: "decision"')
      && memberRunSource.includes("transient: true"),
    "MemberRun Focus uses the V3 semantic timeline, quiet context rail, and transient-only activity path",
  );

  console.log(`\n   workbench visual fixture checks: ${pass} pass, ${fail} fail`);
  process.exit(fail === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
