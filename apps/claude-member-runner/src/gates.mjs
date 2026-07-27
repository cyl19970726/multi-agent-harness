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
 * boundary; it is a boundary-shaped thing people would trust. Everything here
 * therefore observes. Planning is ordinary correlated conversation: the Host
 * can ask a member to return a Markdown plan before execution, then reply with
 * revisions or permission to continue. It is not a provider mode or tool gate.
 *
 * If real containment is ever needed it has to come from the OS — a worktree
 * the member cannot escape, or a container — never from a PreToolUse matcher.
 *
 *   owned paths      ADR 0033 — a declared lane for coordination and
 *                    acceptance. **Observed, never enforced**: a cross-lane
 *                    write emits `cross_lane_write` and the write proceeds.
 *   evidence refs    Issue #232 — handoff/review `evidence_refs` are empty.
 *                    PostToolUse observes which files a member actually wrote,
 *                    so the handoff can carry refs the member did not have to
 *                    remember to declare.
 *
 * Hook output contract (Agent SDK): observers always return `{}`.
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
