# DESIGN TOURNAMENT — divergent-then-convergent design, ported from the internal
# `workflow-layout-design` program. The pattern that beats one-shot-iterated when
# the solution space is wide:
#
#   understand  two parallel TYPED probes map the domain + the constraints
#   propose     THREE complete designs from orthogonal philosophies, each SEEDED
#               with the understanding injected forward verbatim (json.encode)
#   synthesize  a judge scores them on NAMED dimensions and grafts ONE winner
#
# What makes this more than a toy fan-out: every handoff is a multi-field schema
# (so later steps read typed fields, not prose), the full understanding is
# injected into every proposal as ground truth, and the proposals are injected
# into the judge — the shape the real internal design runs use.
#
# Read-only (it produces an artifact held in variables; nothing edits files).
# Run:  harness workflow run-script ./design-tournament.star \
#         --args '{"subject":"the read-only Workflows dashboard surface","read":"crates/harness-core/src/lib.rs + apps/agent-dashboard"}'

workflow(
    "design-tournament",
    "Map the domain + constraints with two typed probes, generate three complete " +
    "designs from orthogonal philosophies (each seeded with the understanding, " +
    "injected forward as JSON), then a judge scores them on named dimensions and " +
    "synthesizes ONE design grafting the best of each — divergent-then-convergent.",
    budget_usd = 8.0,
    success_criterion = "one complete, implementable design that names its winner rationale and grafts the runners-up",
)

subject = args["subject"]
read = args["read"] if "read" in args else ""
read_clause = ("Read: " + read + ". ") if read else ""

# ---- typed contracts: every handoff carries named fields, not prose -----------
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
PROPOSAL = {
    "name": "the proposal name",
    "philosophy": "the core idea / what this design optimizes for",
    "structure": "overall composition + information hierarchy",
    "key_views": "each main view/region and exactly what it shows",
    "states": "empty / loading / error states",
    "wireframes": "CONCRETE ASCII wireframes of the main views, labeled regions",
    "tradeoffs": "what this design sacrifices",
}
SYNTHESIS = {
    "winner_rationale": "which proposal(s) won on which dimensions, and what was grafted from the runners-up",
    "final_design": "the complete chosen design: structure, views/regions, states, inline ASCII wireframes — concrete enough to implement directly",
    "open_questions": "anything left to decide (a list)",
}

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

# ---- propose: 3 complete designs from orthogonal philosophies -----------------
phase("propose")
philosophies = [
    {"key": "structure-first", "brief": "STRUCTURE-FIRST: make the core structure/shape the hero; optimize for 'see the whole thing and its live state at a glance'."},
    {"key": "consistency-first", "brief": "CONSISTENCY-FIRST: echo the dominant existing pattern as closely as possible; optimize for zero new vocabulary and existing muscle memory."},
    {"key": "narrative-first", "brief": "NARRATIVE-FIRST: read top-to-bottom like a report; optimize for calm readability with minimal new visual vocabulary."},
]
proposals = parallel([
    {
        "prompt": "Propose a COMPLETE design for " + subject + " following this philosophy:\n" +
                  p["brief"] + "\n\nGround it in this understanding (reuse its primitives; " +
                  "respect its hard_constraints):\n" + ctx + "\n\nCover every main view/region, " +
                  "all empty/loading/error states, and provide CONCRETE ASCII wireframes of the " +
                  "main views with labeled regions. State what your design sacrifices.",
        "schema": PROPOSAL,
        "label": "propose:" + p["key"],
    }
    for p in philosophies
])

# ---- synthesize: judge on named dimensions, graft one winner -----------------
phase("synthesize")
proposals_json = json.encode([p for p in proposals if type(p) == "dict"])
synthesis = agent(
    "You are the design lead. Here are the proposals for " + subject + ":\n" + proposals_json +
    "\n\nThey must serve this understanding:\n" + ctx +
    "\n\nScore them on: (1) faithfulness to the domain, (2) consistency with the existing " +
    "conventions, (3) scalability, (4) clarity, (5) graceful empty/loading states. Then " +
    "SYNTHESIZE ONE complete, opinionated final design — pick the strongest base and graft " +
    "the best ideas from the others. Be concrete enough to implement directly; include the " +
    "key ASCII wireframes inline.",
    schema = SYNTHESIS,
    label = "synthesize",
)

ok = type(synthesis) == "dict"
verdict(ok, reason = synthesis["winner_rationale"] if ok else "synthesis produced no structured result")
