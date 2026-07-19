# PENDING MANUAL REVIEW - preserve a code patch without blocking the workflow.
#
# This is the default-safe path when a workflow can create a patch but cannot
# prove enough evidence internally to apply or reject it. No apply_patch() /
# reject_patch() call is made for the pending case; the operator decides later.
#
# This is a standalone Dynamic Workflow: omitting apply_patch()/reject_patch()
# leaves the durable WorkflowPatch pending for an operator. An outer
# Mission/Wave may attach this WorkflowRun, its artifacts, and final result
# after completion without changing that pending-patch decision.
#
# Run:
#   harness workflow run-script ./pending-manual-review.star \
#     --args '{"task":"tighten validation for X","owned_paths":["src","tests"],"gate":"cargo test -q"}'

workflow(
    "pending-manual-review",
    "Create a durable WorkflowPatch from an isolated writable implementation, " +
    "review the worker's evidence, and deliberately leave the patch pending when " +
    "manual operator inspection is required instead of blocking or auto-applying.",
    budget_usd = 6.0,
    success_criterion = "the workflow finishes with a pending patch plus explicit operator apply/reject commands",
)

task = args["task"]
owned_paths = args["owned_paths"] if "owned_paths" in args else ["src", "tests"]
gate = args["gate"] if "gate" in args else "cargo test -q"
operator_approved = args["operator_approved"] if "operator_approved" in args else False

IMPLEMENT = {
    "gate_green": "bool",
    "summary": "one sentence: what changed and final gate status",
    "files_changed": "repo-relative files changed, one per line",
    "diff_review_notes": "compact notes from git diff/stat and important hunks",
    "evidence_complete": "bool: true only if the report has enough concrete evidence for an internal apply/reject decision",
    "blockers": "blockers, one per line; empty if none",
}
REVIEW = {
    "ok": "bool",
    "action": "one of: apply | reject | pending",
    "reason": "why this action is correct",
}

phase("implement")
impl = agent(
    """Implement the focused code change in an isolated throwaway worktree.
Do not commit, stage files, or touch unrelated files.

TASK:
{task}

OWNED PATHS:
{owned_paths}

Gate command:
{gate}

After editing, run the gate once. Inspect git diff/stat for the owned paths and
report whether your evidence is complete enough for a reviewer to apply/reject
without opening the patch manually.""".format(
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
    """You are deciding whether this WorkflowPatch can be resolved inside the workflow.

Task:
{task}

Implementation report:
{impl}

Rules:
- action=apply only if gate_green=true, changed files are inside owned paths,
  evidence_complete=true, and operator_approved=true.
- action=reject only if the report proves the patch is wrong or unsafe.
- action=pending when evidence is incomplete or human review is still required.

operator_approved={operator_approved}

Return pending rather than reject when the only problem is insufficient evidence.""".format(
        task=task,
        impl=json.encode(impl),
        operator_approved=str(operator_approved),
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
    apply_patch("implement", reason)
elif action == "reject":
    reject_patch("implement", reason)
else:
    action = "pending"

operator_commands = [
    "harness workflow patch list --run <run_id>",
    "harness workflow patch show <run_id> --step implement",
    "harness workflow patch apply <run_id> --step implement --actor <operator> --reason '<reason>'",
    "harness workflow patch reject <run_id> --step implement --actor <operator> --reason '<reason>'",
]

output({
    "implementation": impl,
    "review": review,
    "patch_action": action,
    "operator_commands": "\n".join(operator_commands),
    "note": "For action=pending the workflow intentionally makes no patch decision; completion is not blocked.",
})
verdict(True, reason = reason)
