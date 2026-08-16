/**
 * Lifecycle tests for the persistent member.
 *
 * The first test is the one that matters: it is the deterministic form of
 * ADR 0037 acceptance item 6 ("the Host revises or advances while another
 * member continues on the same MemberRun and native session"), which today has
 * no coverage anywhere in the repo. It fails against the `-p`-per-delivery
 * design by construction, because there the member is gone before the second
 * message arrives.
 *
 *   node --test apps/claude-member-runner/test/
 */

import { test } from "node:test";
import assert from "node:assert/strict";

import { createMemberRunner } from "../src/member-runner.mjs";
import { Mailbox } from "../src/mailbox.mjs";
import { isInside, ownedPathsObserver } from "../src/gates.mjs";
import { createFakeSdk } from "./fake-sdk.mjs";

const baseConfig = {
  teamRunId: "trun-1",
  memberRunId: "mrun-runtime",
  memberName: "RuntimeBuilder",
  roleLabel: "Runtime owner",
  cwd: "/tmp/project",
  allowedTools: ["Read", "Edit", "Bash"],
  model: "claude-sonnet-4-5",
  effort: "high",
};

function harness(config = {}) {
  const events = [];
  const sdk = createFakeSdk();
  const runner = createMemberRunner({
    sdk,
    config: { ...baseConfig, ...config },
    emit: (event, data) => events.push({ event, data }),
  });
  return { sdk, runner, events, of: (name) => events.filter((e) => e.event === name) };
}

const settled = () => new Promise((r) => setTimeout(r, 0));

test("member survives an empty mailbox and consumes a later message", async () => {
  const { runner, of } = harness();
  const done = runner.start();

  runner.deliver({ id: "w1", kind: "work", sender_runtime_id: "host", body: "build it" });
  await settled();

  // The queue is now empty. Under the current batch design the member would
  // have terminated here.
  assert.equal(runner.mailbox.pending, 0);
  assert.equal(runner.mailbox.closed, false, "empty queue must not end the member");
  assert.equal(of("member_closed").length, 0);

  // A message arriving after the lull still has a recipient.
  runner.deliver({ id: "m2", kind: "message", sender_runtime_id: "host", body: "also do this" });
  await settled();

  assert.equal(of("turn_complete").length, 2, "both messages produced a turn");
  assert.deepEqual(
    of("turn_complete").map((event) => event.data.triggerMessageId),
    ["w1", "m2"],
    "each provider turn identifies the exact durable input it consumed",
  );

  runner.close("host_accepted_work");
  await done;
  assert.equal(of("member_closed")[0].data.reason, "host_accepted_work");
});

test("a provider API failure is not reported as an ordinary successful turn", async () => {
  // Live probe (issue #293): a 403 round arrives with subtype "success" and
  // is_error=true. The runner must forward the error fields so Harness can
  // record a provider-error round instead of a fake completion.
  const events = [];
  const sdk = createFakeSdk({ apiErrorStatus: 403 });
  const runner = createMemberRunner({
    sdk,
    config: { ...baseConfig },
    emit: (event, data) => events.push({ event, data }),
  });
  const done = runner.start();

  runner.deliver({ id: "w1", kind: "work", sender_runtime_id: "host", body: "build it" });
  await settled();

  const turns = events.filter((e) => e.event === "turn_complete");
  assert.equal(turns.length, 1);
  assert.equal(turns[0].data.subtype, "success", "the SDK keeps subtype success even on error");
  assert.equal(turns[0].data.isError, true, "the honest error flag must survive");
  assert.equal(turns[0].data.terminalReason, "api_error");
  assert.equal(turns[0].data.apiErrorStatus, 403);

  runner.close("test_done");
  await done;

  // The SDK re-throws the last error result when the input stream ends; a
  // clean Host close must still produce member_closed, not a runner crash.
  const closed = events.filter((e) => e.event === "member_closed");
  assert.equal(closed.length, 1, "member_closed must survive the SDK's end-of-stream error re-throw");
  assert.equal(closed[0].data.reason, "test_done");
  assert.equal(
    events.filter((e) => e.event === "query_ended_with_provider_error").length,
    1,
    "the end-of-stream provider error is reported as an observation, not a crash",
  );
});

test("nullable Rust tool lists are omitted from Agent SDK options", async () => {
  const { runner, sdk } = harness({ allowedTools: null, disallowedTools: null });
  const done = runner.start();
  runner.deliver({ id: "w-null-tools", kind: "work", sender_runtime_id: "host", body: "reply" });
  await settled();

  assert.equal(sdk.lastOptions.allowedTools, undefined);
  assert.equal(sdk.lastOptions.disallowedTools, undefined);

  runner.close("test_done");
  await done;
});

