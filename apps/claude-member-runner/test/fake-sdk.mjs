/**
 * A fake Agent SDK surface.
 *
 * Exists so the runner's lifecycle — which is the part we actually got wrong —
 * can be tested deterministically with no provider credentials, no network,
 * and no installed SDK. It mimics only the shapes the runner consumes:
 * the init/assistant/result message sequence, `interrupt()`,
 * `setPermissionMode()`, and the two session-registry writes.
 *
 * It is not a provider simulator. Anything that needs real provider behaviour
 * (actual tool execution, real interrupt acknowledgement) belongs in a live
 * canary, not here.
 */

export function createFakeSdk({
  sessionId = "fake-session-0001",
  claudeCodeVersion = "2.1.220-test",
  model = "claude-sonnet-4-5",
  // When set, every turn ends in a provider API failure result, shaped exactly
  // like the real SDK's: `subtype` stays "success" while `is_error` carries the
  // truth (live probe, issue #293).
  apiErrorStatus = null,
} = {}) {
  const calls = { tagSession: [], renameSession: [], permissionModes: [], interrupts: 0 };
  let lastOptions = null;

  function query({ prompt, options }) {
    lastOptions = options;
    let interrupted = false;

    async function* run() {
      yield {
        type: "system",
        subtype: "init",
        session_id: sessionId,
        claude_code_version: claudeCodeVersion,
        model,
      };
      // One turn per inbound user message. The stream ends only when the
      // mailbox closes — which is exactly the property under test.
      for await (const userMessage of prompt) {
        if (interrupted) {
          interrupted = false;
          continue;
        }
        // Read through the nested `message`, exactly as the real SDK does.
        // An earlier version of this fake read `userMessage.content` — the
        // shape the published docs show — so it happily accepted a payload
        // the real provider rejects. A fake written from the same wrong
        // assumption as the code under test validates nothing; this one is
        // pinned to the SDK's `.d.ts` instead.
        if (userMessage.message?.role !== "user") {
          throw new Error(
            `Expected message role 'user', got '${userMessage.message?.role}'`,
          );
        }
        const text = userMessage.message.content?.[0]?.text ?? "";
        if (apiErrorStatus != null) {
          yield {
            type: "assistant",
            message: {
              content: [
                { type: "text", text: `Failed to authenticate. API Error: ${apiErrorStatus} Request not allowed` },
              ],
            },
          };
          yield {
            type: "result",
            subtype: "success",
            is_error: true,
            terminal_reason: "api_error",
            api_error_status: apiErrorStatus,
            session_id: sessionId,
          };
          continue;
        }
        yield {
          type: "assistant",
          message: { content: [{ type: "text", text: `ack: ${text.slice(0, 40)}` }] },
        };
        yield { type: "result", subtype: "success", session_id: sessionId };
      }
      // The real SDK re-throws the session's last error result when the input
      // stream ends (live probe, issue #293). The runner must still emit
      // member_closed rather than turning a clean Host close into a crash.
      if (apiErrorStatus != null) {
        throw new Error(
          `Claude Code returned an error result: Failed to authenticate. API Error: ${apiErrorStatus} Request not allowed`,
        );
      }
    }

    const iterator = run();
    return {
      [Symbol.asyncIterator]: () => iterator,
      async interrupt() {
        calls.interrupts += 1;
        interrupted = true;
        return { still_queued: [] };
      },
      async setPermissionMode(mode) {
        calls.permissionModes.push(mode);
      },
    };
  }

  return {
    query,
    async tagSession(id, tag, opts) {
      calls.tagSession.push({ id, tag, opts });
    },
    async renameSession(id, title, opts) {
      calls.renameSession.push({ id, title, opts });
    },
    // test affordances
    calls,
    get lastOptions() {
      return lastOptions;
    },
  };
}
