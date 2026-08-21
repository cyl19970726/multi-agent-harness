# DIRECT DOC EDIT - one serial leaf writes the selected project root on purpose.
#
# Use this only for small docs/config edits where the operator wants the current
# checkout changed now. Direct mode is not review-before-apply: the write has
# already landed in the working tree, and the runtime records direct_diff evidence.
#
# Run:
#   harness workflow run-script ./direct-doc-edit.star \
#     --args '{"target":"docs/workflow-runtime.md","request":"clarify direct write mode"}'

workflow(
    "direct-doc-edit",
    "Make one intentionally-direct docs/config edit in the selected clean git " +
    "checkout, then run a read-only review leaf and declare a verdict. This " +
    "demonstrates the simple serial direct-write path, not patch landing.",
    budget_usd = 3.0,
    success_criterion = "the requested target was edited directly and a reviewer accepted the result",
)

target = args["target"]
request = args["request"]

EDIT = {
    "summary": "one sentence describing the edit",
    "files_changed": "repo-relative files changed, one per line",
    "risk": "what could be wrong or needs operator attention",
}
REVIEW = {
    "ok": "bool",
    "reason": "why the direct edit satisfies or fails the request",
}

phase("edit")
edit = agent(
    """You are making a small direct edit to the selected repository checkout.

TARGET FILE: {target}
REQUEST: {request}

Constraints:
- Edit only the target file unless the request explicitly requires otherwise.
- Do not git commit, stage files, or run broad formatters.
- Keep the change minimal and reversible from normal git status/diff.

Report a summary, files_changed, and risk.""".format(target=target, request=request),
    provider = "codex",
    label = "direct-edit",
    writable = True,
    write_mode = "direct",
    schema = EDIT,
)

phase("review")
review = agent(
    """Review the direct edit for this request.

TARGET FILE: {target}
REQUEST: {request}
IMPLEMENTATION REPORT:
{edit}

Read the target file and decide whether the request is satisfied without
unrelated changes. Return ok=false if you cannot verify it.""".format(
        target=target,
        request=request,
        edit=json.encode(edit),
    ),
    provider = "codex",
    label = "review",
    schema = REVIEW,
)

output({"edit": edit, "review": review})
ok = type(review) == "dict" and review["ok"] == True
verdict(ok, reason = review["reason"] if type(review) == "dict" else "review produced no JSON")
