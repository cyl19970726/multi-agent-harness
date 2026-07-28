#!/usr/bin/env node
/**
 * stdio entry point: one process per MemberRun.
 *
 * Rust writes NDJSON commands on stdin, reads NDJSON events on stdout. stderr
 * is diagnostics only and is never parsed as protocol.
 *
 * Run standalone for a dry run without provider credentials:
 *   node bin/claude-member-runner.mjs --fake < fixtures/session.ndjson
 */

import { createMemberRunner } from "../src/member-runner.mjs";
import { COMMANDS, EVENTS, encodeEvent, parseCommand, splitLines } from "../src/protocol.mjs";
import { createFakeSdk } from "../test/fake-sdk.mjs";

const useFake = process.argv.includes("--fake");

function emit(event, data) {
  process.stdout.write(encodeEvent(event, data));
}

async function loadSdk() {
  if (useFake) return createFakeSdk();
  // Imported lazily so `--fake` runs with no dependency on the SDK package
  // being installed, and so a missing dependency is reported as a protocol
  // error rather than a module-load crash.
  const sdk = await import("@anthropic-ai/claude-agent-sdk");
  return {
    query: sdk.query,
    tagSession: sdk.tagSession,
    renameSession: sdk.renameSession,
  };
}

async function main() {
  const sdk = await loadSdk();
  let runner = null;
  let started = null;
  let buffer = "";

  process.stdin.setEncoding("utf8");

  for await (const chunk of process.stdin) {
    buffer += chunk;
    const { lines, rest } = splitLines(buffer);
    buffer = rest;

    for (const line of lines) {
      let frame;
      try {
        frame = parseCommand(line);
      } catch (error) {
        emit(EVENTS.runner_error, { stage: "parse", error: String(error) });
        continue;
      }

      try {
        switch (frame.command) {
          case COMMANDS.start:
            if (runner) throw new Error("member already started");
            runner = createMemberRunner({ sdk, config: frame.payload, emit });
            // Not awaited: `start()` resolves only when the member ends.
            started = runner.start().catch((error) => {
              emit(EVENTS.runner_error, { stage: "run", error: String(error) });
            });
            break;

          case COMMANDS.deliver:
            requireRunner(runner).deliver(frame.payload);
            break;

          case COMMANDS.interrupt:
            await requireRunner(runner).interrupt();
            break;

          case COMMANDS.set_permission_mode:
            await requireRunner(runner).setPermissionMode(frame.payload.mode);
            break;

          case COMMANDS.close:
            requireRunner(runner).close(frame.payload.reason ?? "closed_by_host");
            break;
        }
      } catch (error) {
        emit(EVENTS.runner_error, { stage: frame.command, error: String(error) });
      }
    }
  }

  // stdin ended. Close the member if the Host did not, so we never leave an
  // orphaned provider session behind.
  if (runner && !runner.mailbox.closed) runner.close("stdin_closed");
  if (started) await started;
}

function requireRunner(runner) {
  if (!runner) throw new Error("member not started");
  return runner;
}

main().catch((error) => {
  emit(EVENTS.runner_error, { stage: "fatal", error: String(error) });
  process.exitCode = 1;
});