test("only the Host ends the member, and the reason is recorded", async () => {
  const { runner, of } = harness();
  const done = runner.start();
  runner.deliver({ id: "w1", kind: "work", sender_runtime_id: "host", body: "x" });
  await settled();

  runner.close("run_torn_down");
  await done;

  const closed = of("member_closed");
  assert.equal(closed.length, 1);
  assert.equal(closed[0].data.reason, "run_torn_down");
  assert.deepEqual(closed[0].data.undelivered, []);
});

test("native session is bound once and registered under the TeamRun tag", async () => {
  const { runner, sdk, of } = harness();
  const done = runner.start();
  runner.deliver({ id: "w1", kind: "work", sender_runtime_id: "host", body: "x" });
  await settled();
  runner.deliver({ id: "m2", kind: "message", sender_runtime_id: "host", body: "y" });
  await settled();
  runner.close("done");
  await done;

  assert.equal(of("session_bound").length, 1, "bind exactly once");
  assert.equal(
    of("session_bound")[0].data.providerVersion,
    "2.1.220-test",
    "the execution-mode version comes from the SDK system/init event",
  );
  assert.equal(of("session_bound")[0].data.model, "claude-sonnet-4-5");
  assert.equal(of("session_bound")[0].data.effort, "high");
  assert.equal(sdk.lastOptions.model, "claude-sonnet-4-5");
  assert.equal(sdk.lastOptions.effort, "high");
  assert.equal(sdk.calls.tagSession.length, 1);
  assert.equal(sdk.calls.tagSession[0].tag, "trun-1:mrun-runtime");
  assert.equal(sdk.calls.renameSession[0].title, "RuntimeBuilder · Runtime owner");
});

test("undelivered messages are reported rather than silently dropped", async () => {
  const { runner, of } = harness();
  const done = runner.start();
  await settled();

  // Two arrive while the member is mid-turn on nothing; close before drain.
  runner.mailbox.push({ id: "m9", kind: "message", sender_runtime_id: "peer", body: "late" });
  runner.close("closed_by_host");
  await done;

  const closed = of("member_closed")[0].data;
  assert.ok(
    closed.undelivered.includes("m9") || of("turn_complete").length === 1,
    "a message is either consumed or reported as undelivered — never lost",
  );
});

test("a cross-lane write is reported and still allowed to proceed", async () => {
  // Deliberately not a deny. A member holding a shell writes wherever it likes,
  // so blocking Write would only move the same edit into `echo >` and hide it
  // from the Host. Reporting keeps it visible at review time.
  const seen = [];
  const observe = ownedPathsObserver({
    ownedPaths: ["crates/harness-cli"],
    cwd: "/repo",
    onCrossLane: (v) => seen.push(v),
  });

  const mine = await observe({
    hook_event_name: "PreToolUse",
    tool_name: "Edit",
    tool_input: { file_path: "/repo/crates/harness-cli/src/main.rs" },
  });
  assert.deepEqual(mine, {});
  assert.equal(seen.length, 0, "in-lane writes are not reported");

  const theirs = await observe({
    hook_event_name: "PreToolUse",
    tool_name: "Edit",
    tool_input: { file_path: "/repo/apps/agent-dashboard/src/App.tsx" },
  });
  assert.deepEqual(theirs, {}, "must not block");
  assert.equal(seen.length, 1, "but must be visible to the Host");
  assert.match(seen[0].path, /agent-dashboard/);
});

test("owned-paths containment is not fooled by traversal", () => {
  assert.equal(isInside("/repo/crates", "/repo/crates/a.rs"), true);
  assert.equal(isInside("/repo/crates", "/repo/crates/../apps/b.tsx"), false);
  assert.equal(isInside("/repo/crates", "/repo/crates-other/c.rs"), false);
});

test("mailbox rejects delivery after close instead of dropping it", () => {
  const mailbox = new Mailbox();
  mailbox.close("done");
  assert.throws(() => mailbox.push({ id: "m1" }), /closed/);
});

test("permission prompts default to off, because nobody can answer them", async () => {
  const { runner, sdk } = harness({ ownedPaths: ["crates"], allowedTools: ["Read"] });
  const done = runner.start();
  await settled();
  assert.equal(sdk.lastOptions.permissionMode, "bypassPermissions");
  assert.ok(sdk.lastOptions.hooks?.PreToolUse?.length > 0, "observers stay wired");
  runner.close("done");
  await done;
});

