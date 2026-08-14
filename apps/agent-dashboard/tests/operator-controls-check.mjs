#!/usr/bin/env node

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { readWarRoomSource } from "./read-war-room-source.mjs";

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
  const replan = actions.appendMissionLog({
    missionId: "mission/a",
    kind: "replan",
    body: "# revised plan",
    actor: "operator",
  });
  check(
    replan.path === "/v1/missions/mission%2Fa/log"
      && replan.body.kind === "replan"
      && replan.body.body === "# revised plan"
      && replan.body.actor === "operator",
    "Plan revision appends a Mission Log entry through the canonical action (ADR 0051)",
  );
  check(
    typeof actions.createWave !== "function"
      && typeof actions.updateWaveContext !== "function"
      && typeof actions.advanceWave !== "function"
      && typeof actions.gateWave !== "function",
    "Retired Wave write descriptors are removed from the action seam",
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
      && answer.body.source_plan_ref === "wave/a",
    "Lead reply preserves conversation correlation, causation, optional Work link, and Wave navigation context",
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
      && create.body.members[0].provider_cwd_hint === "/workspace/external-worktree",
    "TeamRun create action preserves run execution root and member worktree override",
  );
  const messageCausedWork = actions.createTeamWork("run/a", {
    title: "Investigate request",
    completionCriteriaMarkdown: "Root cause is proven",
    causedByMessageId: "message/c",
  });
  check(
    messageCausedWork.body.caused_by_message_id === "message/c",
    "Work creation API can preserve an explicit source-message relation",
  );
  const resolve = actions.answerProviderMessage("run/a", "message/b", "q0_opt_0", "lead");
  check(
    resolve.path === "/v1/team-runs/run%2Fa/messages/message%2Fb/answer"
      && resolve.body.option_id === "q0_opt_0"
      && resolve.body.resolved_by === "lead",
    "Provider Message answer preserves the exact option and actor",
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
  const closeRuntime = actions.closeTeamMember("run/a", "member/b", "lane accepted");
  check(
    closeRuntime.path === "/v1/team-runs/run%2Fa/members/member%2Fb/close"
      && closeRuntime.body.reason === "lane accepted"
      && closeRuntime.body.requested_by === "host",
    "Host close releases one Member runtime through an explicit action",
  );
  const reopenRuntime = actions.reopenTeamMember("run/a", "member/b", "continue same lane");
  check(
    reopenRuntime.path === "/v1/team-runs/run%2Fa/members/member%2Fb/reopen"
      && reopenRuntime.body.reason === "continue same lane"
      && reopenRuntime.body.reopened_by === "host",
    "Host reopen resumes the same MemberRun through an explicit action",
  );

  const [teamSource, missionSource, memberSource, appSource, apiSource, providerSource] = await Promise.all([
    readWarRoomSource(dashboardRoot),
    readFile(join(dashboardRoot, "src/surfaces/Missions.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/MemberRuns.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/app/App.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/api.ts"), "utf8"),
    readFile(join(dashboardRoot, "src/lib/provider.ts"), "utf8"),
  ]);
  const [agentsSurfaceSource, teamsHomeSource] = await Promise.all([
    readFile(join(dashboardRoot, "src/surfaces/Surfaces.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/AgentTeamsHome.tsx"), "utf8"),
  ]);
  check(
    providerSource.includes("TEAM_MEMBER_PROVIDER_MODES")
      && providerSource.includes('"pi_rpc"')
      && agentsSurfaceSource.includes("TEAM_MEMBER_PROVIDER_MODES.map")
      && teamsHomeSource.includes("TEAM_MEMBER_PROVIDER_MODES")
      && teamSource.includes("TEAM_MEMBER_PROVIDER_MODES")
      && missionSource.includes("TEAM_MEMBER_PROVIDER_MODES"),
    "every creation form sources providers from the single TEAM_MEMBER_PROVIDER_MODES registry (incl. pi_rpc)",
  );
  check(
    teamSource.includes('aria-label="Response intent"'),
    "War Room composer exposes the explicit response-intent control",
  );
  check(
    memberSource.includes('memberStatus === "running"'),
    "native activity polling is gated on the member actually running",
  );
  check(
    missionSource.includes("updateMissionContext(")
      && missionSource.includes("Edit context")
      && missionSource.includes("createTeam(")
      && missionSource.includes("one flat AgentTeam"),
    "Mission canvas exposes durable context editing and one-Team creation without link/unlink",
  );
  check(
    teamSource.includes("fetchHostAttentions(")
      && teamSource.includes("acknowledgeHostAttention(")
      && teamSource.includes("Host attention"),
    "War Room surfaces HostAttention rows with a console Ack control",
  );
  check(
    memberSource.includes("resumeTeamMember(")
      && memberSource.includes("Resume session"),
    "closed resumable members use the standalone resume route behind a capability label",
  );
  check(
    teamSource.includes('delivery.member_id === "host" && delivery.status === "delivered"')
      && teamSource.includes("acknowledgeTeamMessage(run.id, message.id, \"host\")"),
    "Dashboard offers ACK only for delivered Host recipient rows",
  );
  check(
    teamSource.includes("Lead Inbox")
      && teamSource.includes("Every Member message addressed to the Host, preserving its conversation thread and delivery state.")
      && teamSource.includes("<LeadInbox")
      && teamSource.includes("correlationId: replyAnchor?.correlation_id")
      && teamSource.includes("causationId: replyAnchor?.id")
      && teamSource.includes("caused by ·")
      && teamSource.includes("reply to ${shortId(message.causation_id)}")
      && teamSource.includes("Host coordination only · Member-originated messages come from their provider session."),
    "Team War Room exposes a Host-only Lead Inbox and visible correlation/causation lineage",
  );
  check(
    teamSource.includes('<option value="message">Message</option>')
      && teamSource.includes('<option value="handoff">Handoff</option>')
      && teamSource.includes("TeamWorksBoard")
      && !teamSource.includes('<option value="assignment">Assignment</option>')
      && !teamSource.includes("Plan review")
      && !teamSource.includes("sendPlanMessage"),
    "Team War Room uses ordinary messages instead of a dedicated plan lifecycle",
  );
  check(
    teamSource.includes("const assignableMembers = members.filter(canMemberAcceptWork)")
      && teamSource.includes("assignableMembers.map((member)")
      && teamSource.includes("No active members available"),
    "Works owner controls only offer coordination-active member generations that can accept Work",
  );
  check(
    teamSource.includes('aria-label="Filter Works by owner"')
      && teamSource.includes('label="Unassigned"')
      && teamSource.includes("members.map((member)")
      && teamSource.includes('aria-label="Filter Works by attention state"')
      && teamSource.includes('label="Needs review"')
      && teamSource.includes('label="Blocked"')
      && teamSource.includes("selectFilteredTeamWorks"),
    "Works board filters by durable owner and attention state without replacing its five status lanes",
  );
  check(
    !teamSource.includes("causedByMessageId")
      && !teamSource.includes("Convert message to Work"),
    "Team UI does not invent an ambiguous message-to-Work conversion control before its interaction contract is designed",
  );
  check(
    teamSource.includes('data-testid="terminal-work-integrity-anomaly"')
      && teamSource.includes("Integrity anomaly · terminal TeamRun has unfinished Work")
      && teamSource.includes("needsYou.unfinishedWorks.length"),
    "Terminal TeamRuns visibly flag unfinished Work as an integrity anomaly instead of hiding pressure",
  );
  check(
    teamSource.includes('delivery.status === "provider_received" ? "complete" : "queued"')
      && teamSource.includes('delivery.status === "provider_received" ? "good" : "info"')
      && teamSource.includes("delivery.failure_reason")
      && memberSource.includes('delivery.status === "provider_received" ? "good" : "info"')
      && memberSource.includes("delivery.failure_reason"),
    "Team and Member activity treat provider receipt as delivery success and expose delivery failure reasons",
  );
  check(
    teamSource.includes("KEY_ACTIVITY_MESSAGE_KINDS")
      && ["plan_request", "plan_proposal", "plan_feedback", "plan_approval", "question", "answer", "handoff"]
        .every((kind) => teamSource.includes(`"${kind}"`)),
    "Team Activity keeps the complete plan, coordination, and handoff story visible by default",
  );
  check(
    teamSource.includes('starting ? "Starting…" : "Start attempt"'),
    "TeamRun start has an explicit pending state",
  );
  check(
    !teamSource.includes("pending" + "Interactions")
      && !teamSource.includes("resolve" + "Pending" + "Interaction("),
    "Team Activity has no second provider-question authority",
  );
  check(
    missionSource.includes("readyToClose")
      && missionSource.includes("MissionCloseDialog")
      && missionSource.includes("MissionLogDialog"),
    "Mission closeout and Mission Log entry controls are rendered",
  );
  check(
    missionSource.includes("appendMissionLog(")
      && missionSource.includes('setLogKind("replan")')
      && missionSource.includes("Append Host judgment")
      && missionSource.includes("Host decisions are recorded as Mission Log entries (ADR 0051)")
      && !missionSource.includes("updateWaveContext")
      && !missionSource.includes("advanceWave")
      && !missionSource.includes("gateWave")
      && !missionSource.includes("createWave"),
    "Mission Canvas revises the Host plan through Mission Log entries without advancing the Wave",
  );
  check(
    missionSource.includes("linkedTeamSummaries.map")
      && missionSource.includes("Mission-owned Team; no TeamRun has started yet."),
    "Mission Canvas renders its immutable Mission-owned AgentTeam without collapsing its run history",
  );
  check(
    memberSource.includes('composerMode === "steer"')
      && memberSource.includes("Injects only this explicit Steer")
      && memberSource.includes("liveSteerCapability(context.member)")
      && memberSource.includes("steerUnavailableReason={steerCapability.reason}")
      && providerSource.includes('mode !== "codex_app_server"')
      && providerSource.includes("Steer is available only while this Codex member has an active turn.")
      && memberSource.includes('disabled={!supportsLiveSteer}')
      && memberSource.includes('if (composerMode === "steer" && !canLiveSteer) return')
      && !memberSource.includes("queues control guidance for the next provider round")
      && memberSource.includes("steerTeamMember(")
      && memberSource.includes("interruptTeamMember(")
      && memberSource.includes("closeTeamMember(")
      && memberSource.includes("reopenTeamMember(")
      && memberSource.includes('context.member.coordination_status === "closed"')
      && memberSource.includes("supports_cancel")
      && memberSource.includes("interruptUnavailableReason(context.member)")
      && memberSource.includes("does not support provider-native cancellation"),
    "Member Focus exposes capability-gated Interrupt, Close, and same-MemberRun Reopen controls",
  );
  check(
    memberSource.includes("claudeDesktopSessionUri")
      && memberSource.includes("claude://resume?session=")
      && memberSource.includes("Open in Claude Desktop")
      && memberSource.includes("Simultaneous SDK and Desktop generation is not verified"),
    "Member Focus exposes explicit Claude Desktop import with a single-writer warning",
  );
  check(
    teamSource.includes("missionId: navigationMission?.id")
      && teamSource.includes("waveId: navigationWave?.id")
      && memberSource.includes("missionId: navigationMissionId")
      && memberSource.includes("waveId: navigationWave?.id"),
    "Team and Member navigation preserve Mission/Wave context across deep links",
  );
  check(
    memberSource.includes("Current Work · Member Goal")
      && memberSource.includes("context.currentWork")
      && memberSource.includes("context.queuedOwnedWorks")
      && memberSource.includes("context.eligibleReadyWorks")
      && memberSource.includes("work.artifact_refs")
      && memberSource.includes("work.check_refs")
      && memberSource.includes("latestSteerSummary")
      && memberSource.includes("Host & peer threads")
      && memberSource.includes("Native subagent activity")
      && memberSource.includes("reply linked")
      && memberSource.includes("caused by ${message.causation_id}"),
    "Member Focus derives its Goal, collaboration threads, latest steer, peers, native subagent entry, and reply lineage",
  );
  check(
    memberSource.includes('aria-label="Member next Work action"')
      && memberSource.includes("memberWorkNextAction")
      && memberSource.includes("This operator view does not impersonate the member.")
      && memberSource.includes("eligible ready Work from its own runtime")
      && !memberSource.includes("claimTeamWork("),
    "Member Focus explains owned/current/eligible next actions without giving the Operator member-only Work controls",
  );
  check(
    memberSource.includes('kind: "message"')
      && memberSource.includes("responseIntent,")
      && memberSource.includes("workId: currentWork?.id")
      && !memberSource.includes('<option value="question">')
      && !memberSource.includes('<option value="review_request">'),
    "Member Focus sends ordinary Work-linked messages with explicit response intent",
  );
  check(
    !memberSource.includes("Execution plan")
      && !memberSource.includes("selectMemberPlanNegotiation")
      && memberSource.includes("Current Work · Member Goal"),
    "Member Focus keeps planning in ordinary conversation while Work remains the responsibility source",
  );
  check(
    memberSource.includes('label="Ordinary mail"'),
    "Member Focus exposes the provider ordinary-message boundary",
  );
  check(
    appSource.includes("fetchTeamRunSnapshot")
      && apiSource.includes("/v1/team-runs/${encodeURIComponent(teamRunId)}/snapshot"),
    "Team deep links load through the bounded TeamRun snapshot route",
  );

  console.log(`\n   operator control checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error);
  process.exit(1);
});
