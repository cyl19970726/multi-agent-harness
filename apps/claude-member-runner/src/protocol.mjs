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

function matchesType(value, expected) {
  if (expected === "null") return value === null;
  if (expected === "array") return Array.isArray(value);
  if (expected === "object") return value !== null && typeof value === "object" && !Array.isArray(value);
  if (expected === "number") return typeof value === "number" && Number.isFinite(value);
  return typeof value === expected;
}

export function validatePayload(schemaSet, name, payload) {
  const schema = RUNNER_CONTRACT[schemaSet]?.[name];
  if (!schema) throw new Error(`runner contract has no ${schemaSet} schema for ${name}`);
  validateSchema(schema, payload, `${name}.payload`);
  return payload;
}

function validateSchema(schema, value, path) {
  if (!schema || Object.keys(schema).length === 0) return;
  const types = Array.isArray(schema.type) ? schema.type : [schema.type];
  if (!types.some((type) => matchesType(value, type))) {
    throw new Error(`${path} must have type ${types.join("|")}`);
  }
  if (value === null) return;
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) validateSchema(schema.items ?? {}, item, `${path}[${index}]`);
    return;
  }
  if (schema.type === "object" || types.includes("object")) {
    for (const required of schema.required ?? []) {
      if (!Object.hasOwn(value, required)) throw new Error(`${path} is missing ${required}`);
    }
    const properties = schema.properties ?? {};
    for (const [key, item] of Object.entries(value)) {
      if (!Object.hasOwn(properties, key)) {
        if (schema.additionalProperties === false) throw new Error(`${path} has unknown property ${key}`);
        continue;
      }
      validateSchema(properties[key], item, `${path}.${key}`);
    }
  }
}

/** Parse one NDJSON line into `{ command, payload }`, or throw. */
export function parseCommand(line) {
  const frame = JSON.parse(line);
  if (typeof frame?.command !== "string") {
    throw new Error("frame is missing a string `command`");
  }
  if (!Object.hasOwn(COMMANDS, frame.command)) {
    throw new Error(`unknown command: ${frame.command}`);
  }
  const payload = frame.payload ?? {};
  validatePayload("commandPayloadSchemas", frame.command, payload);
  if (frame.command === COMMANDS.start) {
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
  if (!Object.hasOwn(EVENTS, event)) throw new Error(`unknown event: ${event}`);
  validatePayload("eventPayloadSchemas", event, data);
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
