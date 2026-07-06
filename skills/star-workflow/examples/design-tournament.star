# DESIGN TOURNAMENT — divergent-then-convergent design, ported from the internal
# `workflow-layout-design` program, HARDENED against the real failure mode a Goal
# Workbench design run hit: too many structured fields for a creative output, no
# content-quality gate after extraction, no return_status/timeout on expensive
# leaves, one slow/failed leaf sinking the whole barrier, and --dry-run's own
# placeholder mock ("mock content") reading as a passing design.
#
#   understand  two parallel TYPED probes map the domain + the constraints (a
#               few short named fields is fine here — this is NOT the creative
#               output the quality gate below is guarding)
#   propose     THREE complete designs from orthogonal philosophies, each SEEDED
#               with the understanding injected forward verbatim (json.encode).
#               Each proposal is ONE long prose field (schema={"content":...}),
#               not many structured sub-fields — a creative document should not
#               be forced into a dozen typed boxes. return_status=True +
#               timeout_s bound each leaf; a failed/insufficient leaf gets ONE
#               retry before being dropped, so it degrades instead of sinking
#               the tournament.
#   synthesize  a judge grafts ONE winner, then a semantic quality gate decides
#               the run's verdict — NOT the judge's own opinion of itself.
#
# STRICT BY DEFAULT: under --dry-run (or any run where a leaf's content is
# actually the mock/placeholder shape), the quality gate FAILS and verdict()
# reports False with the concrete reason. The ONLY way a placeholder/mock
# result can pass is `--args '{"smoke": true}'`, which relaxes the semantic
# gates to a plumbing-only check and LOGS LOUDLY that the run is smoke-only —
# never treat a green smoke run as evidence of design quality.
#
# Read-only (it produces an artifact held in variables; nothing edits files).
# Run:  harness workflow run-script ./design-tournament.star \
#         --args '{"subject":"the read-only Workflows dashboard surface","read":"crates/harness-core/src/lib.rs + apps/agent-dashboard"}'
# Smoke/plumbing-only dry run (never treat this as a quality signal):
#       harness workflow run-script ./design-tournament.star --dry-run \
#         --args '{"subject":"...", "smoke": true}'

workflow(
    "design-tournament",
    "Map the domain + constraints with two typed probes, generate three complete " +
    "designs from orthogonal philosophies (each seeded with the understanding, " +
    "injected forward as JSON, as ONE long prose field rather than many typed " +
    "sub-fields), bound each expensive leaf with return_status+timeout_s and a " +
    "single retry-or-drop repair pass, then gate the synthesized winner on " +
    "concrete content-quality checks (length/headings/placeholder-content) so " +
    "--dry-run's own mock output cannot read as a passing design — divergent, " +
    "convergent, then verified.",
    budget_usd = 8.0,
    success_criterion = "one complete, implementable design that passes the semantic content gate (real length, required headings, no placeholder content) and names its winner rationale",
)

subject = args["subject"]
read = args["read"] if "read" in args else ""
read_clause = ("Read: " + read + ". ") if read else ""

# `smoke` defaults to False (strict). It is the ONLY switch that relaxes the
# semantic content gate to a plumbing-only check — set it for --dry-run smoke
# tests of the workflow's WIRING, never as a stand-in for real design review.
smoke = bool(args["smoke"]) if "smoke" in args else False
if smoke:
    log("SMOKE MODE: content-quality gates are RELAXED to plumbing-only checks. " +
        "This run proves the workflow WIRES TOGETHER, not that any design is " +
        "actually good. Never treat a green smoke run as design-quality evidence.")

PROPOSAL_TIMEOUT_S = args["proposal_timeout_s"] if "proposal_timeout_s" in args else 600
MIN_CONTENT_CHARS = 400
REQUIRED_HEADINGS = ["Structure", "Key Views", "States", "Tradeoffs"]
FORBIDDEN_MARKERS = ["mock content", "lorem ipsum", "TODO: fill in", "placeholder"]

# ---- typed contracts: the two UNDERSTAND probes stay multi-field (they map a
# domain, not a creative artifact) -----------------------------------
UNDERSTAND_DOMAIN = {
    "objects": "the core objects/data the design must express, with their exact fields + meaning",
    "lifecycle": "the state machines / flows the design must convey",
    "what_must_show": "the concrete information the design MUST surface, prioritized (a list)",
    "data_availability": "what data already exists vs is missing (so empty states are designed, not forgotten)",
}
UNDERSTAND_CONSTRAINTS = {
    "primitives": "the reusable building blocks/atoms a proposal should compose from",
    "patterns": "the established patterns to echo for consistency",
    "vocabulary": "the visual/structural vocabulary already in use",
    "hard_constraints": "constraints EVERY proposal MUST respect (a list)",
}

