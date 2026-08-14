#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { readWarRoomSource } from "./read-war-room-source.mjs";

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

function hasKey(value, key) {
  if (!value || typeof value !== "object") return false;
  if (Object.hasOwn(value, key)) return true;
  return Object.values(value).some((item) => hasKey(item, key));
}

function workHistory(operations, workId) {
  return operations.filter((operation) => operation.work?.id === workId);
}

function hasContinuousVersions(history) {
  return history.every((operation, index) => (
    operation.event.sequence === index + 1
    && operation.event.expected_version === index
    && operation.event.resulting_version === index + 1
    && operation.work.version === index + 1
    && operation.event.work_id === operation.work.id
  ));
}

function latestDeliveries(operations) {
  const byId = new Map();
  for (const operation of operations) {
    for (const delivery of operation.deliveries ?? []) byId.set(delivery.id, delivery);
    for (const update of operation.delivery_updates ?? []) {
      const delivery = byId.get(update.delivery_id);
      if (delivery) byId.set(update.delivery_id, { ...delivery, ...update, id: delivery.id });
    }
  }
  return [...byId.values()];
}

async function main() {
  const manifest = JSON.parse(await readFile(join(fixtureRoot, "fixture-manifest.json"), "utf8"));
  const repoRoot = resolve(dashboardRoot, "../..");
  const [agentTeamsHomeSource, actionsSource, typesSource, missionSource, warRoomSource, memberRunSource, memberNarrativeSource, shellSource, avatarSource, portraitsSource, captureSource, executionSource, activitySource, contextSource, cssSource] = await Promise.all([
    readFile(join(dashboardRoot, "src/surfaces/AgentTeamsHome.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/api/actions.ts"), "utf8"),
    readFile(join(dashboardRoot, "src/types.ts"), "utf8"),
    readFile(join(dashboardRoot, "src/surfaces/Missions.tsx"), "utf8"),
    readWarRoomSource(dashboardRoot),
    readFile(join(dashboardRoot, "src/surfaces/MemberRuns.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/member/MemberHistoryNarrative.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/app/WorkbenchShell.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/Avatar.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/identity/portraits.ts"), "utf8"),
    readFile(join(repoRoot, "scripts/capture-workbench-layout-v2.mjs"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/execution/ExecutionPrimitives.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/activity/ActivityStream.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/components/workbench/context/ContextRail.tsx"), "utf8"),
    readFile(join(dashboardRoot, "src/index.css"), "utf8"),
  ]);
  const [missions, waves, teams, runs, members, workOperations, messages, actions, events] = await Promise.all([
    rows("missions.jsonl"), rows("waves.jsonl"), rows("teams.jsonl"), rows("team_runs.jsonl"),
    rows("member_runs.jsonl"), rows("work_operations.jsonl"), rows("team_messages.jsonl"),
    rows("member_actions.jsonl"), rows("team_run_events.jsonl"),
  ]);

  const mission = missions.find((item) => item.id === manifest.mission_id);
  const linkedTeam = teams.find((item) => item.id === manifest.agent_team_id);
  const currentWave = waves.find((item) => item.id === manifest.wave_id);
  const priorWave = waves.find((item) => item.id === "wave-foundation");
  const currentRun = runs.find((item) => item.id === manifest.team_run_id);
  const currentMember = members.find((item) => item.id === manifest.member_run_id);

  check(
    mission?.status === "running"
      && mission.context.includes("# Ship the AgentFirm Host integration")
      && linkedTeam?.mission_id === mission.id,
    "Mission has durable Markdown context and is owned by one flat AgentTeam",
  );
  check(
    linkedTeam?.status === "active" && linkedTeam.name === "Platform Foundation Team",
    "Fixture contains the flat AgentTeam linked one-to-one with Mission",
  );
  check(
    priorWave?.status === "completed"
      && priorWave.gate_status === "accepted"
      && priorWave.accepted_run_id === null
      && priorWave.context.includes("Host judgment"),
    "Legacy Wave 1 remains readable as pre-cutover history",
  );
  check(
    currentWave?.status === "running"
      && currentWave.gate_status === "pending"
      && currentWave.executor_run_ids.length === 0
      && currentWave.revision === 2
      && currentWave.context.includes("| Member | Role | Responsibility | Deliverable |"),
    "Legacy Wave 2 remains readable as a pre-cutover Host-plan memo",
  );
  check(
    currentRun?.status === "running"
      && currentRun.agent_team_id === manifest.agent_team_id
      && currentRun.execution_node_id === linkedTeam.node_id
      && currentRun.project_binding_id
      && currentRun.member_run_ids.length === 4,
    "Current four-member TeamRun carries Team, Node, and project-binding identity",
  );
  check(
    shellSource.includes("Provider cwd boundary:")
      && shellSource.includes("Skill discovery boundary:")
      && shellSource.includes("Execution coordination:")
      && shellSource.includes("Project Binding does not own Mission, AgentTeam, or Workflow storage.")
      && shellSource.includes("selected.project_root")
      && shellSource.includes("selected.store_root"),
    "TopBar keeps Project Binding boundaries independent from Execution Space storage",
  );
  check(
    currentRun?.execution_root === "/workspace/multi-agent-harness"
      && runs.some((item) => !Object.hasOwn(item, "execution_root")),
    "TeamRun fixture distinguishes the selected execution root while retaining a legacy record without it",
  );
  check(
    currentMember?.status === "running"
      && currentMember.native_session?.availability === "available"
      && currentMember.native_session?.native_session_id,
    "Member Focus target is running and linked to provider-native runtime context",
  );
  check(
    currentMember?.provider_cwd_hint === currentMember?.provider_environment_observation?.cwd
      && !currentMember.provider_cwd_hint.startsWith(currentRun.execution_root)
      && currentMember.provider_environment_observation.git_head
      && currentMember.provider_environment_observation.git_branch
      && currentMember.provider_environment_observation.instruction_roots.length > 0
      && currentMember.provider_environment_observation.skill_roots.length > 0,
    "Member fixture distinguishes an out-of-project worktree override from TeamRun execution root and snapshots actual cwd plus Git/path-root context",
  );
  check(
    members.every((item) => item.provider_environment_observation
      && Array.isArray(item.provider_environment_observation.instruction_roots)
      && Array.isArray(item.provider_environment_observation.skill_roots)),
    "Every current MemberRun fixture snapshots non-secret discovered instruction and skill root paths",
  );
  check(members.some((item) => item.status === "blocked") && members.some((item) => item.status === "reviewing"), "Member states include blocked and reviewing pressure");
  const workIds = [...new Set(workOperations.map((operation) => operation.work?.id))];
  const leadHistory = workHistory(workOperations, "work-lead-integration");
  const researchHistory = workHistory(workOperations, "work-contract-review");
  const backendHistory = workHistory(workOperations, "work-team-console");
  const qaHistory = workHistory(workOperations, "work-responsive-qa");
  const openHistory = workHistory(workOperations, "work-accessibility-followup");
  const assignedHistory = workHistory(workOperations, "work-release-notes");
  check(workIds.length === 6 && workOperations.every((item) => item.work?.id && item.work?.completion_criteria_markdown), "Every execution lane is a durable Work with explicit completion criteria");
  check(
    [leadHistory, researchHistory, backendHistory, qaHistory, openHistory, assignedHistory].every(hasContinuousVersions),
    "Every Work operation history is a continuous append-only event/version chain",
  );
  check(
    JSON.stringify(leadHistory.map((operation) => operation.event.kind))
      === JSON.stringify(["created", "assigned", "started", "blocked", "resumed", "submitted", "accepted"])
      && researchHistory.at(-1)?.work.phase === "review"
      && researchHistory.at(-1)?.work.condition === "normal"
      && backendHistory.at(-1)?.work.phase === "active"
      && backendHistory.at(-1)?.work.condition === "normal"
      && qaHistory.some((operation) => operation.event.kind === "claimed")
      && qaHistory.at(-1)?.work.phase === "active"
      && qaHistory.at(-1)?.work.condition === "blocked"
      && !openHistory.at(-1)?.work.owner_member_id
      && assignedHistory.at(-1)?.work.phase === "open"
      && assignedHistory.at(-1)?.work.condition === "normal"
      && assignedHistory.at(-1)?.work.owner_member_id === "member-wave2-lead",
    "Fixture proves unassigned and assigned queues, team claim, block/resume, submit, and explicit Host acceptance while retaining live board pressure",
  );
  check(
    latestDeliveries(workOperations).length >= 4
      && latestDeliveries(workOperations).every((delivery) => delivery.team_run_id === manifest.team_run_id),
    "Work history rebuilds concrete same-TeamRun delivery projections",
  );
  check(
    messages.some((item) => item.id === "msg-qa-blocker")
      && messages.some((item) => item.id === "msg-review-request")
      && messages.filter((item) => ["msg-qa-blocker", "msg-review-request"].includes(item.id))
        .every((item) => item.kind === "message" && item.work_id),
    "Durable activity carries blocker and review requests as Work-linked canonical messages",
  );
  check(
    messages.filter((item) => item.id !== "msg-kickoff").every((item) => workIds.includes(item.work_id)),
    "Work-related conversation uses explicit work_id relations instead of assignment-message ownership",
  );
  check(
    ["msg-plan-request-research", "msg-plan-proposal-research-r1", "msg-plan-feedback-research", "msg-plan-approval-research"].every(
      (id) => messages.some((item) => item.id === id && item.kind === "message" && item.correlation_id === "corr-wave2-research"),
    ),
    "Fixture preserves the historical Member plan debate as canonical correlated messages",
  );
  check(messages.some((item) => item.deliveries?.some((delivery) => ["queued", "delivered"].includes(delivery.status))), "Fixture includes a concrete unacknowledged delivery");
  check(actions.some((item) => item.evidence_refs?.length) && events.length > 0, "Activity contains evidence-backed actions and folded events");
  check(!actions.some((item) => item.action_type === "thinking"), "No raw thinking is persisted in the fixture");
  const durableRows = [...runs, ...members, ...messages, ...actions, ...events];
  check(
    ["config_contents", "credentials", "provider_transcript", "tool_stream", "thinking"].every(
      (key) => !durableRows.some((item) => hasKey(item, key)),
    ),
    "Workspace fixtures contain no config contents, credentials, provider transcript, tool stream, or thinking fields",
  );
  check(runs.every((item) => !Object.hasOwn(item, "task_ids")), "Native Wave fixture contains only native execution fields");
  const duplicateWaveField = ["wave", "index"].join("_");
  check(runs.every((item) => !Object.hasOwn(item, duplicateWaveField)), "AgentTeamRun fixture does not duplicate the Wave index");
  check(!actionsSource.includes(duplicateWaveField) && !typesSource.includes(duplicateWaveField), "AgentTeamRun API and type contracts do not carry a duplicate Wave index");
  check(
    typesSource.includes("execution_root?: string | null")
      && typesSource.includes("provider_environment_observation?: MemberWorkspaceSnapshot | null")
      && typesSource.includes("instruction_roots: string[]")
      && typesSource.includes("skill_roots: string[]"),
    "Dashboard types mirror the backward-compatible TeamRun and MemberRun workspace wire contract",
  );
  check(
    agentTeamsHomeSource.includes("run.agent_team_id")
      && agentTeamsHomeSource.includes("Flat Mission-owned teams")
      && agentTeamsHomeSource.includes("Execution Nodes")
      && agentTeamsHomeSource.includes("daemon generation")
      && !agentTeamsHomeSource.includes("Legacy Wave")
      && !agentTeamsHomeSource.includes(`run.${duplicateWaveField}`),
    "Agent Team home exposes flat Mission-owned runs and NodeDaemon status without mixing in Legacy Wave history",
  );
  check(
    agentTeamsHomeSource.includes("Host Agent ·")
      && warRoomSource.includes("Host Agent ·")
      && warRoomSource.includes("Host coordination only")
      && warRoomSource.includes("Host Agent identity")
      && missionSource.includes("host_agent_id")
      && missionSource.includes("host {team.host_agent_id}"),
    "Agent Team surfaces identify the Host Agent without inventing a MemberRun",
  );
  check(
    captureSource.includes("HARNESS_CAPTURE_API_PROXY: apiBase")
      && captureSource.includes("api=${encodeURIComponent(webBase)}")
      && captureSource.includes('manifest.routes["agent-teams-home"]')
      && captureSource.includes('"mobile-390x844"')
      && captureSource.includes('"mobile-320x720"')
      && captureSource.includes("rootScrollWidth")
      && captureSource.includes('action: "works-default-and-detail"'),
    "Browser capture keeps API/SSE same-origin, covers Agent Team Works by default, and rejects horizontal clipping at 390px and 320px",
  );
  check(
    missionSource.includes('data-mission-scroll-owner="true"')
      && missionSource.includes("overflow-y-auto"),
    "Mission detail owns a reachable vertical scroll region",
  );
  check(
    avatarSource.includes("portraitFor") && avatarSource.includes("rounded-full")
      && portraitsSource.includes("defaultPortraits")
      && missionSource.includes('surface: "team", teamId: run.id, missionId: mission.id')
      && captureSource.includes('action: "mission-content-reachability"'),
    "Execution identities use project portraits and Mission run rows deep-link to the Team surface",
  );
  check(
    warRoomSource.includes('terminal ? "Unresolved history" : "QA approval required"'),
    "Terminal Team attempts distinguish unresolved history from active operator pressure",
  );
  check(
    missionSource.includes("Mission Log")
      && missionSource.includes("data-legacy-wave-history")
      && missionSource.includes("do not control Mission status, closeout, TeamRun creation, or navigation"),
    "Mission detail renders current Mission Log truth and isolates Legacy Wave history",
  );
  check(
    warRoomSource.includes("TeamConversationStream")
      && warRoomSource.includes("TeamMailboxStrip")
      && warRoomSource.includes("Team mailboxes")
      && warRoomSource.includes("Inbox and Outbox are live projections")
      && warRoomSource.includes("Review request")
      && warRoomSource.includes("showFullActivity")
      && warRoomSource.includes("Search team activity")
      && warRoomSource.includes("Markdown")
      && warRoomSource.includes("ConversationRoute")
      && warRoomSource.includes("recipientLabels"),
    "Agent Team V3 exposes mailbox projections, Markdown group activity, participant/type filters, and anchored review action",
  );
  check(
    warRoomSource.includes('data-testid="team-works-board"')
      && warRoomSource.includes('label: "Works"')
      && warRoomSource.includes('label: "Activity"')
      && warRoomSource.includes('label: "Members"')
      && warRoomSource.includes("Messages discuss Work; they never create ownership.")
      && warRoomSource.includes('aria-label="Related Work"')
      && warRoomSource.includes("Discuss Work"),
    "Team War Room defaults to a shared Works workspace and keeps explicitly related conversation one action away",
  );
  check(
    warRoomSource.includes('className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter Works by owner"')
      && warRoomSource.includes('className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Filter Works by attention state"')
      && warRoomSource.includes('className="max-w-36 truncate"'),
    "Works filters wrap without horizontal overflow",
  );
  // The five desktop lanes and the stacked mobile status list used to be
  // asserted as two literal Tailwind class strings on two sibling containers.
  // That pinned the exact markup AND encoded the duplicate-render defect: every
  // Work card existed twice in the DOM, once per container. The board now
  // renders one set of lane sections that reflows, so lane behaviour is
  // asserted against the real rendered DOM at each viewport in
  // tests/team-war-room-first-viewport-check.mjs instead of against source text.
  check(
    warRoomSource.includes('data-testid="team-works-lanes"')
      && warRoomSource.includes("lg:grid-cols-5")
      && !warRoomSource.includes('className="space-y-3 lg:hidden"'),
    "Works lanes render once and reflow rather than duplicating into a hidden mobile container",
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
    "Team activity uses distinct message, handoff, runtime, evidence, review, and decision glyphs",
  );
  check(
    warRoomSource.includes("<ConversationRoute item={item}")
      && warRoomSource.includes("recipientLabels")
      && warRoomSource.includes("messagePresentation")
      && warRoomSource.includes("sender portrait")
      && warRoomSource.includes('normalized === "message"')
      && warRoomSource.includes('normalized === "handoff"'),
    "Team conversation makes sender, recipient portraits, and message taxonomy explicit",
  );
  check(
    contextSource.includes("contextIconSurface")
      && contextSource.includes("rounded-full border"),
    "Context modules render semantic icon surfaces instead of uniform low-contrast glyphs",
  );
  check(
    memberRunSource.includes("<MemberHistoryNarrative")
      && memberRunSource.includes('<ContextRail label="Member context"')
      && memberRunSource.includes('message.kind === "handoff" ? "handoff"')
      && memberRunSource.includes('? "artifact" : "runtime"')
      && memberRunSource.includes('tone: "decision"')
      && memberRunSource.includes("transient: true")
      && memberRunSource.includes('source: "provider-native"')
      && memberRunSource.includes("nativeActivityState")
      && memberRunSource.includes("latest runtime/tool action")
      && memberNarrativeSource.includes("native session")
      && memberNarrativeSource.includes("Read-time editorial projection"),
    "MemberRun Focus joins visible provider-native activity with Harness coordination and labels provenance",
  );
  check(
    memberRunSource.includes('agent_member_ref?.kind === "agent_member"')
      && memberRunSource.includes("actor.record.agent_member_ref.id === context.member.agent_member_id")
      && !memberRunSource.includes("standing_assignment_conflicts")
      && !memberRunSource.includes("organizationLinkConflict"),
    "MemberRun Focus resolves Company membership only through the canonical AgentMember ActorRef",
  );
  check(
    warRoomSource.includes('label="Execution root"')
      && warRoomSource.includes('label="Worktree override"')
      && warRoomSource.includes('label="Actual cwd"')
      && memberRunSource.includes('label="Execution root"')
      && memberRunSource.includes('label="Worktree"')
      && memberRunSource.includes('label="Git HEAD"')
      && memberRunSource.includes('label="Git branch"')
      && memberRunSource.includes('label="Instruction roots"')
      && memberRunSource.includes('label="Skill roots"')
      && memberRunSource.includes("Not captured (legacy run)"),
    "P0 TeamRun and MemberRun surfaces visibly distinguish execution root, member override, actual cwd, Git facts, discovered roots, and legacy absence",
  );

  console.log(`\n   workbench visual fixture checks: ${pass} pass, ${fail} fail`);
  process.exit(fail === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exit(1);
});
