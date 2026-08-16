/**
 * A persistent Claude Agent Team member.
 *
 * One runner == one `MemberRun` == one provider-native Claude session that
 * stays alive across durable Work inputs, ordinary TeamMessages, many Host
 * plan revisions, and (per ADR 0037) across Waves — until the Host closes it.
 *
 * What this replaces
 * ------------------
 * `run_claude_team_member` in `harness-cli` spawns `claude -p "<envelope>"`
 * per delivery and loops `while let Some(prompt)`. Each delivery is a fresh
 * process; `--resume` reopens a finished conversation rather than continuing a
 * live one. That shape cannot honour steer, interrupt, wake, or "carry the
 * member forward into the next Wave", and `docs/integration/claude.md` says so
 * itself ("No mid-turn control channel", "No interrupt / thread pause").
 *
 * The Agent SDK's streaming-input mode gives all of it natively:
 *   query({ prompt: AsyncIterable })  -> a live Query handle
 *   q.streamInput(stream)             -> deliver into a running session
 *   q.interrupt()                     -> real interrupt, returns still_queued
 *   q.setPermissionMode() / setModel()-> steer, backed by the real protocol
 *
 * AGENTS.md requires interactive controls to be backed by the selected mode's
 * real protocol and terminal acknowledgements. These are; the `-p` path's were
 * not, which is why the current adapter reports them as unsupported.
 *
 * Boundary
 * --------
 * Harness stores the binding and the coordination facts. It does not copy the
 * transcript: the bound native session is the sole execution truth, read on
 * demand through `getSessionMessages`. Nothing here writes a second history.
 */

import { Mailbox, renderTeamMessage } from "./mailbox.mjs";
import { buildHooks } from "./gates.mjs";

/**
 * @param {object} deps
 * @param {object} deps.sdk   injected Agent SDK surface: { query, tagSession,
 *                            renameSession }. Injected rather than imported so
 *                            the runner is testable with a fake and without
 *                            provider credentials.
 * @param {object} deps.config member/run configuration (see README).
 * @param {(event: string, data: object) => void} deps.emit outbound events to
 *                            the Harness side of the stdio protocol.
 */
