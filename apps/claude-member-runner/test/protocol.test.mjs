import { test } from "node:test";
import assert from "node:assert/strict";

import { COMMANDS, EVENTS, RUNNER_CONTRACT, parseCommand } from "../src/protocol.mjs";

test("shared runner contract owns the complete command and event vocabulary", () => {
  assert.deepEqual(Object.values(COMMANDS), RUNNER_CONTRACT.commands);
  assert.deepEqual(Object.values(EVENTS), RUNNER_CONTRACT.events);
  assert.ok(EVENTS.consumed, "input acceptance is part of the versioned event vocabulary");
});

test("start rejects a mismatched contract before provider loading", () => {
  assert.throws(
    () => parseCommand(JSON.stringify({ command: "start", payload: {} })),
    /runner contract mismatch/,
  );
  const parsed = parseCommand(
    JSON.stringify({
      command: "start",
      payload: {
        protocolVersion: RUNNER_CONTRACT.protocolVersion,
        protocolFingerprint: RUNNER_CONTRACT.fingerprint,
      },
    }),
  );
  assert.equal(parsed.command, "start");
});
