#!/usr/bin/env node
// Compatibility filename; current Mission console live-read consistency check.
//
// Proves the real SnapshotFrameBuffer used by App.tsx does not lose an SSE
// delta when a full snapshot/action response resolves later. This is purposely
// dependency-free (apart from the dashboard's TypeScript compiler) and imports
// the transpiled production api.ts rather than copying the merge algorithm.

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

async function loadApi() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "mission-wave-live-merge-"));
  try {
    const source = await readFile(join(here, "..", "src", "api.ts"), "utf8");
    const js = ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const output = join(directory, "api.mjs");
    await writeFile(output, js, "utf8");
    return await import(pathToFileURL(output).href);
  } finally {
    // ESM evaluation is complete before import() resolves, so cleanup cannot
    // remove a dependency under the loaded module.
    await rm(directory, { recursive: true, force: true });
  }
}

async function loadActions() {
  const { default: ts } = await import("typescript");
  const directory = await mkdtemp(join(tmpdir(), "mission-current-actions-"));
  try {
    const source = await readFile(join(here, "..", "src", "api", "actions.ts"), "utf8");
    const js = ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
    }).outputText;
    const output = join(directory, "actions.mjs");
    await writeFile(output, js, "utf8");
    return await import(pathToFileURL(output).href);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

function memberAction(id) {
  return {
    id,
    team_run_id: "run-1",
    member_run_id: "member-1",
    kind: "report",
    summary: `action ${id}`,
    created_at: "2026-07-19T00:00:00.000Z",
  };
}

