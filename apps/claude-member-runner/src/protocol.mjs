/**
 * Line-delimited JSON protocol between `harness-cli` (Rust) and this runner.
 *
 * One process per MemberRun. Rust owns coordination and the ledger; the runner
 * owns exactly one provider-native session and nothing else. Every outbound
 * event is a coordination fact Harness may persist — none of them carry
 * transcript, tool streams, or thinking, per ADR 0032 and the AGENTS.md
 * thinking policy.
 */

import fs from "node:fs";

export const RUNNER_CONTRACT = Object.freeze(
  JSON.parse(
    fs.readFileSync(new URL("../contract/runner-v1.json", import.meta.url), "utf8"),
  ),
);

const vocabulary = (names) =>
  Object.freeze(Object.fromEntries(names.map((name) => [name, name])));

/** Rust -> runner. */
export const COMMANDS = vocabulary(RUNNER_CONTRACT.commands);

/** Runner -> Rust. */
export const EVENTS = vocabulary(RUNNER_CONTRACT.events);

/** Parse one NDJSON line into `{ command, payload }`, or throw. */
export function parseCommand(line) {
  const frame = JSON.parse(line);
  if (typeof frame?.command !== "string") {
    throw new Error("frame is missing a string `command`");
  }
  if (!Object.hasOwn(COMMANDS, frame.command)) {
    throw new Error(`unknown command: ${frame.command}`);
  }
  if (frame.command === COMMANDS.start) {
    const payload = frame.payload ?? {};
    if (
      payload.protocolVersion !== RUNNER_CONTRACT.protocolVersion ||
      payload.protocolFingerprint !== RUNNER_CONTRACT.fingerprint
    ) {
      throw new Error(
        `runner contract mismatch: expected ${RUNNER_CONTRACT.protocolVersion} ${RUNNER_CONTRACT.fingerprint}`,
      );
    }
  }
  return { command: frame.command, payload: frame.payload ?? {} };
}

/** Serialise one outbound event as an NDJSON line. */
export function encodeEvent(event, data) {
  return `${JSON.stringify({ event, data })}\n`;
}

/**
 * Split a byte stream into complete lines. Returns `{ lines, rest }` so the
 * caller keeps the partial tail for the next chunk.
 */
export function splitLines(buffer) {
  const parts = buffer.split("\n");
  return { lines: parts.slice(0, -1).filter((l) => l.trim() !== ""), rest: parts.at(-1) };
}
