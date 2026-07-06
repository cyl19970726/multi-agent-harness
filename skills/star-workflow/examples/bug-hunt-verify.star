# BUG HUNT with adversarial verification — a QUALITY review, the shape the internal
# `multica-layout-review` run uses. Diverse finders hunt from orthogonal lenses;
# every candidate is then cross-examined by a skeptic PANEL that each tries to
# REFUTE it (defaulting to refuted when unsure); a finding survives only if a
# MAJORITY fail to refute it; the survivors are synthesized into a triaged report.
#
# Note the prompt shape: every leaf has a ROLE, an explicit what-to-READ, the
# categories to HUNT, and an exact OUTPUT format — not a one-liner. A `schema`
# field is enforced as a STRING, so list-valued returns come back one-per-line
# and are `.splitlines()`-ed.
#
# Run:  harness workflow run-script ./bug-hunt-verify.star \
#   --args '{"area":"the order-pricing module in src/pricing"}'

workflow(
    "bug-hunt-verify",
    "Fan out diverse bug-finders from orthogonal lenses, then adversarially verify " +
    "EACH candidate with a skeptic panel (a majority must fail to refute) so only " +
    "cross-checked bugs survive, then synthesize a triaged report — survival-of-" +
    "scrutiny, not a single rubber-stamp.",
    budget_usd = 8.0,
    success_criterion = "every reported bug survived an adversarial skeptic panel",
)

area = args["area"]

# ---- COMMON: the shared frame every worker receives --------------------------
COMMON = (
    "AREA UNDER REVIEW: " + area + ".\n" +
    "Ground EVERY claim in the actual code — name the file and the line. A 'bug' is " +
    "behavior that is wrong, unsafe, or contract-violating; it is NOT a style nit, a " +
    "naming preference, or a hypothetical. Do not speculate."
)

# ---- typed contracts ---------------------------------------------------------
FINDINGS = {
    "findings": "each concrete bug you can justify, ONE PER LINE, formatted as " +
                "`<file>:<line> — <the bug and the concrete failure it causes>`",
}
SKEPTIC = {
    "refuted": "bool: true unless you can clearly confirm this is a REAL bug in the actual code",
    "reason": "one sentence: why it is (not) a real bug, citing the code",
}
REPORT = {
    "summary": "2-3 sentence overall verdict on the area's correctness",
    "must_fix": "the blocking bugs that should gate a merge, one per line",
    "should_fix": "the non-blocking but real bugs, one per line",
}

# ---- find: diverse finders from orthogonal lenses ----------------------------
phase("find")
lenses = [
    {"key": "logic", "what": "logic and off-by-one errors: wrong conditions, boundary mistakes, mishandled empty/edge cases, incorrect ordering or rounding"},
    {"key": "failure", "what": "error handling: unchecked failures, swallowed errors, unhandled null/empty, partial writes, resources never released"},
    {"key": "concurrency", "what": "concurrency and shared state: races, unguarded mutation, deadlocks, ordering assumptions that don't hold under interleaving"},
]
finds = parallel([
    {
        "prompt": COMMON + "\n\nYou are a specialist bug-finder. READ all of " + area +
                  " (and the code it calls into) and HUNT specifically for " + lens["what"] +
                  ".\n\nReport ONLY concrete, justifiable bugs — each on its own line as " +
                  "`<file>:<line> — <bug + the failure it causes>`. No prose, no preamble.",
        "provider": "codex",
        "label": "find:" + lens["key"],
        "schema": FINDINGS,
    }
    for lens in lenses
])

# Flatten the finders' one-per-line findings into a candidate list.
candidates = []
for res in finds:
    if type(res) == "dict" and type(res["findings"]) == "string":
        for line in res["findings"].splitlines():
            line = line.strip()
            if line:
                candidates.append(line)
log("collected " + str(len(candidates)) + " candidate findings")

# ---- verify: a skeptic panel per finding, default-refute ---------------------
phase("verify")
SKEPTICS = 3
confirmed = []
for finding in candidates:
    panel = parallel([
        {
            "prompt": COMMON + "\n\nYou are a SKEPTICAL reviewer whose job is to REFUTE " +
                      "weak bug reports. Try hard to refute this claimed bug:\n  \"" + finding +
                      "\"\n\nCheck the ACTUAL code: does the failure really occur, on a reachable " +
                      "path, given the real types and guards? Set refuted=true unless you can " +
                      "clearly confirm it is a real bug; set refuted=false ONLY when you are " +
                      "confident, citing the line.",
            "provider": "codex",
            "label": "skeptic",
            "schema": SKEPTIC,
        }
        for _ in range(SKEPTICS)
    ])
    # A missing / non-dict / non-false vote counts as refuted (conservative).
    refuted = 0
    for v in panel:
        if not (type(v) == "dict" and v["refuted"] == False):
            refuted += 1
    if refuted * 2 < SKEPTICS:   # a MAJORITY did NOT refute it
        confirmed.append(finding)
log(str(len(confirmed)) + " of " + str(len(candidates)) + " findings survived the panel")

# ---- synthesize: a triaged report from the CONFIRMED set ---------------------
phase("synthesize")
report = agent(
    COMMON + "\n\nYou are the reviewer of record. Write a tight, triaged bug report from " +
    "these CONFIRMED findings — each already survived an adversarial skeptic panel, so do " +
    "NOT re-litigate them; classify each as must_fix (blocks merge) or should_fix.\n\n" +
    "CONFIRMED FINDINGS:\n- " + "\n- ".join(confirmed),
    provider = "codex",
    label = "synthesize",
    schema = REPORT,
)

# The triaged report IS the run's answer — declare it as the result so the calling
# agent reads `final_output.result` directly (it is schema'd, so carried uncapped),
# instead of digging it out of the synthesize step by label.
output(report)

# Status reflects intent: a completed review with zero confirmed bugs is still a
# success (nothing survived scrutiny). The verdict is on whether the review ran.
verdict(type(report) == "dict", reason = "review complete; " + str(len(confirmed)) + " bug(s) confirmed")
