# PATCH REVIEW APPLY - default standalone code-development landing path.
#
# Implement in a throwaway worktree, capture the diff as a pending WorkflowPatch,
# review the worker's structured diff/gate summary, then declare apply/reject or
# leave it pending for manual `workflow patch show`. The CLI performs apply/reject
# after the run journals patch rows; no action means the patch remains pending.
#
# This standalone-run shape assumes no orchestration. Under `goal run-phases`
# (this script attached to a workflow-mode phase via workflow_ref), no
# WorkflowPatch rows are created at all: apply_patch()/reject_patch() become
# landing INTENTS (apply = no-op beyond the audit trail, reject = exclude this
# step's diff), and phase landing is the sole landing authority.
#
# Run:
#   harness workflow run-script ./patch-review-apply.star \
#     --args '{"task":"add focused behavior X","owned_paths":["src","tests"],"gate":"cargo test -q"}'

workflow(
    "patch-review-apply",
    "Implement code in an isolated worktree, preserve the diff as a WorkflowPatch, " +
    "review structured evidence, and let the workflow declare apply_patch, " +
    "reject_patch, or no action for pending manual review.",
    budget_usd = 8.0,
    success_criterion = "the patch is applied only when accepted, rejected only when bad, otherwise left pending",
)

task = args["task"]
owned_paths = args["owned_paths"] if "owned_paths" in args else ["src", "tests"]
gate = args["gate"] if "gate" in args else "cargo test -q"

IMPLEMENT = {
    "gate_green": "bool",
    "summary": "one sentence: what changed and final gate status",
    "files_changed": "repo-relative files changed, one per line",
    "diff_review_notes": "compact notes from git diff/stat and important hunks",
    "blockers": "blockers, one per line; empty if none",
}
REVIEW = {
    "ok": "bool",
    "action": "one of: apply | reject | pending",
    "reason": "why this action is correct",
}

phase("implement")
impl = agent(
    """You are implementing a focused code change in an isolated throwaway worktree.
Do not commit, stage files, or change unrelated files.

TASK:
{task}

OWNED PATHS:
{owned_paths}

Gate command:
{gate}

Steps:
1. Implement the smallest correct change.
2. Add or update tests only where needed.
3. Run the gate command. If it fails, fix the root cause and rerun once.
4. Before reporting, inspect git diff/stat for the owned paths and include compact
   diff_review_notes with the important changed files/hunks.

Return gate_green, summary, files_changed, diff_review_notes, and blockers.""".format(
        task=task,
        owned_paths=json.encode(owned_paths),
        gate=gate,
    ),
    provider = "codex",
    label = "implement",
    writable = True,
    persist_changes = "patch",
    owned_paths = owned_paths,
    schema = IMPLEMENT,
)

phase("review")
review = agent(
    """You are the patch gate. Decide whether the implementation should be applied.

Original task:
{task}

Worker evidence:
{impl}

Choose exactly one action:
- apply: gate_green is true, changed files stay inside owned paths, and the
  diff_review_notes give enough concrete evidence.
- reject: the patch is wrong, unsafe, or outside scope.
- pending: evidence is insufficient; leave the WorkflowPatch pending for manual
  `harness workflow patch show` review.

Set ok=true only for apply. Set ok=false for reject or pending.""".format(
        task=task,
        impl=json.encode(impl),
    ),
    provider = "codex",
    label = "review",
    schema = REVIEW,
)

if type(review) == "dict":
    action = review["action"]
    reason = review["reason"]
else:
    action = "pending"
    reason = "review produced no JSON; leave patch pending for manual review"

if action == "apply":
    apply_patch("implement", review["reason"])
elif action == "reject":
    reject_patch("implement", reason)
else:
    action = "pending"

output({"implementation": impl, "review": review, "patch_action": action})
verdict(type(review) == "dict", reason = reason)
