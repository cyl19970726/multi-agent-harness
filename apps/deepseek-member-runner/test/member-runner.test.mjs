import assert from "node:assert/strict";
import test from "node:test";
import { createMemberRunner, PROTOCOL_FINGERPRINT, PROTOCOL_VERSION } from "../src/member-runner.mjs";

function fakeRuntime() {
  const listeners = new Set();
  let sequence = 0;
  const session = { id: "dsh-session-1" };
  const agent = {
    session,
    followup(message) {
      for (const listener of listeners) listener({ type: "agent/inbox/spliced", data: { inserted: [message] } });
      for (const listener of listeners) listener({ type: "turn/start", data: { turn: sequence } });
      for (const listener of listeners) listener({ type: "assistant/message", data: { message: { content: [{ type: "text", text: "done" }] } } });
      for (const listener of listeners) listener({ type: "turn/end", data: { turn: sequence, reason: { kind: "completed" } } });
    },
    cancel() {},
    async whenIdle() {},
  };
  return {
    listeners,
    create: async () => ({ agent, dispose: async () => {} }),
    resume: async () => ({ agent, dispose: async () => {} }),
    createUserMessage: (content) => ({ id: `message-${++sequence}`, content: [{ type: "text", text: content }] }),
    onEvent: (_session, listener) => { listeners.add(listener); return () => listeners.delete(listener); },
    flush: async () => {},
  };
}

test("binds, receipts, settles, and closes one native DSH session", async () => {
  const events = [];
  const runner = createMemberRunner({ runtime: fakeRuntime(), emit: (event, data) => events.push({ event, data }) });
  await runner.command({ command: "start", payload: { protocolVersion: PROTOCOL_VERSION, protocolFingerprint: PROTOCOL_FINGERPRINT } });
  await runner.command({ command: "deliver", payload: { id: "work-delivery-1", body: "do work" } });
  await runner.command({ command: "close", payload: { reason: "test" } });
  assert.deepEqual(events.map(({ event }) => event), ["session_bound", "consumed", "assistant_message", "turn_complete", "member_closed"]);
  assert.equal(events.find(({ event }) => event === "consumed").data.id, "work-delivery-1");
  assert.equal(events.find(({ event }) => event === "assistant_message").data.content[0].text, "done");
  assert.equal(events.at(-1).data.sessionId, "dsh-session-1");
});

test("resumes the exact requested native session", async () => {
  const runtime = fakeRuntime();
  let resumed;
  runtime.resume = async (options) => { resumed = options.resumeSessionId; return runtime.create(options); };
  const events = [];
  const runner = createMemberRunner({ runtime, emit: (event, data) => events.push({ event, data }) });
  await runner.command({ command: "start", payload: { protocolVersion: PROTOCOL_VERSION, protocolFingerprint: PROTOCOL_FINGERPRINT, resumeSessionId: "dsh-session-1" } });
  assert.equal(resumed, "dsh-session-1");
  assert.equal(events[0].data.resumed, true);
});

test("normalizes native live phases without exposing reasoning or tool arguments", async () => {
  const runtime = fakeRuntime();
  const handle = await runtime.create({});
  handle.agent.followup = (message) => {
    for (const listener of runtime.listeners) {
      listener({ type: "agent/inbox/spliced", data: { inserted: [message] } });
      listener({ type: "turn/start", data: { turn: 1 } });
      listener({ type: "assistant/chunk", data: { chunk: { type: "block-start", blockType: "reasoning", text: "secret" } } });
      listener({ type: "tool/call", data: { name: "bash", arguments: "private" } });
      listener({ type: "tool/result", data: { output: "private" } });
      listener({ type: "assistant/message", data: { message: { content: [{ type: "text", text: "done" }] } } });
      listener({ type: "turn/end", data: { turn: 1, reason: { kind: "completed" } } });
    }
  };
  runtime.create = async () => handle;
  const events = [];
  const runner = createMemberRunner({ runtime, emit: (event, data) => events.push({ event, data }) });
  await runner.command({ command: "start", payload: { protocolVersion: PROTOCOL_VERSION, protocolFingerprint: PROTOCOL_FINGERPRINT } });
  await runner.command({ command: "deliver", payload: { id: "live-phases", body: "do work" } });
  const activities = events.filter(({ event }) => event === "provider_activity");
  assert.deepEqual(activities.map(({ data }) => data.kind), ["thinking", "tool_started", "tool_completed"]);
  assert.equal(JSON.stringify(activities).includes("secret"), false);
  assert.equal(JSON.stringify(activities).includes("private"), false);
});

test("idle without a native turn/end fails closed", async () => {
  const runtime = fakeRuntime();
  runtime.turnTerminalTimeoutMs = 5;
  const handle = await runtime.create({});
  handle.agent.followup = (message) => {
    for (const listener of runtime.listeners ?? []) {
      listener({ type: "agent/inbox/spliced", data: { inserted: [message] } });
      listener({ type: "turn/start", data: { turn: 1 } });
    }
  };
  runtime.create = async () => handle;
  const events = [];
  const runner = createMemberRunner({ runtime, emit: (event, data) => events.push({ event, data }) });
  await runner.command({ command: "start", payload: { protocolVersion: PROTOCOL_VERSION, protocolFingerprint: PROTOCOL_FINGERPRINT } });
  await runner.command({ command: "deliver", payload: { id: "missing-terminal", body: "do work" } });
  const terminal = events.find(({ event }) => event === "turn_complete").data;
  assert.equal(terminal.isError, true);
  assert.equal(terminal.terminalReason, "missing_terminal");
});

