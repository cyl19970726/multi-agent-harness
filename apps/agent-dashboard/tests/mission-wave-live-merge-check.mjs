#!/usr/bin/env node
// Mission/Wave console live-read consistency check.
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
  console.log("== Mission/Wave live snapshot merge checks ==");
  const { SnapshotFrameBuffer, matchesStreamProject } = await loadApi();

  // The exact race: /v1/snapshot starts, an SSE member_action arrives, then
  // the older snapshot returns without that action. The client must replay it.
  const buffer = new SnapshotFrameBuffer();
  const request = buffer.beginReadRequest();
  buffer.recordFrame({ kind: "member_action", action: memberAction("action-live") });
  const merged = buffer.resolveResponse(request, { member_actions: [] });
  if (merged?.member_actions?.some((action) => action.id === "action-live")) {
    ok("member_action received during a snapshot request survives its response");
  } else {
    bad("member_action received during a snapshot request was dropped");
  }

  // Latest-wins applies among overlapping reads only. A late earlier response
  // cannot overwrite the newer read, while the new response still replays its
  // in-flight SSE frame.
  const concurrent = new SnapshotFrameBuffer();
  const earlier = concurrent.beginReadRequest();
  const newer = concurrent.beginReadRequest();
  concurrent.recordFrame({ kind: "member_action", action: memberAction("action-newer") });
  const stale = concurrent.resolveResponse(earlier, { member_actions: [memberAction("stale")] });
  const current = concurrent.resolveResponse(newer, { member_actions: [] });
  if (stale === null) {
    ok("an older read response is ignored after a newer read begins");
  } else {
    bad("an older read response was allowed to clobber the newer request");
  }
  if (current?.member_actions?.map((action) => action.id).join(",") === "action-newer") {
    ok("the newest response replays only its in-flight member_action delta");
  } else {
    bad("the newest response did not preserve its in-flight member_action delta");
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

  // Thinking remains explicitly transient. It may arrive before a newer
  // overlapping read, but the server snapshot can never carry it; the browser
  // retains only its current in-memory preview during the crossing.
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
  const activityRequest = activity.beginReadRequest();
  const activityMerged = activity.resolveResponse(activityRequest, {});
  if (activityMerged?.live_member_activity?.["member-1"]?.preview === "brief in-progress update") {
    ok("live member activity before an overlapping read survives the snapshot crossing");
  } else {
    bad("live-only member activity was dropped by the overlapping read");
  }

  if (activity.resolveResponse(initialRead, {}) === null) {
    ok("the older overlapping read remains stale after activity is preserved");
  } else {
    bad("the older overlapping read unexpectedly committed");
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

  console.log(`\n   mission-wave live merge checks: ${passed} pass, ${failed} fail`);
  process.exit(failed === 0 ? 0 : 1);
}

main().catch((error) => {
  console.error(`mission-wave-live-merge-check crashed: ${error.stack || error}`);
  process.exit(1);
});
