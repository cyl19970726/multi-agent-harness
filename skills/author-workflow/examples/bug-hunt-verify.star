# BUG HUNT with adversarial verification — a QUALITY review, not a naive fan-out.
# Diverse finders look from orthogonal lenses; every candidate is then cross-
# examined by a skeptic PANEL that each tries to REFUTE it (defaulting to refuted
# when unsure), and a finding survives only if a MAJORITY fail to refute it. The
# survivors are synthesized into a report. This is the review-harness shape the
# internal `multica-layout-review` run uses.
#
# Note on schema fields: a `schema` field is natively enforced as a STRING, so a
# finder returns its findings as ONE-PER-LINE TEXT and we `.splitlines()` it — a
# robust way to get a list of items out of a structured leaf.
#
# Read-only. Run:  harness workflow run-script ./bug-hunt-verify.star \
#   --args '{"area":"the checkout flow in src/checkout"}'

workflow(
    "bug-hunt-verify",
    "Fan out diverse bug-finders from orthogonal lenses, then adversarially verify " +
    "EACH candidate with a skeptic panel (a majority must fail to refute) so only " +
    "cross-checked bugs survive, then synthesize the confirmed set — survival-of-" +
    "scrutiny, not a single rubber-stamp.",
    budget_usd = 6.0,
    success_criterion = "every reported bug survived an adversarial skeptic panel",
)

area = args["area"]

# ---- find: diverse finders, each returns its findings as one-per-line text ----
phase("find")
lenses = [
    "logic and off-by-one errors",
    "error handling and unchecked failures",
    "concurrency and shared-state races",
]
finds = parallel([
    {
        "prompt": "Hunt for " + lens + " in " + area + ". Report ONLY concrete bugs you " +
                  "can justify, ONE PER LINE, each naming the location and the defect. " +
                  "No prose, no preamble.",
        "provider": "codex",
        "label": "find:" + lens,
        "schema": {"findings": "the concrete bugs, one per line"},
    }
    for lens in lenses
])

# Flatten: each finder returns a dict whose `findings` is newline-separated text.
candidates = []
for res in finds:
    if type(res) == "dict" and type(res["findings"]) == "string":
        for line in res["findings"].splitlines():
            line = line.strip()
            if line:
                candidates.append(line)
log("collected " + str(len(candidates)) + " candidate findings")

# ---- verify: a skeptic panel per finding, default-refute ----------------------
# Each skeptic must ACTIVELY confirm a real bug or it counts as refuted, so a
# finding only survives if a MAJORITY of the panel did NOT refute it.
phase("verify")
SKEPTICS = 3
confirmed = []
for finding in candidates:
    panel = parallel([
        {
            "prompt": "You are a skeptical reviewer of " + area + ". Try to REFUTE this " +
                      "claimed bug: \"" + finding + "\". Set refuted=true unless you can " +
                      "clearly confirm it is a REAL bug in the actual code.",
            "provider": "codex",
            "label": "skeptic",
            "schema": {"refuted": "bool", "reason": "one sentence"},
        }
        for _ in range(SKEPTICS)
    ])
    # A missing / non-dict vote counts as refuted (conservative).
    refuted = 0
    for v in panel:
        if not (type(v) == "dict" and v["refuted"] == False):
            refuted += 1
    if refuted * 2 < SKEPTICS:   # majority did NOT refute
        confirmed.append(finding)
log(str(len(confirmed)) + " of " + str(len(candidates)) + " findings survived the panel")

# ---- synthesize: a report from the CONFIRMED set ------------------------------
phase("synthesize")
report = agent(
    "Write a concise, prioritized bug report for " + area + " from these CONFIRMED " +
    "findings (each survived an adversarial skeptic panel). Group by severity.\n\n" +
    "CONFIRMED:\n- " + "\n- ".join(confirmed),
    provider = "codex",
    label = "synthesize",
    schema = {"summary": "2-3 sentence overall", "must_fix": "the blocking bugs, one per line"},
)

# Status reflects intent: a report with no confirmed bugs is still a successful
# review (nothing survived scrutiny) — verdict on whether the review COMPLETED.
verdict(type(report) == "dict", reason = "review complete; " + str(len(confirmed)) + " confirmed")
