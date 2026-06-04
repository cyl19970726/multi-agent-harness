# BASELINE arm — one reviewer, no cross-checking. The naive control: a single
# pass is prone to over-reporting plausible-but-false bugs.
workflow(
    "bug-baseline",
    "Single-agent control: one reviewer lists the bugs it finds with no cross-checking — the naive baseline the workflow arm must beat on false-positive resistance.",
)

agent(
    "Review this Python module and list every REAL bug (not style, not nitpicks). "
    + "Be precise — do NOT report things that are not actually bugs.\n\n"
    + args["subject"],
    provider = "codex",
    label = "review",
    schema = {"findings": "list of short, concrete bug descriptions"},
)