# The CREATIVE artifact (a proposal, and the final synthesis) is ONE long prose
# field. A judge/gate reading prose written for a human is more reliable than
# a dozen forced sub-fields — and it is what the content-quality gate below
# actually validates.
PROPOSAL_PROMPT_CONTRACT = """Structure your ENTIRE response as prose under these
exact markdown headings, in this order: "## Structure", "## Key Views",
"## States", "## Tradeoffs". Under "## Key Views" include CONCRETE ASCII
wireframes of the main views with labeled regions. Do not use placeholder text
anywhere — every section must contain real, specific content for this exact
subject."""

# ---- understand: two parallel typed probes -----------------------------------
phase("understand")
u = parallel([
    {
        "prompt": read_clause + "Deeply understand the DOMAIN for designing " + subject +
                  ". Output the precise semantics the design must convey: the objects + " +
                  "their fields, the lifecycle/flows, the prioritized list of what the " +
                  "design must surface, and what data exists vs is missing.",
        "schema": UNDERSTAND_DOMAIN,
        "label": "understand:domain",
    },
    {
        "prompt": read_clause + "Catalog the existing CONVENTIONS for " + subject +
                  " so a new design stays consistent. Output the reusable primitives to " +
                  "compose from, the patterns to echo, the vocabulary in use, and the hard " +
                  "constraints every proposal MUST respect.",
        "schema": UNDERSTAND_CONSTRAINTS,
        "label": "understand:constraints",
    },
])
domain = u[0] if type(u[0]) == "dict" else {}
constraints = u[1] if type(u[1]) == "dict" else {}
log("understanding done; running a 3-way tournament")

# Inject the FULL typed understanding forward — every proposal is built from it.
ctx = "DOMAIN:\n" + json.encode(domain) + "\n\nCONSTRAINTS:\n" + json.encode(constraints)

# ---- content-quality gate: min length, required headings, no placeholder ----
# This is the guard the Goal Workbench run was missing: schema extraction alone
# does not prove a creative output is USABLE. Applied to every proposal AND the
# final synthesis before verdict().
def content_quality_issues(content, label):
    issues = []
    if type(content) != "string":
        return ["%s: no content string extracted" % label]
    text = content.strip()
    if smoke:
        # Plumbing-only: just prove a non-empty string came back.
        if len(text) == 0:
            issues.append("%s: empty content (even under smoke mode)" % label)
        return issues
    if len(text) < MIN_CONTENT_CHARS:
        issues.append("%s: content too short (%d chars, need >= %d)" % (label, len(text), MIN_CONTENT_CHARS))
    lowered = text.lower()
    for marker in FORBIDDEN_MARKERS:
        if marker.lower() in lowered:
            issues.append("%s: contains forbidden placeholder marker %r" % (label, marker))
    for heading in REQUIRED_HEADINGS:
        if heading.lower() not in lowered:
            issues.append("%s: missing required heading %r" % (label, heading))
    return issues

# ---- propose: 3 complete designs from orthogonal philosophies, each bounded
# with return_status+timeout_s and gated on content quality with one retry ----
phase("propose")
philosophies = [
    {"key": "structure-first", "brief": "STRUCTURE-FIRST: make the core structure/shape the hero; optimize for 'see the whole thing and its live state at a glance'."},
    {"key": "consistency-first", "brief": "CONSISTENCY-FIRST: echo the dominant existing pattern as closely as possible; optimize for zero new vocabulary and existing muscle memory."},
    {"key": "narrative-first", "brief": "NARRATIVE-FIRST: read top-to-bottom like a report; optimize for calm readability with minimal new visual vocabulary."},
]

def proposal_prompt(p, repair_note):
    return (
        "Propose a COMPLETE design for " + subject + " following this philosophy:\n" +
        p["brief"] + "\n\nGround it in this understanding (reuse its primitives; " +
        "respect its hard_constraints):\n" + ctx + "\n\n" + PROPOSAL_PROMPT_CONTRACT +
        " State what your design sacrifices under \"## Tradeoffs\"." + repair_note
    )

