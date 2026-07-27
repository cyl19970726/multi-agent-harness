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
  const revisePlan = actions.updateWaveContext("wave/a", "# revised plan", "host");
  check(
    revisePlan.path === "/v1/waves/wave%2Fa/context"
      && revisePlan.body.context === "# revised plan"
      && revisePlan.body.updated_by === "host",
    "Update plan writes a Wave Markdown revision through the canonical action",
  );
  const answer = actions.sendTeamMessage("run/a", {
    fromMemberId: "host",
    toMemberIds: ["member/b"],
    kind: "answer",
    body: "Proceed",
    correlationId: "corr/c",
    causationId: "message/d",
    originWaveId: "wave/a",
  });
  check(
    answer.body.correlation_id === "corr/c"
      && answer.body.causation_id === "message/d"
      && answer.body.origin_wave_id === "wave/a",
    "Lead reply preserves Assignment correlation, causation, and Wave navigation context",
  );
  check(
    actions.startTeamRun("run/a").path === "/v1/team-runs/run%2Fa/start",
    "Start action targets the selected TeamRun",
  );
  const create = actions.createTeamRun({
    objective: "workspace contract",
    executionRoot: "/workspace/project",
    members: [{
      name: "fixer",
      role: "implementer",
      provider: "codex",
      executionMode: "codex_app_server",
      worktreeRef: "/workspace/external-worktree",
    }],
  });
  check(
    create.body.execution_root === "/workspace/project"
      && create.body.members[0].worktree_ref === "/workspace/external-worktree",
    "TeamRun create action preserves run execution root and member worktree override",
  );
  const resolve = actions.resolvePendingInteraction("run/a", "interaction/b", "q0_opt_0", "lead");
  check(
    resolve.path === "/v1/team-runs/run%2Fa/interactions/interaction%2Fb/resolve"
      && resolve.body.option_id === "q0_opt_0"
      && resolve.body.resolved_by === "lead",
    "Provider interaction resolution preserves the exact option and actor",
  );
  const steer = actions.steerTeamMember("run/a", "member/b", "focus on the gate");
  check(
    steer.path === "/v1/team-runs/run%2Fa/members/member%2Fb/steer"
      && steer.body.content === "focus on the gate",
    "Live steer targets one MemberRun and carries explicit input",
  );
  const interrupt = actions.interruptTeamMember("run/a", "member/b", "stop now");
  check(
    interrupt.path === "/v1/team-runs/run%2Fa/members/member%2Fb/interrupt"
      && interrupt.body.reason === "stop now",
    "Provider interruption targets one MemberRun with an auditable reason",
  );

  const [teamSource, missionSource, memberSource] = await Promise.all([
    readFile(join(dashboardRoot, "src/surfaces/TeamWarRoom.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/Missions.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/MemberRuns.tsx"), "utf8"),
  ]);
  check(
    teamSource.includes('delivery.member_id === "host" && delivery.status === "delivered"')
      && teamSource.includes("acknowledgeTeamMessage(run.id, message.id, \"host\")"),
    "Dashboard offers ACK only for delivered Host recipient rows",
  );
  check(
    teamSource.includes("Lead Inbox")
      && teamSource.includes("Every Member message addressed to the Host, with its Assignment work chain.")
      && teamSource.includes("<LeadInbox")
      && teamSource.includes("correlationId: replyAnchor?.correlation_id")
      && teamSource.includes("causationId: replyAnchor?.id")
      && teamSource.includes("Host coordination only · Member-originated messages come from their provider session."),
    "Team War Room exposes a Host-only Lead Inbox and correlation-anchored replies",
  );
  check(
    teamSource.includes('<option value="message">Message</option>')
      && teamSource.includes('<option value="assignment">Assignment</option>')
      && !teamSource.includes("Plan review")
      && !teamSource.includes("sendPlanMessage"),
    "Team War Room uses ordinary messages instead of a dedicated plan lifecycle",
  );
  check(
    teamSource.includes("KEY_ACTIVITY_MESSAGE_KINDS")
      && ["assignment", "plan_request", "plan_proposal", "plan_feedback", "plan_approval", "question", "answer", "handoff"]
        .every((kind) => teamSource.includes(`"${kind}"`)),
    "Team Activity keeps the complete plan, coordination, and handoff story visible by default",
  );
  check(
    teamSource.includes('starting ? "Starting…" : "Start attempt"'),
    "TeamRun start has an explicit pending state",
  );
  check(
    teamSource.includes("pendingInteractions")
      && teamSource.includes("resolvePendingInteraction(")
      && teamSource.includes('interaction.route === "human" ? "operator" : "host"')
      && teamSource.includes("Awaiting governed policy decision"),
    "Team Activity renders provider questions and approvals as actionable pressure",
  );
  check(
    missionSource.includes("readyToClose")
      && missionSource.includes("MissionCloseDialog")
      && missionSource.includes('const requiresRun = wave.executor_kind !== "host"'),
    "Mission closeout and executor-aware Wave Gate controls are rendered",
  );
  check(
    missionSource.includes("UpdatePlanDialog")
      && missionSource.includes("updateWaveContext(wave.id, context.trim(), \"host\")")
      && missionSource.includes("Save revision"),
    "Mission Canvas can revise current Wave Markdown without advancing the Wave",
  );
  check(
    missionSource.includes("linkedTeamSummaries.map")
      && missionSource.includes("Linked and reusable; no TeamRun has started yet."),
    "Mission Canvas renders every linked reusable Agent Team instead of collapsing the relation to one latest run",
  );
  check(
    memberSource.includes('messageKind === "steer"')
      && memberSource.includes('kind: messageKind === "steer" ? "control" : messageKind')
      && memberSource.includes("Injects only this explicit Steer")
      && memberSource.includes("queues control guidance for the next provider round")
      && memberSource.includes('execution_mode === "codex_app_server"')
      && memberSource.includes("steerTeamMember(")
      && memberSource.includes("interruptTeamMember(")
      && memberSource.includes("supports_cancel"),
    "Member Focus invokes same-turn steer only for an explicit Steer action and otherwise queues Host coordination",
  );
  check(
    teamSource.includes("missionId: navigationMission?.id")
      && teamSource.includes("waveId: navigationWave?.id")
      && memberSource.includes("missionId: navigationMissionId")
      && memberSource.includes("waveId: navigationWave?.id"),
    "Team and Member navigation preserve Mission/Wave context across deep links",
  );
  check(
    memberSource.includes("Current Assignment · Member Goal")
      && memberSource.includes("assignmentCompletionCriteria")
      && memberSource.includes("latestSteerSummary")
      && memberSource.includes("Host & peer threads")
      && memberSource.includes("Native subagent activity"),
    "Member Focus derives its Goal, collaboration threads, latest steer, peers, and native subagent entry",
  );
  check(
    !memberSource.includes("Execution plan")
      && !memberSource.includes("selectMemberPlanNegotiation")
      && memberSource.includes("Current Assignment · Member Goal"),
    "Member Focus keeps planning inside the Assignment conversation instead of a separate product panel",
  );

  console.log(`\n   operator control checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error);
  process.exit(1);
});
