#!/usr/bin/env node
import readline from "node:readline";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { boot, installFailLoud } from "@deepseek-ai/dsh-app-boot";
import { createUserMessage } from "@deepseek-ai/dsh-llm";
import { SessionId } from "@deepseek-ai/dsh-session";
import { createMemberRunner } from "../src/member-runner.mjs";
import { registerMemberRoleActionEnvironment } from "../src/member-role-action-env.mjs";

const name = "star-deepseek-member-runner";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const uninstallFailLoud = installFailLoud(name);
let ctx;

function emit(event, data) {
  process.stdout.write(`${JSON.stringify({ event, data })}\n`);
}

try {
  ctx = await boot(name, resolve(root, "cordis.yml"));
  registerMemberRoleActionEnvironment(ctx);
  const runtime = {
    create: (options) => ctx.agents.create({ ...options, sessionId: SessionId(options.sessionId) }),
    resume: (options) => ctx.agents.resume({ ...options, resumeSessionId: SessionId(options.resumeSessionId) }),
    createUserMessage: (content) => createUserMessage({ content: [{ type: "text", text: content }], source: { kind: "user" } }),
    onEvent: (session, listener) => ctx.on("session/event", (candidate, event) => {
      if (candidate === session) listener(event);
    }),
    flush: (session) => ctx.sessions.flush(session),
  };
  const runner = createMemberRunner({ runtime, emit });
  const active = new Set();
  const dispatch = (frame) => {
    const task = runner.command(frame).catch((error) => {
      emit("runner_error", { stage: frame?.command ?? "decode", error: error instanceof Error ? error.message : String(error) });
    }).finally(() => active.delete(task));
    active.add(task);
    return task;
  };
  const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of lines) {
    if (!line.trim()) continue;
    let frame;
    try {
      frame = JSON.parse(line);
      if (frame.command === "deliver") {
        dispatch(frame);
        continue;
      }
      await dispatch(frame);
      if (frame.command === "close") {
        lines.close();
        process.stdin.pause();
        break;
      }
    } catch (error) {
      emit("runner_error", { stage: frame?.command ?? "decode", error: error instanceof Error ? error.message : String(error) });
    }
  }
  await Promise.allSettled(active);
} finally {
  await ctx?.fiber.dispose();
  uninstallFailLoud();
}