raw_proposals = parallel([
    {
        "prompt": proposal_prompt(p, ""),
        "schema": {"content": "the full proposal, formatted per the heading contract"},
        "label": "propose:" + p["key"],
        "timeout_s": PROPOSAL_TIMEOUT_S,
        "return_status": True,
    }
    for p in philosophies
])

# Guarded degrade: a leaf that failed/timed out, or whose content fails the
# quality gate, gets exactly ONE repair retry before being dropped — one bad
# leaf must not sink the whole tournament (barrier-of-death anti-pattern).
proposals = []
dropped = []
for i in range(len(philosophies)):
    p = philosophies[i]
    status = raw_proposals[i]
    ok = type(status) == "dict" and status["ok"] == True and type(status["structured"]) == "dict"
    content = status["structured"]["content"] if ok else None
    issues = content_quality_issues(content, "propose:" + p["key"])
    if not issues:
        proposals.append({"key": p["key"], "content": content})
        continue

    reason = status["reason"] if (type(status) == "dict" and status["reason"] != None) else "content quality gate failed"
    log("propose:" + p["key"] + " needs repair — " + reason + "; issues: " + "; ".join(issues))
    repair_note = ("\n\nA PRIOR ATTEMPT FAILED THIS BAR: " + "; ".join(issues) +
                   ". Fix every issue explicitly.")
    retry_status = agent(
        proposal_prompt(p, repair_note),
        schema = {"content": "the full proposal, formatted per the heading contract"},
        label = "propose:" + p["key"] + ":retry",
        timeout_s = PROPOSAL_TIMEOUT_S,
        return_status = True,
    )
    retry_ok = type(retry_status) == "dict" and retry_status["ok"] == True and type(retry_status["structured"]) == "dict"
    retry_content = retry_status["structured"]["content"] if retry_ok else None
    retry_issues = content_quality_issues(retry_content, "propose:" + p["key"] + ":retry")
    if not retry_issues:
        proposals.append({"key": p["key"], "content": retry_content})
    else:
        dropped.append({"key": p["key"], "issues": issues + retry_issues})
        log("propose:" + p["key"] + " dropped after retry — " + "; ".join(retry_issues))

if len(proposals) == 0:
    output({
        "proposals_survived": 0,
        "dropped": [d["key"] for d in dropped],
        "operator_note": "every proposal failed the content-quality gate (or ran in smoke mode with no real content); no design to synthesize",
    })
    verdict(False, reason = "no proposal survived the content-quality gate after retry: " +
            "; ".join([d["key"] + " (" + "; ".join(d["issues"]) + ")" for d in dropped]))
else:
    # ---- synthesize: judge grafts ONE winner from whichever proposals survived
    phase("synthesize")
    proposals_json = json.encode(proposals)
    synth_status = agent(
        "You are the design lead. Here are the surviving proposals for " + subject + " " +
        "(some philosophies may be missing if they failed a quality bar — work with " +
        "what you have):\n" + proposals_json +
        "\n\nThey must serve this understanding:\n" + ctx +
        "\n\nScore them on: (1) faithfulness to the domain, (2) consistency with the " +
        "existing conventions, (3) scalability, (4) clarity, (5) graceful empty/loading " +
        "states. Then SYNTHESIZE ONE complete, opinionated final design — pick the " +
        "strongest base and graft the best ideas from the others. " + PROPOSAL_PROMPT_CONTRACT +
        " Under \"## Tradeoffs\" also name which proposal(s) won on which dimensions and " +
        "what was grafted from the runners-up.",
        schema = {"content": "the full synthesized design, formatted per the heading contract"},
        label = "synthesize",
        timeout_s = PROPOSAL_TIMEOUT_S,
        return_status = True,
    )
    synth_ok = type(synth_status) == "dict" and synth_status["ok"] == True and type(synth_status["structured"]) == "dict"
    final_content = synth_status["structured"]["content"] if synth_ok else None
    synth_issues = content_quality_issues(final_content, "synthesize")

    output({
        "proposals_survived": len(proposals),
        "dropped": [d["key"] for d in dropped],
        "final_design": final_content,
        "smoke": smoke,
    })

    if synth_issues:
        verdict(False, reason = "synthesized design failed the content-quality gate: " + "; ".join(synth_issues))
    else:
        verdict(True, reason = "synthesized one implementable design from " + str(len(proposals)) +
                " surviving proposal(s); passed length/heading/placeholder checks" +
                (" (SMOKE MODE — plumbing only, not a quality signal)" if smoke else ""))
