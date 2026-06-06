# CANONICAL CLOSED-LOOP workflow — the skeleton a non-trivial READ-ONLY workflow
# should follow. It composes, in ONE program, every idiom the runtime supports:
#
#   C  a leading typed PLAN, injected forward into later steps (json.encode bridge)
#   D  a shared, authoritative COMMON preamble every worker receives
#   A  a BOUNDED verify -> refine LOOP (not an open-ended retry) against a real bar
#   B  a SCHEMA-GATED BRANCH: control flow keys off a typed field, not prose
#   E  a typed VERDICT so the run's status means "intent met", not "workers ran"
#   +  a per-run budget_usd ceiling, a declared success_criterion, and a CHEAP
#      model on the read-only verify step (route cost to where it is needed).
#
# It produces and hardens an ARTIFACT (an analysis/answer held in a variable), so
# no worker edits files. For a WRITABLE build, see build-and-gate.star — each
# writable worker runs in its OWN worktree, so the edit+gate+fix loop lives inside
# ONE writable worker while the PLAN and VERDICT stay in Starlark.
#
# Run:  harness workflow run-script ./closed-loop.star --max-budget-usd 5 \
#   --args '{"task":"explain how auth sessions are issued, validated, and revoked","bar":"covers issuance, validation, revocation, and every failure path, each grounded in a named code location"}'

workflow(
    "closed-loop",
    "Plan the work (typed, injected forward), produce an artifact, then loop " +
    "verify->refine against a schema'd quality bar up to N times, and assert a " +
    "typed verdict so the run's status reflects intent — not merely that workers ran.",
    budget_usd = 5.0,
    success_criterion = "the artifact meets the declared quality bar (verify.passed == true)",
)

task = args["task"]
bar = args["bar"]

# ---- typed contracts (defined up front, the internal idiom) -------------------
PLAN = {
    "approach": "1-2 sentences: how you will satisfy the task and the bar",
    "steps": "the concrete sub-steps, one per line",
    "risks": "what could make the artifact miss the bar, one per line",
}
CHECK = {
    "passed": "bool: true ONLY if the artifact clearly meets EVERY part of the bar",
    "gaps": "each concrete way it falls short of the bar, one per line; empty if it passes",
}

# ---- D: one shared, authoritative context every worker receives --------------
COMMON = (
    "TASK: " + task + "\n" +
    "QUALITY BAR (definition of done): " + bar + "\n" +
    "Be precise and grounded; cite specifics; do not pad or hedge."
)

# ---- C: leading PLAN — a typed plan the rest of the run is built from ---------
phase("plan")
plan = agent(
    COMMON + "\n\nYou are scoping the work. Produce a short, concrete PLAN to satisfy " +
    "the task and clear the bar: the approach, the sub-steps, and the risks that could " +
    "make the result fall short. Do not do the work yet — just plan it.",
    label = "plan",
    schema = PLAN,
)
# Inject the plan FORWARD verbatim (json.encode serializes the dict for the prompt).
plan_json = json.encode(plan) if type(plan) == "dict" else "{}"

# ---- produce the first artifact, built from the plan -------------------------
phase("draft")
artifact = agent(
    COMMON + "\n\nNow produce the artifact. Follow this PLAN exactly, covering every " +
    "step and pre-empting every risk it names:\n" + plan_json +
    "\n\nDeliverable: the complete artifact, grounded in specifics — nothing left as " +
    "a placeholder or 'TODO'.",
    label = "draft",
)

# ---- A + B: bounded verify -> refine loop against the schema'd bar ------------
phase("verify")
MAX_ATTEMPTS = 3
passed = False
for attempt in range(MAX_ATTEMPTS):
    # Read-only verify on a CHEAP model: it only judges against the bar.
    check = agent(
        COMMON + "\n\nYou are a STRICT grader. Judge this artifact against the bar, " +
        "clause by clause. Set passed=false unless it clearly satisfies EVERY part; " +
        "for each shortfall, name the exact gap. Do not be generous.\n\nARTIFACT:\n" + artifact,
        label = "verify:" + str(attempt + 1),
        provider = "claude",
        model = "claude-haiku-4-5",
        schema = CHECK,
    )
    # B: branch on the TYPED field, not on prose.
    if type(check) == "dict" and check["passed"] == True:
        passed = True
        break
    # The grader returns gaps as one-per-line text; split it, fall back if empty.
    raw = check["gaps"] if type(check) == "dict" else ""
    gaps = [g.strip() for g in raw.splitlines() if g.strip()] if type(raw) == "string" else []
    if not gaps:
        gaps = ["the grader reported no structured gaps; tighten grounding and coverage"]
    log("attempt " + str(attempt + 1) + ": not yet — " + str(len(gaps)) + " gap(s); refining")
    # Refine the SAME artifact, fed the EXACT gaps to close.
    artifact = agent(
        COMMON + "\n\nRevise the artifact to close these EXACT gaps and nothing else " +
        "(do not regress what already works):\n- " + "\n- ".join(gaps) +
        "\n\nCURRENT ARTIFACT:\n" + artifact,
        label = "refine:" + str(attempt + 1),
    )

# ---- the hardened artifact IS the answer — declare it as the run's result ----
# The calling agent reads `final_output.result` instead of the last refine step.
# (This artifact is free text, so it rode through the ~4000-char step cap; for a
# larger answer, have the final producer return a schema'd dict and output() that.)
output(artifact)

# ---- E: typed VERDICT — status reflects intent, not just step success --------
verdict(
    passed,
    reason = "bar met" if passed else "bar not met after " + str(MAX_ATTEMPTS) + " attempts",
)
