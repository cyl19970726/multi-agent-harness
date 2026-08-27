#!/usr/bin/env node
// Native Mission/AgentTeam selector checks. This imports the real
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

async function loadTypes() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "team-types-"));
  try {
    const source = await readFile(join(here, "..", "src", "types.ts"), "utf8");
    const output = ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const path = join(directory, "types.mjs");
    await writeFile(path, output, "utf8");
    return await import(pathToFileURL(path).href);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

function fixture() {
  return {
    missions: [{ id: "mission-1", title: "Build console", objective: "ship" }],
    legacy_waves: [
      { id: "wave-2", mission_id: "mission-1", index: 2, title: "Second", objective: "second", executor_kind: "agent_team" },
      { id: "wave-1", mission_id: "mission-1", index: 1, title: "First", objective: "first", executor_kind: "agent_team", executor_run_ids: ["run-2", "run-1"] },
    ],
    teams: [{ id: "team-1", mission_id: "mission-1", host_agent_id: "host", node_id: "node-1" }],
    team_runs: [
      { id: "run-1", agent_team_id: "team-1", member_run_ids: ["member-1"], created_at: "2026-07-19T00:00:02Z" },
      { id: "run-2", agent_team_id: "team-1", member_run_ids: ["member-2"], created_at: "2026-07-19T00:00:01Z" },
    ],
    member_runs: [
      { id: "member-1", team_run_id: "run-1", agent_member_id: "agent-a", name: "Worker", coordination_status: "active", status: "waiting" },
      { id: "member-2", team_run_id: "run-2", agent_member_id: "agent-b", name: "Critic", coordination_status: "active", status: "blocked" },
    ],
    team_messages: [
      { id: "message-progress", team_run_id: "run-1", sender_runtime_id: "member-1", kind: "progress", correlation_id: "corr-1", created_at: "2026-07-19T00:00:03Z" },
      { id: "message-briefing", team_run_id: "run-1", sender_runtime_id: "host", recipient_runtime_ids: ["member-1"], kind: "message", correlation_id: "corr-1", created_at: "2026-07-19T00:00:01Z", deliveries: [{ member_id: "member-1", status: "delivered" }] },
      { id: "message-review", team_run_id: "run-1", sender_runtime_id: "member-1", kind: "review_request", created_at: "2026-07-19T00:00:04Z" },
    ],
    works: [
      { id: "work-prerequisite", team_run_id: "run-1", title: "Baseline", context_markdown: "", completion_criteria_markdown: "Done", phase: "closed", condition: "normal", resolution: "accepted", owner_member_id: "agent-a", active_member_run_id: null, claim_mode: "host_assign", eligible_member_ids: [], prerequisite_work_ids: [], priority: "normal", artifact_refs: [], check_refs: [], version: 2, created_at: "2026-07-19T00:00:00Z", updated_at: "2026-07-19T00:00:01Z" },
      { id: "work-1", team_run_id: "run-1", title: "Implement selector", context_markdown: "Own the selector lane", completion_criteria_markdown: "Checks pass", phase: "active", condition: "normal", resolution: null, owner_member_id: "agent-a", active_member_run_id: null, claim_mode: "host_assign", eligible_member_ids: [], prerequisite_work_ids: [], priority: "normal", artifact_refs: [], check_refs: [], version: 2, created_at: "2026-07-19T00:00:01Z", updated_at: "2026-07-19T00:00:03Z" },
      { id: "work-blocked", team_run_id: "run-1", title: "Blocked lane", context_markdown: "", completion_criteria_markdown: "Unblocked", phase: "active", condition: "blocked", resolution: null, owner_member_id: "agent-a", active_member_run_id: null, claim_mode: "host_assign", eligible_member_ids: [], prerequisite_work_ids: [], priority: "urgent", artifact_refs: [], check_refs: [], version: 3, created_at: "2026-07-19T00:00:01Z", updated_at: "2026-07-19T00:00:04Z" },
      { id: "work-review", team_run_id: "run-1", title: "Review lane", context_markdown: "", completion_criteria_markdown: "Accepted", phase: "review", condition: "normal", resolution: null, owner_member_id: "agent-a", active_member_run_id: null, claim_mode: "host_assign", eligible_member_ids: [], prerequisite_work_ids: [], priority: "high", artifact_refs: [], check_refs: [], version: 3, created_at: "2026-07-19T00:00:01Z", updated_at: "2026-07-19T00:00:04Z" },
      { id: "work-open-owned", team_run_id: "run-1", title: "Queued lane", context_markdown: "", completion_criteria_markdown: "Started", phase: "open", condition: "normal", resolution: null, owner_member_id: "agent-a", active_member_run_id: null, claim_mode: "host_assign", eligible_member_ids: [], prerequisite_work_ids: [], priority: "normal", artifact_refs: [], check_refs: [], version: 1, created_at: "2026-07-19T00:00:01Z", updated_at: "2026-07-19T00:00:01Z" },
      { id: "work-ready-for-all", team_run_id: "run-1", title: "Ready pool", context_markdown: "", completion_criteria_markdown: "Claimed", phase: "open", condition: "normal", resolution: null, owner_member_id: null, active_member_run_id: null, claim_mode: "team_claim", eligible_member_ids: [], prerequisite_work_ids: ["work-prerequisite"], priority: "high", artifact_refs: [], check_refs: [], version: 1, created_at: "2026-07-19T00:00:01Z", updated_at: "2026-07-19T00:00:01Z" },
      { id: "work-not-ready", team_run_id: "run-1", title: "Waiting pool", context_markdown: "", completion_criteria_markdown: "Claimed", phase: "open", condition: "normal", resolution: null, owner_member_id: null, active_member_run_id: null, claim_mode: "team_claim", eligible_member_ids: [], prerequisite_work_ids: ["work-1"], priority: "normal", artifact_refs: [], check_refs: [], version: 1, created_at: "2026-07-19T00:00:01Z", updated_at: "2026-07-19T00:00:01Z" },
    ],
    work_events: [
      { id: "work-event-assigned", team_run_id: "run-1", work_id: "work-1", sequence: 1, kind: "assigned", expected_version: 0, resulting_version: 1, performed_by_actor: { kind: "host", id: "host" }, idempotency_key: "assign-1", created_at: "2026-07-19T00:00:00.500Z" },
      { id: "work-event-started", team_run_id: "run-1", work_id: "work-1", sequence: 4, kind: "started", expected_version: 1, resulting_version: 2, performed_by_actor: { kind: "member_run", id: "member-1" }, idempotency_key: "start-1", created_at: "2026-07-19T00:00:02Z" },
      { id: "work-event-created-hidden", team_run_id: "run-1", work_id: "work-1", sequence: 0, kind: "created", expected_version: 0, resulting_version: 1, performed_by_actor: { kind: "host", id: "host" }, idempotency_key: "create-1", created_at: "2026-07-19T00:00:00Z" },
    ],
    work_deliveries: [
      { id: "delivery-queued", work_event_id: "work-event-assigned", team_run_id: "run-1", work_id: "work-1", work_version: 1, recipient_member_run_id: "member-1", status: "queued", attempt: 0, updated_at: "2026-07-19T00:00:01Z" },
      { id: "delivery-failed", work_event_id: "work-event-assigned", team_run_id: "run-1", work_id: "work-1", work_version: 1, recipient_member_run_id: "member-1", status: "failed", attempt: 1, updated_at: "2026-07-19T00:00:05Z" },
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

  const runContext = selectors.selectTeamRunContext(snapshot, "run-1");
  if (runContext?.mission?.id === "mission-1"
      && runContext.attempts.map((run) => run.id).join(",") === "run-1"
      && !Object.hasOwn(runContext, "wave")) {
    ok("TeamRun context resolves Mission through AgentTeam and never consumes Legacy Wave history");
  } else {
    bad("TeamRun context did not preserve the current Mission/AgentTeam boundary");
  }

  const member = selectors.selectMemberRunContext(snapshot, "member-1");
  if (member?.run.id === "run-1" && member.member.id === "member-1" && member.liveActivity?.preview === "not durable") {
    ok("MemberRun context retains run identity and exposes transient live preview separately");
  } else {
    bad("MemberRun context did not preserve its TeamRun relationship or live preview");
  }

  if (member?.ownedWorks.length === 5
      && member.ownedWorks.some((work) => work.id === "work-prerequisite")
      && !member.activeWorks.some((work) => work.id === "work-prerequisite")) {
    ok("MemberRun ownership uses stable AgentMember identity across runtime generations");
  } else {
    bad(`MemberRun stable ownership was ${member?.ownedWorks.map((work) => work.id).join(",")}`);
  }

  if (member?.currentWork?.id === "work-1" && member.queuedOwnedWorks.map((work) => work.id).join(",") === "work-blocked,work-review,work-open-owned") {
    ok("Current Work prioritizes in-progress, then blocked/review, then assigned open Work");
  } else {
    bad(`Current/queued Work was ${member?.currentWork?.id ?? "none"} / ${member?.queuedOwnedWorks.map((work) => work.id).join(",")}`);
  }

  if (member?.eligibleReadyWorks.map((work) => work.id).join(",") === "work-ready-for-all") {
    ok("Empty eligibility means every active member, while unfinished prerequisites stay out of the pool");
  } else {
    bad(`Ready claim pool was ${member?.eligibleReadyWorks.map((work) => work.id).join(",")}`);
  }

  const unassignedWorks = selectors.selectFilteredTeamWorks(
    snapshot.works,
    snapshot.member_runs.filter((item) => item.team_run_id === "run-1"),
    "unassigned",
    "all",
  );
  const memberReviewWorks = selectors.selectFilteredTeamWorks(
    snapshot.works,
    snapshot.member_runs.filter((item) => item.team_run_id === "run-1"),
    "member:member-1",
    "review",
  );
  const owner = selectors.selectWorkOwnerMember(
    snapshot.works.find((work) => work.id === "work-1"),
    snapshot.member_runs.filter((item) => item.team_run_id === "run-1"),
  );
  if (unassignedWorks.map((work) => work.id).join(",") === "work-ready-for-all,work-not-ready"
      && memberReviewWorks.map((work) => work.id).join(",") === "work-review"
      && owner?.id === "member-1") {
    ok("Works board owner and attention filters preserve durable ownership and lane status");
  } else {
    bad(`Works filters were unassigned=${unassignedWorks.map((work) => work.id).join(",")} review=${memberReviewWorks.map((work) => work.id).join(",")} owner=${owner?.id ?? "none"}`);
  }

  const closedSnapshot = structuredClone(snapshot);
  closedSnapshot.member_runs.find((item) => item.id === "member-1").coordination_status = "closed";
  const closedMember = selectors.selectMemberRunContext(closedSnapshot, "member-1");
  if (closedMember?.eligibleReadyWorks.length === 0) {
    ok("Closed member generations do not appear eligible for the shared claim pool");
  } else {
    bad("Closed member generation still received eligible claim Work");
  }

  if (member?.needsYou.total === 3
      && member.needsYou.blockedWorks[0]?.id === "work-blocked"
      && member.needsYou.reviewWorks[0]?.id === "work-review"
      && member.needsYou.workDeliveryPressure[0]?.id === "delivery-failed"
      && member.needsYou.approvals.length === 0) {
    ok("Needs-you uses Work blocker/review truth plus failed Work delivery, not message wording");
  } else {
    bad(`Needs-you rollup was ${member?.needsYou.total}`);
  }

  const completedNeedsYou = selectors.selectTeamRunNeedsYou(
    snapshot.member_runs.filter((item) => item.team_run_id === "run-1"),
    snapshot.team_messages.filter((item) => item.team_run_id === "run-1"),
    "completed",
    snapshot.works.filter((item) => item.team_run_id === "run-1"),
    snapshot.work_deliveries.filter((item) => item.team_run_id === "run-1"),
  );
  if (completedNeedsYou.total === 7
      && completedNeedsYou.unfinishedWorks.length === 6
      && completedNeedsYou.blockedWorks[0]?.id === "work-blocked"
      && completedNeedsYou.reviewWorks[0]?.id === "work-review"
      && completedNeedsYou.workDeliveryPressure[0]?.id === "delivery-failed"
      && completedNeedsYou.unacknowledgedDeliveries.length === 0) {
    ok("Terminal attempts suppress historical message noise but expose contradictory unfinished Work truth");
  } else {
    bad(`Completed attempt still exposed ${completedNeedsYou.total} open signals`);
  }

  const stoppedSnapshot = structuredClone(snapshot);
  stoppedSnapshot.member_runs.find((item) => item.id === "member-1").status = "stopped";
  const stoppedMember = selectors.selectMemberRunContext(stoppedSnapshot, "member-1");
  if (stoppedMember?.eligibleReadyWorks.length === 0
      && selectors.canMemberAcceptWork(snapshot.member_runs[0])
      && !selectors.canMemberAcceptWork(stoppedSnapshot.member_runs[0])) {
    ok("Only coordination-active, non-stopped/failed/closed member generations can accept Work");
  } else {
    bad("Stopped member generation remained eligible for new Work");
  }

  const activity = member?.activity ?? [];
  if (activity.map((item) => item.id).join(",") === "work-event:work-event-assigned,message:message-briefing,event:event-1,action:action-1,work-event:work-event-started,message:message-progress,message:message-review,work-delivery:delivery-failed") {
    ok("Stable activity includes meaningful Work transitions and delivery failures, while excluding thinking and noisy creation");
  } else {
    bad(`Stable activity was ${activity.map((item) => item.id).join(",")}`);
  }

  const responsibilityActivity = selectors.selectStableTeamActivity({
    workEvents: [
      { id: "released", team_run_id: "run-1", work_id: "work-1", sequence: 1, kind: "released", expected_version: 1, resulting_version: 2, performed_by_actor: { kind: "member_run", id: "member-1" }, idempotency_key: "released", payload: null, created_at: "2026-07-19T00:00:01Z" },
      { id: "changes", team_run_id: "run-1", work_id: "work-1", sequence: 2, kind: "changes_requested", expected_version: 2, resulting_version: 3, performed_by_actor: { kind: "host", id: "host" }, idempotency_key: "changes", payload: null, created_at: "2026-07-19T00:00:02Z" },
      { id: "rebound", team_run_id: "run-1", work_id: "work-1", sequence: 3, kind: "rebound", expected_version: 3, resulting_version: 4, performed_by_actor: { kind: "host", id: "host" }, idempotency_key: "rebound", payload: null, created_at: "2026-07-19T00:00:03Z" },
      { id: "owner-update", team_run_id: "run-1", work_id: "work-1", sequence: 4, kind: "updated", expected_version: 4, resulting_version: 5, performed_by_actor: { kind: "host", id: "host" }, idempotency_key: "owner-update", payload: { patch: { owner_member_id: "agent-b" } }, created_at: "2026-07-19T00:00:04Z" },
      { id: "title-update", team_run_id: "run-1", work_id: "work-1", sequence: 5, kind: "updated", expected_version: 5, resulting_version: 6, performed_by_actor: { kind: "host", id: "host" }, idempotency_key: "title-update", payload: { patch: { title: "renamed" } }, created_at: "2026-07-19T00:00:05Z" },
    ],
  });
  if (responsibilityActivity.map((item) => item.id).join(",") === "work-event:released,work-event:changes,work-event:rebound,work-event:owner-update") {
    ok("Activity includes release, requested changes, rebound, and responsibility updates without title-edit noise");
  } else {
    bad(`Responsibility activity was ${responsibilityActivity.map((item) => item.id).join(",")}`);
  }

  const types = await loadTypes();
  const intent = types.effectiveTeamMessageResponseIntent;
  const peer = { kind: "message", sender: { kind: "member_run", id: "member-run-2" }, sender_runtime_id: "member-run-2" };
  const informationalAck = intent(peer) === "informational"
    && intent({ ...peer, response_intent: "informational" }) === "informational"
    && intent({ kind: "message", sender_runtime_id: "member-run-2" }) === "informational";
  const requiredByKind = intent({ kind: "provider_interaction_request" }) === "response_required"
    && intent({ kind: "control" }) === "response_required"
    && intent({ ...peer, kind: "provider_interaction_request" }) === "response_required"
    && intent({ kind: "provider_interaction_response" }) === "informational";
  // Sender-aware default: the coordination plane (Host, Operator via the
  // Dashboard, Service via routed inbox mail) wakes an idle member.
  const requiredBySender = intent({ kind: "message", sender_runtime_id: "host" }) === "response_required"
    && intent({ kind: "message", sender: { kind: "host", id: "host" }, sender_runtime_id: "host" }) === "response_required"
    && intent({ kind: "message", sender: { kind: "operator", id: "op-1" }, sender_runtime_id: "operator:op-1" }) === "response_required"
    && intent({ kind: "message", sender: { kind: "service", id: "svc-1" }, sender_runtime_id: "service:svc-1" }) === "response_required";
  const explicitWins = intent({ ...peer, response_intent: "response_required" }) === "response_required"
    && intent({ kind: "message", sender_runtime_id: "host", response_intent: "informational" }) === "informational";
  if (informationalAck && requiredByKind && requiredBySender && explicitWins) {
    ok("Response intent distinguishes informational delivery from response-required (kind + sender default, explicit override both ways)");
  } else {
    bad(`Response intent resolution was ack=${informationalAck} kind=${requiredByKind} sender=${requiredBySender} explicit=${explicitWins}`);
  }

  console.log(`\n   team selector checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`team selector check crashed: ${error.stack || error}`);
  process.exit(1);
});
