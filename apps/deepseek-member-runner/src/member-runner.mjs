import { randomUUID } from "node:crypto";
import contract from "../contract/runner-v1.json" with { type: "json" };

export const PROTOCOL_VERSION = contract.protocolVersion;
export const PROTOCOL_FINGERPRINT = contract.fingerprint;

const textOf = (message) => (message?.content ?? [])
  .filter((block) => block?.type === "text")
  .map((block) => block.text ?? "")
  .join("\n");

export function createMemberRunner({ runtime, emit }) {
  let handle;
  let activeInputId;
  let activeMessageId;
  let interrupted = false;
  let disposed = false;
  let detachEvents;

  function requireHandle() {
    if (!handle || disposed) throw new Error("DSH_SESSION_NOT_BOUND");
    return handle;
  }

  async function start(payload) {
    if (payload.protocolVersion !== PROTOCOL_VERSION || payload.protocolFingerprint !== PROTOCOL_FINGERPRINT) {
      throw new Error("DSH_PROTOCOL_MISMATCH");
    }
    if (handle) throw new Error("DSH_SESSION_ALREADY_BOUND");
    const options = {
      agentOptions: {
        provider: "deepseek-official",
        model: payload.model ?? "deepseek-v4-pro",
        reasoningEffort: payload.effort ?? "max",
      },
    };
    if (payload.resumeSessionId) {
      handle = await runtime.resume({ ...options, resumeSessionId: payload.resumeSessionId });
    } else {
      handle = await runtime.create({ ...options, sessionId: payload.sessionId ?? `star-${randomUUID()}` });
    }
    detachEvents = runtime.onEvent(handle.agent.session, (event) => {
      if (event.type === "agent/inbox/spliced" && activeMessageId
        && event.data?.inserted?.some((message) => message.id === activeMessageId)) {
        emit("consumed", { id: activeInputId, kind: "runtime_cycle", sessionId: handle.agent.session.id });
      }
      if (event.type === "tool/call") {
        emit("assistant_message", { sessionId: handle.agent.session.id, content: [{ type: "tool_use", name: event.data?.name ?? "tool" }] });
      }
      if (event.type === "assistant/message") {
        const text = textOf(event.data?.message);
        if (text) emit("assistant_message", { sessionId: handle.agent.session.id, content: [{ type: "text", text }] });
      }
    });
    emit("session_bound", {
      sessionId: handle.agent.session.id,
      resumed: Boolean(payload.resumeSessionId),
      providerVersion: "0.1.1-rc.2",
      sourceRevision: "b150a551b8d465e31e418e1b2eaf5e79bbb7d28e",
      compositionFingerprint: PROTOCOL_FINGERPRINT,
      tag: `${payload.teamRunId}:${payload.memberRunId}`,
      title: `${payload.memberName} · ${payload.roleLabel ?? "member"}`,
      model: options.agentOptions.model,
      effort: options.agentOptions.reasoningEffort,
    });
  }

  async function prompt(payload) {
    const current = requireHandle();
    if (activeInputId) throw new Error("DSH_CYCLE_ALREADY_ACTIVE");
    activeInputId = payload.id;
    interrupted = false;
    let finalText = "";
    let toolCallCount = 0;
    let turnReason;
    const message = runtime.createUserMessage(payload.body);
    activeMessageId = message.id;
    const detachCycle = runtime.onEvent(current.agent.session, (event) => {
      if (event.type === "assistant/message") finalText = textOf(event.data?.message) || finalText;
      if (event.type === "tool/call") toolCallCount += 1;
      if (event.type === "turn/end") turnReason = event.data?.reason ?? turnReason;
    });
    try {
      current.agent.followup(message);
      await current.agent.whenIdle();
      await runtime.flush(current.agent.session);
      if (!interrupted) {
        emit("turn_complete", {
          triggerMessageId: activeInputId,
          sessionId: current.agent.session.id,
          subtype: "success",
          evidenceRefs: [],
          isError: turnReason?.kind !== "completed",
          terminalReason: turnReason?.kind === "error"
            ? (turnReason.error?.code ?? turnReason.error?.message ?? "provider_error")
            : (turnReason?.kind ?? "missing_terminal"),
          apiErrorStatus: turnReason?.error?.status ?? null,
        });
      }
    } finally {
      detachCycle();
      activeInputId = undefined;
      activeMessageId = undefined;
    }
  }

  async function interrupt(payload = {}) {
    const current = requireHandle();
    const interruptedInputId = activeInputId;
    interrupted = true;
    current.agent.cancel({ kind: "user", detail: payload.reason ?? "Harness interrupt" }, { keepInbox: false });
    await current.agent.whenIdle();
    await runtime.flush(current.agent.session);
    emit("interrupted", { stillQueued: [], abandonedTriggerMessageIds: interruptedInputId ? [interruptedInputId] : [] });
    emit("member_resumed_after_interrupt", { sessionId: current.agent.session.id });
  }

  async function close(payload = {}) {
    const current = requireHandle();
    if (activeInputId) await interrupt({ reason: payload.reason ?? "Harness close" });
    const sessionId = current.agent.session.id;
    detachEvents?.();
    await current.dispose();
    disposed = true;
    emit("member_closed", { sessionId, reason: payload.reason ?? "Harness close", undelivered: [], evidenceRefs: [] });
  }

  return {
    async command(frame) {
      switch (frame.command) {
        case "start": return start(frame.payload ?? {});
        case "deliver": return prompt(frame.payload ?? {});
        case "interrupt": return interrupt(frame.payload ?? {});
        case "close": return close(frame.payload ?? {});
        default: throw new Error(`DSH_UNKNOWN_COMMAND:${frame.command}`);
      }
    },
  };
}