test("the provider subprocess inherits Harness coordination identity", async () => {
  const previous = {
    project: process.env.HARNESS_PROJECT,
    team: process.env.HARNESS_TEAM_RUN_ID,
    member: process.env.HARNESS_MEMBER_RUN_ID,
    work: process.env.HARNESS_WORK_ID,
    workVersion: process.env.HARNESS_WORK_VERSION,
  };
  Object.assign(process.env, {
    HARNESS_PROJECT: "project-live",
    HARNESS_TEAM_RUN_ID: "trun-live",
    HARNESS_MEMBER_RUN_ID: "mrun-live",
    HARNESS_WORK_ID: "work-live",
    HARNESS_WORK_VERSION: "7",
  });
  try {
    const { runner, sdk } = harness();
    const done = runner.start();
    await settled();
    assert.equal(sdk.lastOptions.env.HARNESS_PROJECT, "project-live");
    assert.equal(sdk.lastOptions.env.HARNESS_TEAM_RUN_ID, "trun-live");
    assert.equal(sdk.lastOptions.env.HARNESS_MEMBER_RUN_ID, "mrun-live");
    assert.equal(sdk.lastOptions.env.HARNESS_WORK_ID, "work-live");
    assert.equal(sdk.lastOptions.env.HARNESS_WORK_VERSION, "7");
    assert.equal(sdk.lastOptions.env.PATH, process.env.PATH);
    runner.close("done");
    await done;
  } finally {
    for (const [key, value] of Object.entries({
      HARNESS_PROJECT: previous.project,
      HARNESS_TEAM_RUN_ID: previous.team,
      HARNESS_MEMBER_RUN_ID: previous.member,
      HARNESS_WORK_ID: previous.work,
      HARNESS_WORK_VERSION: previous.workVersion,
    })) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
});



test("an explicit permission mode is not overridden", async () => {
  const { runner, sdk } = harness({ permissionMode: "plan", ownedPaths: ["x"] });
  const done = runner.start();
  await settled();
  assert.equal(sdk.lastOptions.permissionMode, "plan");
  runner.close("done");
  await done;
});

test("planning messages remain ordinary mailbox conversation", async () => {
  const { runner, of } = harness();
  const done = runner.start();
  runner.deliver({
    id: "p1",
    kind: "message",
    sender_runtime_id: "host",
    correlation_id: "corr-1",
    body: "Return a Markdown plan first. Do not execute yet.",
  });
  await settled();
  runner.deliver({
    id: "p2",
    kind: "message",
    sender_runtime_id: "host",
    correlation_id: "corr-1",
    body: "Plan reviewed. Revise item 2, then execute.",
  });
  await settled();
  assert.equal(of("turn_complete").length, 2);
  assert.equal(of("plan_gate_armed").length, 0);
  runner.close("done");
  await done;
});

test("the member closes the interrupted query, resumes, and consumes the next message", async () => {
  // Regression for a live defect found by the 2026-07-27 canary: `interrupt()`
  // ends the SDK *query*, not the turn. The first implementation bound one
  // member to one query, so interrupting left the member hung — the stream
  // stopped yielding but never ended, later deliveries went nowhere, and
  // `member_closed` never fired. A member now spans query generations.
  const { runner, sdk, of } = harness();
  const done = runner.start();

  runner.deliver({ id: "w1", kind: "work", sender_runtime_id: "host", body: "long task" });
  await settled();

  await runner.interrupt();
  await settled();

  assert.equal(runner.mailbox.closed, false, "an interrupt must not end the member");
  assert.equal(of("member_closed").length, 0);
  assert.equal(sdk.calls.queryCloses, 1, "the spent SDK query must be closed without awaiting return()");
  assert.equal(of("member_resumed_after_interrupt").length, 1, "a fresh query resumed");

  // The load-bearing assertion: the member is still reachable afterwards.
  const before = of("turn_complete").length;
  runner.deliver({ id: "m2", kind: "message", sender_runtime_id: "host", body: "still there?" });
  await settled();
  assert.ok(of("turn_complete").length > before, "post-interrupt delivery must land");

  runner.close("done");
  await done;
  assert.equal(of("member_closed").length, 1);
});

test("the resumed query continues the same native session", async () => {
  const { runner, sdk, of } = harness();
  const done = runner.start();
  runner.deliver({ id: "w1", kind: "work", sender_runtime_id: "host", body: "x" });
  await settled();
  const bound = of("session_bound")[0].data.sessionId;

  await runner.interrupt();
  await settled();

  assert.equal(sdk.lastOptions.resume, bound, "reopen must resume, not start fresh");
  assert.equal(of("session_bound").length, 1, "still one MemberRun, one session");
  runner.close("done");
  await done;
});
