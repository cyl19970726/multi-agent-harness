import { test } from "node:test";
import assert from "node:assert/strict";

import { COMMANDS, EVENTS, RUNNER_CONTRACT, encodeEvent, parseCommand } from "../src/protocol.mjs";

test("shared runner contract owns the complete command and event vocabulary", () => {
  assert.deepEqual(Object.values(COMMANDS), RUNNER_CONTRACT.commands);
  assert.deepEqual(Object.values(EVENTS), RUNNER_CONTRACT.events);
  assert.ok(EVENTS.consumed, "input acceptance is part of the versioned event vocabulary");
});

test("start rejects a mismatched contract before provider loading", () => {
  const validPayload = {
    protocolVersion: RUNNER_CONTRACT.protocolVersion,
    protocolFingerprint: RUNNER_CONTRACT.fingerprint,
    teamRunId: "team-1",
    memberRunId: "member-1",
    memberName: "reviewer",
    cwd: "/tmp/project",
    ownedPaths: [],
    permissionMode: "bypassPermissions",
    settingSources: ["project", "user"],
  };
  assert.throws(
    () => parseCommand(JSON.stringify({
      command: "start",
      payload: { ...validPayload, protocolFingerprint: "wrong-contract" },
    })),
    /runner contract mismatch/,
  );
  const parsed = parseCommand(
    JSON.stringify({
      command: "start",
      payload: validPayload,
    }),
  );
  assert.equal(parsed.command, "start");
});

test("shared payload schemas reject malformed commands and events", () => {
  assert.throws(
    () => parseCommand(JSON.stringify({ command: "deliver", payload: { id: "only-id" } })),
    /missing kind/,
  );
  assert.throws(
    () => parseCommand(JSON.stringify({ command: "interrupt", payload: { surprise: true } })),
    /unknown property surprise/,
  );
  assert.throws(
    () => encodeEvent("session_bound", { sessionId: "s" }),
    /missing tag/,
  );
  assert.match(
    encodeEvent("delivered", { id: "message-1", kind: "runtime_cycle" }),
    /"event":"delivered"/,
  );
});
