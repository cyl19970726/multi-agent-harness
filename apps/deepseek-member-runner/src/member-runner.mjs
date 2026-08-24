import { randomUUID } from "node:crypto";
import contract from "../contract/runner-v1.json" with { type: "json" };

export const PROTOCOL_VERSION = contract.protocolVersion;
export const PROTOCOL_FINGERPRINT = contract.fingerprint;
const REVIEWED_PROVIDER = contract.reviewedProvider;

const textOf = (message) => (message?.content ?? [])
  .filter((block) => block?.type === "text")
  .map((block) => block.text ?? "")
  .join("\n");

export function createMemberRunner({ runtime, emit }) {
  let handle;
  let activeInputId;
  let activeMessageId;
  let interrupted = false;
  let releaseInterruptedCycle;
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
      providerVersion: REVIEWED_PROVIDER.version,
      sourceRevision: REVIEWED_PROVIDER.sourceRevision,
      compositionFingerprint: REVIEWED_PROVIDER.compositionFingerprint,
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
    let inputAcceptedObserved = false;
    let activeTurnNumber;
    let resolveTurnTerminal;
    let terminalTimer;
    const message = runtime.createUserMessage(payload.body);
    activeMessageId = message.id;
    let acceptTimer;
    let detachAcceptance = () => {};
    const inputAccepted = new Promise((resolve, reject) => {
      detachAcceptance = runtime.onEvent(current.agent.session, (event) => {
        if (event.type !== "agent/inbox/spliced"
          || !event.data?.inserted?.some((inserted) => inserted.id === message.id)) return;
        inputAcceptedObserved = true;
        clearTimeout(acceptTimer);
        detachAcceptance();
        resolve();
      });
      acceptTimer = setTimeout(() => {
        detachAcceptance();
        reject(new Error(`DSH_INPUT_NOT_ACCEPTED:${payload.id}`));
      }, 30_000);
    });
    const turnTerminal = new Promise((resolve) => { resolveTurnTerminal = resolve; });
    const interruptedCycle = new Promise((resolve) => { releaseInterruptedCycle = resolve; });
    const terminalTimeout = Number.isFinite(runtime.turnTerminalTimeoutMs)
      ? new Promise((resolve) => { terminalTimer = setTimeout(resolve, runtime.turnTerminalTimeoutMs); })
      : new Promise(() => {});
    const detachCycle = runtime.onEvent(current.agent.session, (event) => {
      if (event.type === "assistant/message") finalText = textOf(event.data?.message) || finalText;
      if (event.type === "tool/call") toolCallCount += 1;
      if (event.type === "turn/start" && inputAcceptedObserved && activeTurnNumber === undefined) {
        activeTurnNumber = event.data?.turn;
      }
      if (event.type === "turn/end" && activeTurnNumber !== undefined
        && event.data?.turn === activeTurnNumber) {
        turnReason = event.data?.reason ?? turnReason;
        resolveTurnTerminal();
      }
    });
    try {
      current.agent.followup(message);
      // DSH can still report the previous idle boundary immediately after an
      // interrupt. Bind this cycle to its exact inbox splice before awaiting
      // idle/turn-end, otherwise a newly accepted followup can be settled as
      // missing_terminal without ever observing its native turn.
      await inputAccepted;
      // `whenIdle()` can still resolve from the previous interrupted cycle
      // after the exact splice. Bind the subsequent exact turn/start number
      // and require its matching turn/end. An authenticated interrupt releases
      // the wait only after its own idle+flush boundary.
      await Promise.race([turnTerminal, interruptedCycle, terminalTimeout]);
      if (!interrupted) {
        await current.agent.whenIdle();
        await runtime.flush(current.agent.session);
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
      clearTimeout(acceptTimer);
      clearTimeout(terminalTimer);
      detachAcceptance();
      detachCycle();
      releaseInterruptedCycle = undefined;
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
    releaseInterruptedCycle?.();
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