test("interrupt settles without fabricating turn completion and retains the session", async () => {
  const runtime = fakeRuntime();
  let releaseIdle;
  const idle = new Promise((resolve) => { releaseIdle = resolve; });
  const originalCreate = runtime.create;
  runtime.create = async (options) => {
    const handle = await originalCreate(options);
    handle.agent.whenIdle = () => idle;
    handle.agent.cancel = () => releaseIdle();
    return handle;
  };
  const events = [];
  const runner = createMemberRunner({ runtime, emit: (event, data) => events.push({ event, data }) });
  await runner.command({ command: "start", payload: { protocolVersion: PROTOCOL_VERSION, protocolFingerprint: PROTOCOL_FINGERPRINT } });
  const delivery = runner.command({ command: "deliver", payload: { id: "work-delivery-interrupt", body: "long work" } });
  await Promise.resolve();
  await runner.command({ command: "interrupt", payload: { reason: "test interrupt" } });
  await delivery;
  assert.deepEqual(events.map(({ event }) => event), [
    "session_bound",
    "consumed",
    "assistant_message",
    "interrupted",
    "member_resumed_after_interrupt",
  ]);
  assert.equal(events.at(-1).data.sessionId, "dsh-session-1");
  assert.deepEqual(
    events.find(({ event }) => event === "interrupted").data.abandonedTriggerMessageIds,
    ["work-delivery-interrupt"],
  );
  assert.equal(events.some(({ event }) => event === "turn_complete"), false);
});

test("interrupt after turn terminal wins over a delivery still awaiting idle", async () => {
  const runtime = fakeRuntime();
  const handle = await runtime.create({});
  let releaseDeliveryIdle;
  const deliveryIdle = new Promise((resolve) => { releaseDeliveryIdle = resolve; });
  let idleCalls = 0;
  handle.agent.whenIdle = () => (++idleCalls === 1 ? deliveryIdle : Promise.resolve());
  handle.agent.cancel = () => {};
  runtime.create = async () => handle;
  const events = [];
  const runner = createMemberRunner({ runtime, emit: (event, data) => events.push({ event, data }) });
  await runner.command({ command: "start", payload: { protocolVersion: PROTOCOL_VERSION, protocolFingerprint: PROTOCOL_FINGERPRINT } });
  const delivery = runner.command({ command: "deliver", payload: { id: "terminal-then-interrupt", body: "long cleanup" } });
  await new Promise((resolve) => setImmediate(resolve));
  await runner.command({ command: "interrupt", payload: { reason: "interrupt during idle fence" } });
  releaseDeliveryIdle();
  await delivery;
  assert.equal(events.some(({ event }) => event === "turn_complete"), false);
  assert.deepEqual(
    events.find(({ event }) => event === "interrupted").data.abandonedTriggerMessageIds,
    ["terminal-then-interrupt"],
  );
});

test("post-interrupt followup waits for its exact inbox splice before the idle boundary", async () => {
  const runtime = fakeRuntime();
  const handle = await runtime.create({});
  handle.agent.cancel = () => {};
  handle.agent.whenIdle = async () => {};
  handle.agent.followup = (message) => {
    for (const listener of runtime.listeners) {
      listener({ type: "turn/end", data: { turn: 1, reason: { kind: "stale_previous_cycle" } } });
    }
    setImmediate(() => {
      for (const listener of runtime.listeners) {
        listener({ type: "agent/inbox/spliced", data: { inserted: [message] } });
        listener({ type: "turn/end", data: { turn: 1, reason: { kind: "stale_previous_cycle" } } });
      }
      setTimeout(() => {
        for (const listener of runtime.listeners) {
          listener({ type: "turn/start", data: { turn: 2 } });
          listener({ type: "assistant/message", data: { message: { content: [{ type: "text", text: "resumed" }] } } });
          listener({ type: "turn/end", data: { turn: 2, reason: { kind: "completed" } } });
        }
      }, 20);
    });
  };
  runtime.create = async () => handle;
  const events = [];
  const runner = createMemberRunner({ runtime, emit: (event, data) => events.push({ event, data }) });
  await runner.command({ command: "start", payload: { protocolVersion: PROTOCOL_VERSION, protocolFingerprint: PROTOCOL_FINGERPRINT } });
  await runner.command({ command: "interrupt", payload: { reason: "idle interrupt boundary" } });
  await runner.command({ command: "deliver", payload: { id: "post-interrupt", body: "continue" } });
  const terminal = events.find(({ event, data }) => event === "turn_complete" && data.triggerMessageId === "post-interrupt");
  assert.equal(terminal.data.isError, false);
  assert.equal(terminal.data.terminalReason, "completed");
});
