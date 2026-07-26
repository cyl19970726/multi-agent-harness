/**
 * Line-delimited JSON protocol between `harness-cli` (Rust) and this runner.
 *
 * One process per MemberRun. Rust owns coordination and the ledger; the runner
 * owns exactly one provider-native session and nothing else. Every outbound
 * event is a coordination fact Harness may persist — none of them carry
 * transcript, tool streams, or thinking, per ADR 0032 and the AGENTS.md
 * thinking policy.
 */

/** Rust -> runner. */
export const COMMANDS = Object.freeze({
  /** Begin the member. Payload is the config object (see README). */
  start: "start",
  /** Deliver one TeamMessage into the live session. */
  deliver: "deliver",
  /** Real provider interrupt; replies with `interrupted`. */
  interrupt: "interrupt",
  /** Steer permission posture, e.g. plan -> acceptEdits after approval. */
  set_permission_mode: "set_permission_mode",
  /** Terminal: the Host accepted the handoff, or the run is being torn down. */
  close: "close",
});

/** Runner -> Rust. */
export const EVENTS = Object.freeze({
  member_started: "member_started",
  /** Native session bound + registered. Carries the id Harness stores. */
  session_bound: "session_bound",
  assistant_message: "assistant_message",
  /** A turn finished. NOT a member lifecycle event. */
  turn_complete: "turn_complete",
  turn_idle: "turn_idle",
  delivered: "delivered",
  interrupted: "interrupted",
  permission_mode_changed: "permission_mode_changed",
  plan_approved: "plan_approved",
  plan_gate_blocked: "plan_gate_blocked",
  owned_path_violation: "owned_path_violation",
  registry_write_failed: "registry_write_failed",
  /** Terminal. Only ever follows an explicit `close`. */
  member_closed: "member_closed",
  runner_error: "runner_error",
});

/** Parse one NDJSON line into `{ command, payload }`, or throw. */
export function parseCommand(line) {
  const frame = JSON.parse(line);
  if (typeof frame?.command !== "string") {
    throw new Error("frame is missing a string `command`");
  }
  if (!Object.hasOwn(COMMANDS, frame.command)) {
    throw new Error(`unknown command: ${frame.command}`);
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
