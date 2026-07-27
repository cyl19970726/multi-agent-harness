/**
 * PreToolUse / PostToolUse hooks.
 *
 * **There is no containment boundary here, by design.** Members run at maximum
 * provider permission with the full tool set — Claude `bypassPermissions`,
 * Codex `danger-full-access`, Kimi's headless `-p` rejecting permission flags
 * outright — because a member has to build, test and use git, and an
 * interactive prompt has nobody to answer it.
 *
 * Given that, a hook that blocks `Write` while `echo >` walks past it is not a
 * boundary; it is a boundary-shaped thing people would trust. So the only
 * blocking hook here is the plan gate, which is a *sequencing* contract from
 * ADR 0038 (do not execute before the Host approves), not a safety one.
 * Everything else observes.
 *
 * If real containment is ever needed it has to come from the OS — a worktree
 * the member cannot escape, or a container — never from a PreToolUse matcher.
 *
 *   owned paths      ADR 0033 — a declared lane for coordination and
 *                    acceptance. **Observed, never enforced**: a cross-lane
 *                    write emits `cross_lane_write` and the write proceeds.
 *   plan approval    ADR 0038 — the native Goal must stay paused until a
 *                    correlated `plan_approval`. Denying mutating tools before
 *                    approval is that pause, enforced at the tool boundary.
 *   evidence refs    Issue #232 — handoff/review `evidence_refs` are empty.
 *                    PostToolUse observes which files a member actually wrote,
 *                    so the handoff can carry refs the member did not have to
 *                    remember to declare.
 *
 * Hook output contract (Agent SDK):
 *   allow  -> {}
 *   deny   -> { hookSpecificOutput: { hookEventName, permissionDecision: "deny",
 *                                     permissionDecisionReason } }
 * `deny` wins over every other decision when several hooks match.
 */

import { resolve, sep } from "node:path";

/** Tools that mutate the workspace. Matcher string and predicate must agree. */
export const MUTATING_TOOLS = "Write|Edit|NotebookEdit";

/**
 * True when `candidate` is inside `root`. Resolves both sides first so `..`,
 * a relative path, or a symlink-ish trick cannot escape the owned path.
 */
export function isInside(root, candidate) {
  const base = resolve(root);
  const target = resolve(candidate);
  return target === base || target.startsWith(base.endsWith(sep) ? base : base + sep);
}

function denial(hookEventName, reason) {
  return {
    hookSpecificOutput: {
      hookEventName,
      permissionDecision: "deny",
      permissionDecisionReason: reason,
    },
  };
}

/**
 * Observe writes outside the member's owned paths. **Never blocks.**
 *
 * This deliberately does not deny. Members run at maximum provider permission
 * with the full tool set, and a member that can be stopped by a matcher on
 * `Write` but not by `echo >` was never contained — it was only inconvenienced
 * in one of two directions. Half-enforcement is worse than none: it produces a
 * boundary people trust and shell walks through.
 *
 * What `owned_paths` is instead, and always was under ADR 0033: a declared lane
 * used for coordination and acceptance. Recording a cross-lane write gives the
 * Host something real to look at when reviewing a handoff — "this member edited
 * outside its lane, was that intended?" — which is the question that actually
 * matters. Blocking it would only push the same edit through Bash and hide it.
 */
export function ownedPathsObserver({ ownedPaths, cwd, onCrossLane }) {
  const roots = (ownedPaths ?? []).map((p) => resolve(cwd, p));
  return async function ownedPathsHook(input) {
    if (input.hook_event_name !== "PreToolUse") return {};
    if (roots.length === 0) return {};

    const filePath = input.tool_input?.file_path;
    if (typeof filePath !== "string" || filePath.length === 0) return {};

    const absolute = resolve(cwd, filePath);
    if (roots.some((root) => isInside(root, absolute))) return {};

    onCrossLane?.({ tool: input.tool_name, path: absolute, ownedPaths: roots });
    return {}; // observation only — the write proceeds
  };
}

/**
 * Hold execution until the Host has approved a plan for this Assignment.
 *
 * `state.planApproved` is flipped by the runner when a `plan_approval`
 * TeamMessage is delivered, so the gate reads live state rather than a value
 * captured at construction time.
 *
 * We deny rather than `defer`: `defer` ends the query, which is exactly the
 * batch-termination behaviour this runner exists to remove. Denying keeps the
 * member alive and tells it to submit a `plan_proposal` first.
 */
export function planApprovalGate({ state, onBlocked }) {
  return async function planApprovalHook(input) {
    if (input.hook_event_name !== "PreToolUse") return {};
    if (!state.planRequired || state.planApproved) return {};

    onBlocked?.({ tool: input.tool_name });
    return denial(
      input.hook_event_name,
      "Execution is gated on Host plan approval (ADR 0038). Submit your plan " +
        "as a `plan_proposal` and wait for a correlated `plan_approval` before " +
        "editing files. Reading and searching remain available.",
    );
  };
}

/**
 * Record which files the member actually changed, so a later handoff can carry
 * real `evidence_refs` instead of an empty array.
 *
 * Returns `{}` always — this observes, it never blocks.
 */
export function evidenceCollector({ collect }) {
  return async function evidenceHook(input) {
    if (input.hook_event_name !== "PostToolUse") return {};
    const filePath = input.tool_input?.file_path;
    if (typeof filePath === "string" && filePath.length > 0) {
      collect({ kind: "file", ref: filePath, tool: input.tool_name });
    }
    return {};
  };
}

/**
 * Assemble the `hooks` option for `query()`.
 *
 * `Stop` deliberately does NOT close the mailbox. A stop means "this turn is
 * over", not "this member is finished" — conflating the two is the batch bug.
 */
export function buildHooks({ state, cwd, ownedPaths, collect, emit }) {
  return {
    PreToolUse: [
      {
        matcher: MUTATING_TOOLS,
        hooks: [
          ownedPathsObserver({
            ownedPaths,
            cwd,
            onCrossLane: (v) => emit("cross_lane_write", v),
          }),
          planApprovalGate({
            state,
            onBlocked: (b) => emit("plan_gate_blocked", b),
          }),
        ],
      },
    ],
    PostToolUse: [{ hooks: [evidenceCollector({ collect })] }],
    Stop: [
      {
        hooks: [
          async () => {
            emit("turn_idle", { pending: state.pending() });
            return {};
          },
        ],
      },
    ],
  };
}
