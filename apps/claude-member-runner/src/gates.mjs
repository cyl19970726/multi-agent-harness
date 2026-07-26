/**
 * PreToolUse / PostToolUse gates.
 *
 * These turn three things that are currently advisory into contracts the
 * provider actually enforces, without Harness mirroring any provider state:
 *
 *   owned paths      ADR 0033 / review P2 — today `owned_paths` is a
 *                    coordination and acceptance boundary with no TeamRun-level
 *                    interception. A PreToolUse deny makes it real.
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
 * Deny writes outside the member's owned paths.
 *
 * Register with matcher `MUTATING_TOOLS`. `ownedPaths` is resolved against
 * `cwd`; an empty list means "no owned-path restriction" (current default
 * behaviour), which we keep so this gate is additive rather than a silent
 * tightening of existing runs.
 */
export function ownedPathsGate({ ownedPaths, cwd, onViolation }) {
  const roots = (ownedPaths ?? []).map((p) => resolve(cwd, p));
  return async function ownedPathsHook(input) {
    if (input.hook_event_name !== "PreToolUse") return {};
    if (roots.length === 0) return {};

    const filePath = input.tool_input?.file_path;
    if (typeof filePath !== "string" || filePath.length === 0) return {};

    const absolute = resolve(cwd, filePath);
    if (roots.some((root) => isInside(root, absolute))) return {};

    onViolation?.({ tool: input.tool_name, path: absolute, ownedPaths: roots });
    return denial(
      input.hook_event_name,
      `\`${filePath}\` is outside this member's owned paths ` +
        `(${roots.join(", ")}). Hand the change to the member that owns it ` +
        `instead of writing across the lane boundary.`,
    );
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
          ownedPathsGate({
            ownedPaths,
            cwd,
            onViolation: (v) => emit("owned_path_violation", v),
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
