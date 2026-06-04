# CANONICAL CLOSED-LOOP workflow — the skeleton a non-trivial workflow should
# follow. It demonstrates, in ONE program, every idiom the runtime supports:
#
#   C  leading typed PLAN, injected forward into later steps (json.encode bridge)
#   D  a shared, authoritative COMMON preamble every worker receives
#   A  a BOUNDED verify -> refine LOOP (not an open-ended retry) against a real bar
#   B  a SCHEMA-GATED BRANCH: control flow keys off a typed field, not prose
#   E  a typed VERDICT so the run's status means "intent met", not "workers ran"
#   +  a per-run budget_usd ceiling, a declared success_criterion, and a CHEAP
#      model on the read-only verify step (route cost to where it is needed).
#
# This loop is READ-ONLY: it produces and hardens an ARTIFACT (an analysis/answer
# held in a Starlark variable), so no worker edits files. For a WRITABLE build the
# shape differs — see the note at the bottom — because each writable worker runs in
# its OWN throwaway worktree and cannot share a tree with a separate verify step.

workflow(
    "closed-loop",
    "Plan the work (typed, injected forward), produce an artifact, then loop " +
    "verify->refine against a schema'd quality bar up to N times, and assert a " +
    "typed verdict so the run's status reflects intent — not merely that workers ran.",
    budget_usd = 5.0,
    success_criterion = "the artifact meets the declared quality bar (verify.passed == true)",
)

task = args["task"]   # what to produce/answer
bar = args["bar"]     # the quality bar the verifier judges against

# ---- D: one shared, authoritative context every worker receives --------------
COMMON = (
    "TASK: " + task + "\n" +
    "QUALITY BAR (definition of done): " + bar + "\n" +
    "Be precise and grounded; do not pad."
)

# ---- C: leading PLAN — a typed plan the rest of the run is built from ---------
phase("plan")
plan = agent(
    COMMON + "\n\nProduce a short plan for how to satisfy the task and bar.",
    label = "plan",
    schema = {"approach": "1-2 sentences", "steps": "list of strings", "risks": "list of strings"},
)
# Inject the plan FORWARD verbatim (json.encode serializes the dict for the prompt).
plan_json = json.encode(plan) if type(plan) == "dict" else "{}"

# ---- produce the first artifact, built from the plan -------------------------
phase("draft")
artifact = agent(
    COMMON + "\n\nProduce the artifact. Follow this plan:\n" + plan_json,
    label = "draft",
)

# ---- A + B: bounded verify -> refine loop against the schema'd bar ------------
phase("verify")
MAX_ATTEMPTS = 3
passed = False
for attempt in range(MAX_ATTEMPTS):
    # Read-only verify on a CHEAP model: it only judges against the bar.
    check = agent(
        COMMON + "\n\nJudge whether this artifact meets the bar. Be a strict grader; " +
        "set passed=false unless it clearly does.\n\nARTIFACT:\n" + artifact,
        label = "verify:" + str(attempt + 1),
        provider = "claude",
        model = "claude-haiku-4-5",
        schema = {"passed": "bool", "gaps": "list of concrete gaps, empty if it passes"},
    )
    # B: branch on the TYPED field, not on prose.
    if type(check) == "dict" and check["passed"] == True:
        passed = True
        break
    # Be defensive: a worker that ignored the schema may return a non-list; only
    # iterate when it is actually a list, else fall back to a single generic gap.
    raw_gaps = check["gaps"] if type(check) == "dict" else None
    gaps = raw_gaps if type(raw_gaps) == "list" else ["verifier returned no structured gaps"]
    log("attempt " + str(attempt + 1) + ": not yet — " + str(len(gaps)) + " gap(s); refining")
    # Refine the SAME artifact, fed the exact gaps to close.
    artifact = agent(
        COMMON + "\n\nRevise the artifact to close these gaps:\n- " + "\n- ".join(gaps) +
        "\n\nCURRENT ARTIFACT:\n" + artifact,
        label = "refine:" + str(attempt + 1),
    )

# ---- E: typed VERDICT — status reflects intent, not just step success --------
verdict(
    passed,
    reason = "bar met" if passed else "bar not met after " + str(MAX_ATTEMPTS) + " attempts",
)

# ==============================================================================
# Adapting this to a WRITABLE build (editing files + running a gate):
#
#   Each `writable=True` worker runs in its OWN throwaway worktree, so a separate
#   build step and verify step do NOT share a tree. So put the whole edit+gate+fix
#   loop INSIDE ONE writable worker, and keep the PLAN and the VERDICT in Starlark:
#
#     phase("plan");  plan = agent(..., schema={...})            # C: typed plan
#     phase("build")
#     result = agent(
#         COMMON + "\nPlan:\n" + json.encode(plan) +
#         "\nImplement it, then run `" + args["gate"] + "` and FIX until it exits 0 " +
#         "or you have tried 3 times. Report whether the gate is green.",
#         writable = True,                                       # edits in its worktree
#         schema = {"gate_green": "bool", "summary": "string"},
#     )
#     verdict(type(result) == "dict" and result["gate_green"] == True,   # B + E
#             reason = result["summary"] if type(result) == "dict" else "no result")
# ==============================================================================
