/**
 * A persistent Claude Agent Team member.
 *
 * One runner == one `MemberRun` == one provider-native Claude session that
 * stays alive across many TeamMessages, many Host plan revisions, and (per
 * ADR 0037) across Waves — until the Host accepts its handoff.
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

/** Kinds that flip the plan gate open. See ADR 0038. */
const PLAN_APPROVAL_KIND = "plan_approval";

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

  const state = {
    sessionId: null,
    planRequired: Boolean(config.planRequired),
    planApproved: false,
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

  /**
   * Bind the native session to this MemberRun the first time we see it, and
   * register it in the provider's own session registry.
   *
   * `tagSession` is the member registry: Harness does not keep a second roster.
   * `listSessions()` filtered by this tag IS the list of a TeamRun's members,
   * which is why the tag encodes both ids.
   */
  async function bindSession(sessionId) {
    if (state.registered) return;
    state.sessionId = sessionId;
    state.registered = true;

    const tag = `${config.teamRunId}:${config.memberRunId}`;
    const title = `${config.memberName} · ${config.roleLabel ?? "member"}`;
    try {
      await sdk.tagSession(sessionId, tag, { dir: config.cwd });
      await sdk.renameSession(sessionId, title, { dir: config.cwd });
    } catch (error) {
      // Registration is a convenience for discovery and for humans opening the
      // session in Claude. It must not take the member down.
      emit("registry_write_failed", { sessionId, error: String(error) });
    }
    emit("session_bound", { sessionId, tag, title });
  }

  function noteDelivery(message) {
    if (message.kind === PLAN_APPROVAL_KIND) {
      state.planApproved = true;
      emit("plan_approved", { correlationId: message.correlation_id });
    }
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
      query = sdk.query({
        prompt: mailbox.stream(renderTeamMessage),
        options: {
          cwd: config.cwd,
          allowedTools: config.allowedTools,
          disallowedTools: config.disallowedTools,
          // `bypassPermissions`, matching what `claude_team_permission_mode()`
          // already sends on the `claude_cli` path. An interactive permission
          // prompt has nobody to answer it inside an unattended member, so
          // leaving that layer on only produces a deadlock.
          //
          // This does NOT disable the gates below. Verified live on 2026-07-27
          // with `scripts/gate-live.mjs`: under `bypassPermissions`, a Write
          // outside `ownedPaths` was denied by the PreToolUse hook and the file
          // was never created, while a Write inside `ownedPaths` succeeded.
          // `bypassPermissions` skips the prompt layer; hooks still run and a
          // hook `deny` still wins. Hooks — not permission mode — are the
          // enforcement boundary for a member.
          permissionMode: config.permissionMode ?? "bypassPermissions",
          // Members must discover the project's own CLAUDE.md and .claude/
          // skills from their execution root. This is the corner case raised
          // during dogfooding: a provider started outside the project loads the
          // wrong instructions while writing valid-looking rows to the right store.
          settingSources: config.settingSources ?? ["project", "user"],
          resume: config.resumeSessionId ?? undefined,
          forkSession: config.forkSession ?? false,
          hooks,
        },
      });

      const permissionMode = config.permissionMode ?? "bypassPermissions";
      const ownedPathCount = (config.ownedPaths ?? []).length;

      // The one genuinely unbounded combination: prompts off AND no owned-path
      // gate. Each half is reasonable alone — prompts are useless unattended,
      // and an empty `ownedPaths` is the documented "no restriction" value —
      // but together nothing constrains the member's writes. Say so out loud
      // rather than letting it be the quiet default.
      if (permissionMode === "bypassPermissions" && ownedPathCount === 0) {
        emit("unbounded_write_scope", {
          memberRunId: config.memberRunId,
          permissionMode,
          detail:
            "permission prompts are off and no ownedPaths are set, so no gate " +
            "constrains this member's writes; set ownedPaths to bound the lane",
        });
      }

      emit("member_started", {
        memberRunId: config.memberRunId,
        cwd: config.cwd,
        permissionMode,
        ownedPathCount,
        resumed: Boolean(config.resumeSessionId),
      });

      for await (const message of query) {
        if (message.type === "system" && message.subtype === "init") {
          await bindSession(message.session_id);
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
            await bindSession(message.session_id);
          }
          emit("turn_complete", {
            sessionId: message.session_id ?? state.sessionId,
            subtype: message.subtype,
            evidenceRefs: evidence.map((e) => e.ref),
          });
        }
      }

      emit("member_closed", {
        sessionId: state.sessionId,
        reason: mailbox.closeReason,
        undelivered: mailbox.drain().map((m) => m.id),
        evidenceRefs: evidence.map((e) => e.ref),
      });
    },

    /** Deliver a TeamMessage into the live session. */
    deliver(message) {
      noteDelivery(message);
      mailbox.push(message);
      emit("delivered", { id: message.id, kind: message.kind });
    },

    /**
     * Real provider interrupt. Returns whatever the provider says survived,
     * which we surface rather than inventing an acknowledgement.
     */
    async interrupt() {
      if (!query) throw new Error("member not started");
      const receipt = await query.interrupt();
      emit("interrupted", { stillQueued: receipt?.still_queued ?? null });
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