export function createMemberRunner({ sdk, config, emit }) {
  const mailbox = new Mailbox();
  const evidence = [];
  // `delivered` means the runner accepted a Work or TeamMessage input into its mailbox.
  // `consumedMessageIds` records the stronger fact that the provider stream
  // actually pulled that message as the input for a turn. The Rust control
  // plane uses the matching id to acknowledge the exact input it consumed.
  const consumedMessageIds = [];

  const state = {
    sessionId: null,
    registered: false,
    pending: () => mailbox.pending,
  };

  const hooks = buildHooks({
    state,
    cwd: config.cwd,
    ownedPaths: config.ownedPaths,
    collect: (ref) => {
      // De-duplicate: a member editing one file ten times is one evidence ref.
      if (!evidence.some((e) => e.ref === ref.ref)) evidence.push(ref);
    },
    emit,
  });

  let query = null;
  // `interrupt()` ends the SDK *query*, not the member. Live probe on
  // 2026-07-27: after `query.interrupt()` the stream stopped yielding, the
  // turn never produced a `result`, later deliveries went nowhere and the
  // member hung with `member_closed` never emitted. The SDK's own wording is
  // "Interrupts the query." — so the member survives an interrupt by opening a
  // fresh query that resumes the same native session, which is what this flag
  // drives.
  let interruptedGeneration = false;

  /**
   * Bind the native session to this MemberRun the first time we see it, and
   * register it in the provider's own session registry.
   *
   * `tagSession` is the member registry: Harness does not keep a second roster.
   * `listSessions()` filtered by this tag IS the list of a TeamRun's members,
   * which is why the tag encodes both ids.
   */
  async function bindSession(sessionId, providerVersion = null, effectiveModel = null) {
    if (state.registered) return;
    state.sessionId = sessionId;
    state.registered = true;

    const tag = `${config.teamRunId}:${config.memberRunId}`;
    const title = `${config.memberName} · ${config.roleLabel ?? "member"}`;
    try {
      await sdk.tagSession(sessionId, tag, { dir: config.cwd });
      await sdk.renameSession(sessionId, title, { dir: config.cwd });
    } catch (error) {
      // Registration is a convenience for SDK-native discovery and resume.
      // Agent SDK sessions do not automatically enter Claude Desktop's session
      // picker, so registry metadata must not be described as a Desktop bridge.
      // A registry write failure still must not take the member down.
      emit("registry_write_failed", { sessionId, error: String(error) });
    }
    emit("session_bound", {
      sessionId,
      tag,
      title,
      providerVersion,
      model: effectiveModel ?? config.model ?? null,
      effort: config.effort ?? null,
    });
  }

  function openQuery(resumeSessionId) {
    return sdk.query({
      prompt: mailbox.stream((message) => {
        consumedMessageIds.push(message.id);
        emit("consumed", {
          id: message.id,
          kind: message.kind,
          sessionId: state.sessionId,
        });
        return renderTeamMessage(message);
      }),
      options: {
        cwd: config.cwd,
        // Make the coordination identity an explicit part of the provider
        // subprocess contract. The SDK currently documents omitted `env` as
        // inheriting `process.env`, but the live canary showed that relying on
        // that implicit hop can leave the provider's Bash tool pointed at the
        // wrong Harness project. Rust injects only non-secret HARNESS_* values
        // into this runner; forwarding the complete runner environment also
        // preserves PATH, HOME, credentials, and provider configuration.
        env: { ...process.env },
        // Rust's NDJSON frame represents an omitted optional list as JSON
        // null. Agent SDK 0.3.220 expects either an array or `undefined` and
        // dereferences `.length`; forwarding null crashes before session bind.
        allowedTools: config.allowedTools ?? undefined,
        disallowedTools: config.disallowedTools ?? undefined,
        model: config.model ?? undefined,
        effort: config.effort ?? undefined,
        // `bypassPermissions`: an interactive permission prompt has nobody to
        // answer it inside an unattended member. It does not switch observation
        // hooks off, but it does NOT make this a sandbox and nothing here tries
        // to be one; see the header comment in `gates.mjs`.
        permissionMode: config.permissionMode ?? "bypassPermissions",
        // Members must discover the project's own CLAUDE.md and .claude/
        // skills from their execution root.
        settingSources: config.settingSources ?? ["project", "user"],
        resume: resumeSessionId ?? undefined,
        forkSession: config.forkSession ?? false,
        hooks,
      },
    });
  }

  return {
    state,
    mailbox,

    /**
     * Start the member. Resolves only when the mailbox is closed and the
     * provider stream drains — i.e. when the Host ends the member, not when a
     * turn happens to finish.
     */
    async start() {
      let resumeId = config.resumeSessionId ?? undefined;

      emit("member_started", {
        memberRunId: config.memberRunId,
        cwd: config.cwd,
        permissionMode: config.permissionMode ?? "bypassPermissions",
        ownedPathCount: (config.ownedPaths ?? []).length,
        resumed: Boolean(config.resumeSessionId),
      });

      // One iteration per query generation. Normally there is exactly one; an
      // interrupt retires the current query and the next iteration resumes the
      // same native session, so the MemberRun and its transcript continue.
      while (!mailbox.closed) {
        interruptedGeneration = false;
        query = openQuery(resumeId);

        try {
          for await (const message of query) {
            if (message.type === "system" && message.subtype === "init") {
              await bindSession(
                message.session_id,
                message.claude_code_version ?? null,
                message.model ?? null,
              );
              continue;
            }
            if (message.type === "assistant") {
              emit("assistant_message", {
                sessionId: state.sessionId,
                content: message.message?.content ?? message.content ?? null,
              });
              continue;
            }
            if (message.type === "result") {
              // A result ends a TURN. It does not end the member; the mailbox
              // decides that. This distinction is the entire fix.
              if (!state.sessionId && message.session_id) {
                await bindSession(
                  message.session_id,
                  message.claude_code_version ?? null,
                  message.model ?? null,
                );
              }
              emit("turn_complete", {
                sessionId: message.session_id ?? state.sessionId,
                subtype: message.subtype,
                triggerMessageId: consumedMessageIds.shift() ?? null,
                evidenceRefs: evidence.map((e) => e.ref),
                // `subtype` stays "success" on a provider API failure (live
                // probe, issue #293): the honest signal is `is_error` plus
                // `terminal_reason`/`api_error_status`. Forward them so Harness
                // never records a provider-down round as an ordinary success.
                isError: message.is_error === true,
                terminalReason: message.terminal_reason ?? null,
                apiErrorStatus: message.api_error_status ?? null,
              });
            }
          }
        } catch (error) {
          if (interruptedGeneration) {
            // A query torn down by our own interrupt is expected to end badly —
            // the observed failure is an `ede_diagnostic` result with
            // `stop_reason=null`. Anything else is a real fault.
            emit("query_ended_by_interrupt", {
              sessionId: state.sessionId,
              error: String(error).slice(0, 200),
            });
          } else if (mailbox.closed) {
            // The SDK re-throws the session's last error result when the input
            // stream ends ("Claude Code returned an error result: …"). The
            // Host already decided to end the member and the error round was
            // already reported via turn_complete(isError) — a clean Host close
            // must not be turned into a runner_error that loses member_closed.
            emit("query_ended_with_provider_error", {
              sessionId: state.sessionId,
              error: String(error).slice(0, 200),
            });
          } else {
            throw error;
          }
        }

        if (mailbox.closed) break;
        if (!interruptedGeneration) break; // stream ended for some other reason
        resumeId = state.sessionId;
        emit("member_resumed_after_interrupt", { sessionId: resumeId });
      }

      emit("member_closed", {
        sessionId: state.sessionId,
        reason: mailbox.closeReason,
        undelivered: mailbox.drain().map((m) => m.id),
        evidenceRefs: evidence.map((e) => e.ref),
      });
    },

    /** Deliver a durable Work or ordinary TeamMessage into the live session. */
    deliver(message) {
      mailbox.push(message);
      emit("delivered", { id: message.id, kind: message.kind });
    },

    /**
     * Real provider interrupt. Returns whatever the provider says survived,
     * which we surface rather than inventing an acknowledgement.
     */
    async interrupt() {
      if (!query) throw new Error("member not started");
      interruptedGeneration = true;
      const receipt = await query.interrupt();
      // The interrupted turn did not finish consuming its durable input. Do
      // not let that input id become the trigger of a later resumed turn.
      const abandonedTriggerMessageIds = consumedMessageIds.splice(0);
      // Retire this query's consumer, then end its iterator, so `start()`
      // leaves the for-await and opens a fresh query on the same session.
      // Without this the member hangs: the stream stops yielding but never
      // ends, so nothing detects that the query is spent.
      mailbox.supersede();
      try {
        await query.return?.();
      } catch {
        // already torn down
      }
      emit("interrupted", {
        stillQueued: receipt?.still_queued ?? null,
        abandonedTriggerMessageIds,
      });
      return receipt;
    },

    /** Steer: change permission posture mid-session. */
    async setPermissionMode(mode) {
      if (!query) throw new Error("member not started");
      await query.setPermissionMode(mode);
      emit("permission_mode_changed", { mode });
    },

    /** End the member. Host decision only — see Mailbox. */
    close(reason) {
      mailbox.close(reason);
    },
  };
}
