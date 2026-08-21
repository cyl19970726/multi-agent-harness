# FAILURE AWARE RETRY - branch on leaf failure status instead of prose.
#
# Use return_status=True when a workflow must distinguish "the verifier said no"
# from "the verifier timed out / failed / returned malformed JSON" and choose a
# fallback or abort path deliberately.
#
# Run:
#   harness workflow run-script ./failure-aware-retry.star \
#     --args '{"target":"checkout flow","primary_timeout_s":120}'

workflow(
    "failure-aware-retry",
    "Run a primary verifier with return_status=True, inspect ok/reason/detail " +
    "and structured output, then run a cheaper fallback only when the primary " +
    "leaf failed, timed out, or produced no passing structured result.",
    budget_usd = 4.0,
    success_criterion = "a verifier returns structured passed=true, or the run reports the concrete failed status",
)

target = args["target"] if "target" in args else "the requested target"
primary_timeout_s = args["primary_timeout_s"] if "primary_timeout_s" in args else 120

VERIFY = {
    "passed": "bool",
    "coverage": "what was checked, one line",
    "risk": "remaining risk or uncertainty, one line",
}

phase("primary")
primary = agent(
    """Verify this target with the normal thorough check:
{target}

Return passed=true only when the target meets the requested bar. If you cannot
inspect enough evidence, return passed=false and explain the risk.""".format(target=target),
    provider = "codex",
    label = "primary-verify",
    timeout_s = primary_timeout_s,
    schema = VERIFY,
    return_status = True,
)

primary_passed = (
    type(primary) == "dict" and
    primary["ok"] == True and
    primary["structured"] != None and
    type(primary["structured"]) == "dict" and
    primary["structured"]["passed"] == True
)

fallback = None
used_fallback = not primary_passed

if used_fallback:
    phase("fallback")
    fallback = agent(
        """The primary verifier did not produce a passing structured result.

Primary status:
{primary}

Run a narrower fallback check for this same target:
{target}

Return passed=true only if the fallback evidence is enough. Otherwise explain
the concrete remaining risk.""".format(
            primary=json.encode(primary),
            target=target,
        ),
        provider = "codex",
        label = "fallback-verify",
        timeout_s = 180,
        schema = VERIFY,
        return_status = True,
    )

final_status = fallback if used_fallback else primary
final_passed = (
    type(final_status) == "dict" and
    final_status["ok"] == True and
    final_status["structured"] != None and
    type(final_status["structured"]) == "dict" and
    final_status["structured"]["passed"] == True
)

reason = "verification passed"
if not final_passed:
    if type(final_status) == "dict":
        failure_reason = final_status["reason"] if final_status["reason"] != None else "domain-result-not-passing"
        failure_detail = final_status["detail"] if final_status["detail"] != None else json.encode(final_status["structured"])
        reason = "verification did not pass: " + failure_reason + " " + failure_detail
    else:
        reason = "verification did not return a status dict"

output({
    "primary": primary,
    "used_fallback": used_fallback,
    "fallback": fallback,
    "final_passed": final_passed,
    "operator_note": "return_status=True exposes ok/reason/detail/structured so the script can retry or abort intentionally.",
})
verdict(final_passed, reason = reason)
