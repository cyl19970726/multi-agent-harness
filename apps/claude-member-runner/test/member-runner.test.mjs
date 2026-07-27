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
import { isInside, ownedPathsObserver, planApprovalGate } from "../src/gates.mjs";
import { createFakeSdk } from "./fake-sdk.mjs";

const baseConfig = {
  teamRunId: "trun-1",
  memberRunId: "mrun-runtime",
  memberName: "RuntimeBuilder",
  roleLabel: "Runtime owner",
  cwd: "/tmp/project",
  allowedTools: ["Read", "Edit", "Bash"],
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

  runner.deliver({ id: "m1", kind: "assignment", from_member_id: "host", body: "build it" });
  await settled();

  // The queue is now empty. Under the current batch design the member would
  // have terminated here.
  assert.equal(runner.mailbox.pending, 0);
  assert.equal(runner.mailbox.closed, false, "empty queue must not end the member");
  assert.equal(of("member_closed").length, 0);

  // A message arriving after the lull still has a recipient.
  runner.deliver({ id: "m2", kind: "progress", from_member_id: "host", body: "also do this" });
  await settled();

  assert.equal(of("turn_complete").length, 2, "both messages produced a turn");

  runner.close("review_result_accepted");
  await done;
  assert.equal(of("member_closed")[0].data.reason, "review_result_accepted");
});

test("only the Host ends the member, and the reason is recorded", async () => {
  const { runner, of } = harness();
  const done = runner.start();
  runner.deliver({ id: "m1", kind: "assignment", from_member_id: "host", body: "x" });
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
  runner.deliver({ id: "m1", kind: "assignment", from_member_id: "host", body: "x" });
  await settled();
  runner.deliver({ id: "m2", kind: "question", from_member_id: "host", body: "y" });
  await settled();
  runner.close("done");
  await done;

  assert.equal(of("session_bound").length, 1, "bind exactly once");
  assert.equal(sdk.calls.tagSession.length, 1);
  assert.equal(sdk.calls.tagSession[0].tag, "trun-1:mrun-runtime");
  assert.equal(sdk.calls.renameSession[0].title, "RuntimeBuilder · Runtime owner");
});

test("undelivered messages are reported rather than silently dropped", async () => {
  const { runner, of } = harness();
  const done = runner.start();
  await settled();

  // Two arrive while the member is mid-turn on nothing; close before drain.
  runner.mailbox.push({ id: "m9", kind: "progress", from_member_id: "peer", body: "late" });
  runner.close("closed_by_host");
  await done;

  const closed = of("member_closed")[0].data;
  assert.ok(
    closed.undelivered.includes("m9") || of("turn_complete").length === 1,
    "a message is either consumed or reported as undelivered — never lost",
  );
});

test("plan gate blocks edits until a correlated plan_approval arrives", async () => {
  const state = { planRequired: true, planApproved: false };
  const gate = planApprovalGate({ state });
  const input = { hook_event_name: "PreToolUse", tool_name: "Edit", tool_input: {} };

  const blocked = await gate(input);
  assert.equal(blocked.hookSpecificOutput.permissionDecision, "deny");

  state.planApproved = true;
  assert.deepEqual(await gate(input), {}, "approval opens the gate");
});

test("plan_approval delivery flips the gate", async () => {
  const { runner, of } = harness({ planRequired: true });
  const done = runner.start();
  assert.equal(runner.state.planApproved, false);

  runner.deliver({
    id: "m1",
    kind: "plan_approval",
    from_member_id: "host",
    correlation_id: "corr-1",
    body: "approved",
  });
  await settled();

  assert.equal(runner.state.planApproved, true);
  assert.equal(of("plan_approved")[0].data.correlationId, "corr-1");
  runner.close("done");
  await done;
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
  // Verified live: bypassPermissions skips the prompt layer, not the hooks.
  // A PreToolUse deny still blocks a Write. See FINDINGS §C.
  assert.ok(sdk.lastOptions.hooks?.PreToolUse?.length > 0, "gates stay wired");
  runner.close("done");
  await done;
});



test("an explicit permission mode is not overridden", async () => {
  const { runner, sdk } = harness({ permissionMode: "plan", ownedPaths: ["x"] });
  const done = runner.start();
  await settled();
  assert.equal(sdk.lastOptions.permissionMode, "plan");
  runner.close("done");
  await done;
});