async function main() {
  console.log("== Current Mission live snapshot merge checks (compatibility filename) ==");
  const {
    ProjectionInvalidationTracker,
    SnapshotFrameBuffer,
    matchesStreamProject,
    streamSelectionKey,
  } = await loadApi();
  const actions = await loadActions();
  // The Mission console surface is retired (DOC-107); its Wave-isolation
  // assertions retired with it. Wave deep-link hygiene is still enforced on
  // the retained navigation contract in selection.ts below.
  const selectionSource = await readFile(join(here, "..", "src", "app", "selection.ts"), "utf8");

  const createRun = actions.createTeamRun({
    objective: "current Mission runtime",
    agentTeamId: "team-1",
    members: [],
  });
  if (createRun.body.agent_team_id === "team-1"
      && !Object.hasOwn(createRun.body, "mission_id")
      && !Object.hasOwn(createRun.body, "wave_id")) {
    ok("TeamRun creation binds only agent_team_id; Mission is inherited from the Team");
  } else {
    bad("TeamRun creation still emitted retired Mission/Wave request fields");
  }

  const answer = actions.answerProviderMessage("run-1", "message-1", "allow_once");
  if (answer.body.option_id === "allow_once"
      && !Object.hasOwn(answer.body, "resolved_by")
      && !Object.hasOwn(answer.body, "source_plan_ref")) {
    ok("Provider answer sends only response content and leaves Host identity to transport authentication");
  } else {
    bad("Provider answer still emitted caller-selected identity or retired plan context");
  }

  if (selectionSource.includes('params.delete("wave")')
      && !selectionSource.includes("waveId?:")
      && !selectionSource.includes('params.get("wave")')) {
    ok("retired Wave deep links are discarded rather than restored into current navigation");
  } else {
    bad("current navigation still restores a Wave selection");
  }

  // Raw ledger rows are never browser truth. A crossing SSE member_action is
  // ignored; App schedules a fresh authoritative GET instead of replaying it.
  const buffer = new SnapshotFrameBuffer();
  const request = buffer.beginReadRequest();
  buffer.recordFrame({ kind: "member_action", action: memberAction("action-live") });
  const merged = buffer.resolveResponse(request, { member_actions: [] });
  if (merged?.member_actions?.length === 0) {
    ok("raw member_action is not folded into an authoritative response");
  } else {
    bad("raw member_action became browser truth");
  }

  // Full snapshot reads are serialized. A retry signal cannot supersede a slow
  // first response; App retains that signal in one dirty follow-up slot.
  const concurrent = new SnapshotFrameBuffer();
  const earlier = concurrent.beginReadRequest();
  const blockedRetry = concurrent.beginReadRequest();
  concurrent.recordFrame({ kind: "member_action", action: memberAction("action-newer") });
  const firstSuccess = concurrent.resolveResponse(earlier, { member_actions: [] });
  if (blockedRetry === null) {
    ok("a retry cannot overlap and invalidate the pending full snapshot");
  } else {
    bad("a retry was allowed to overlap the pending full snapshot");
  }
  if (firstSuccess?.member_actions?.length === 0) {
    ok("the first successful response commits without replaying a durable row");
  } else {
    bad("the first successful response lost commit eligibility or replayed a raw row");
  }

  // A mutation causally outranks reads. A poll started after an action POST is
  // suppressed, so it cannot return pre-commit state and supersede the action
  // response. A read that began before the mutation is invalidated too.
  const mutation = new SnapshotFrameBuffer();
  const preActionRead = mutation.beginReadRequest();
  const action = mutation.beginMutationRequest();
  const blockedPoll = mutation.beginReadRequest();
  const actionSnapshot = mutation.resolveResponse(action, {
    member_actions: [memberAction("action-response")],
  });
  mutation.finishMutation(action);
  const stalePreActionRead = mutation.resolveResponse(preActionRead, {
    member_actions: [memberAction("pre-commit-poll")],
  });
  if (blockedPoll === null && stalePreActionRead === null) {
    ok("polls during an action are suppressed and pre-action reads cannot commit");
  } else {
    bad("a poll/read was allowed to install pre-commit state during an action");
  }
  if (actionSnapshot?.member_actions?.[0]?.id === "action-response") {
    ok("the action response wins the mutation/read overlap");
  } else {
    bad("the action response did not win the mutation/read overlap");
  }

  // Thinking remains explicitly transient. The server snapshot can never carry
  // it; the browser retains only its current in-memory preview during the read.
  const activity = new SnapshotFrameBuffer();
  const initialRead = activity.beginReadRequest();
  activity.recordFrame({
    kind: "member_activity",
    activity: {
      member_run_id: "member-1",
      preview: "brief in-progress update",
      revision: 1,
      expires_at: "2026-07-19T00:00:10.000Z",
    },
  });
  const blockedActivityRetry = activity.beginReadRequest();
  const activityMerged = activity.resolveResponse(initialRead, {});
  if (activityMerged?.live_member_activity?.["member-1"]?.preview === "brief in-progress update") {
    ok("live member activity survives the serialized snapshot crossing");
  } else {
    bad("live-only member activity was dropped by the serialized read");
  }

  if (blockedActivityRetry === null) {
    ok("member activity does not weaken the one-full-read invariant");
  } else {
    bad("member activity allowed a second full read to overlap");
  }

  // Leaving the live connection clears the client-only registry before an
  // offline retry can fetch a fresh snapshot. No old thinking is replayed.
  activity.clearLiveMemberActivity();
  const offlineRetry = activity.beginReadRequest();
  const offlineRetryMerged = activity.resolveResponse(offlineRetry, {});
  if (!offlineRetryMerged?.live_member_activity) {
    ok("clearing the live connection prevents old thinking from returning on offline retry");
  } else {
    bad("offline retry replayed thinking that belonged to an old live connection");
  }

  // Project switches reset the buffer; coupled with App's captured-project
  // guard, a late A callback is rejected before it can reach B's buffer.
  const projects = new SnapshotFrameBuffer();
  const projectA = projects.beginReadRequest();
  projects.recordFrame({ kind: "member_action", action: memberAction("from-A") });
  projects.reset();
  const projectB = projects.beginReadRequest();
  const projectBMerged = projects.resolveResponse(projectB, { member_actions: [] });
  if (
    matchesStreamProject("project-b", "project-a") === false &&
    projectBMerged?.member_actions?.length === 0 &&
    projects.resolveResponse(projectA, {}) === null
  ) {
    ok("project reset and captured-project guard reject a late A frame after selecting B");
  } else {
    bad("a late A frame can still contaminate project B");
  }

  const invalidations = new ProjectionInvalidationTracker();
  invalidations.reset("serve-a");
  const scope = { executionSpaceId: "space-a", companyScopeId: "company-a" };
  const token = (revision, overrides = {}) => ({
    scope: "execution_space",
    scope_id: "space-a",
    ledger: "work_operations.jsonl",
    revision,
    reason: "append",
    stream_epoch: "serve-a",
    ...overrides,
  });
  const first = invalidations.observe(token(1), scope);
  const gap = invalidations.observe(token(3), scope);
  const duplicate = invalidations.observe(token(2), scope);
  const otherCompany = invalidations.observe(token(1, {
    scope: "company", scope_id: "company-b", ledger: "company_os_documents.jsonl",
  }), scope);
  if (
    first.kind === "refresh" && !first.gap
    && gap.kind === "refresh" && gap.gap
    && duplicate.kind === "ignore" && duplicate.reason === "duplicate"
    && otherCompany.kind === "ignore" && otherCompany.reason === "other_scope"
  ) {
    ok("invalidation revisions detect gaps, ignore duplicates, and enforce scope");
  } else {
    bad("invalidation revision/scope decision was not deterministic");
  }

  const restarted = invalidations.observe(token(1, { stream_epoch: "serve-b" }), scope);
  const malformed = invalidations.observe(null, scope);
  if (
    restarted.kind === "refresh" && !restarted.gap
    && malformed.kind === "refresh" && malformed.malformed
  ) {
    ok("serve epoch reset accepts low revisions and malformed invalidations fail stale");
  } else {
    bad("serve epoch reset or malformed invalidation recovery was rejected");
  }

  if (
    streamSelectionKey("space-a", "project-a", "company-a")
      !== streamSelectionKey("space-a", "project-a", "company-b")
  ) {
    ok("stream identity includes Company as well as Execution Space and Project");
  } else {
    bad("Company selection was omitted from stream identity");
  }

  console.log(`\n   mission-wave live merge checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`mission-wave-live-merge-check crashed: ${error.stack || error}`);
  process.exit(1);
});
