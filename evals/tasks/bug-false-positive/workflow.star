# WORKFLOW arm — diverse finders then an adversarial skeptic panel. Each
# candidate bug must survive cross-examination (a majority of skeptics must FAIL
# to refute it), so plausible-but-false bugs are filtered out before reporting.
#
# PERFORMANCE: the skeptic panels are FLATTENED into ONE parallel() wave over
# every (candidate, skeptic) pair, not a sequential loop of per-candidate
# barriers — so all skeptics run concurrently (up to the scheduler's width)
# instead of 3-at-a-time. Verdicts are regrouped by slicing.
workflow(
    "bug-verify",
    "Two finders with different lenses emit schema-d bug findings; every (candidate, skeptic) pair is cross-examined in ONE parallel wave by a 3-skeptic panel that defaults to refuted unless confident, so plausible-but-false bugs (e.g. 'sorted mutates the list') are rejected while the real crash survives; then the confirmed set is reported.",
)

subject = args["subject"]
SKEPTICS = 3

# ---- find: diverse finders, structured findings -----------------------------
phase("find")
finds = parallel([
    {
        "prompt": "Review this Python for " + lens + ". List concrete bugs only.\n\n" + subject,
        "provider": "codex",
        "label": "find",
        "schema": {"findings": "list of short, concrete bug descriptions"},
    }
    for lens in ["crashes and unhandled edge cases", "logic and correctness errors"]
])

candidates = []
for res in finds:
    if type(res) == "dict" and type(res["findings"]) == "list":
        for finding in res["findings"]:
            candidates.append(finding)
log("collected " + str(len(candidates)) + " candidate findings")

# ---- verify: ALL skeptics for ALL candidates in ONE parallel wave -----------
phase("verify")
jobs = []
for finding in candidates:
    for _ in range(SKEPTICS):
        jobs.append({
            "prompt": "You are a skeptical code reviewer. Try to REFUTE this claimed bug. "
                      + "If you cannot clearly confirm it is a REAL bug in the code, set "
                      + "refuted=true. Set refuted=false ONLY when you are confident the bug "
                      + "is real.\n\nCODE:\n" + subject + "\n\nCLAIMED BUG: " + finding,
            "provider": "codex",
            "label": "skeptic",
            "schema": {"refuted": "bool", "reason": "string"},
        })
verdicts = parallel(jobs)

# Regroup the flat verdict list back into per-candidate panels (SKEPTICS each,
# in order) and keep a candidate only if a MAJORITY did NOT refute it.
confirmed = []
for i in range(len(candidates)):
    panel = verdicts[i * SKEPTICS:(i + 1) * SKEPTICS]
    refuted_votes = 0
    for verdict in panel:
        if type(verdict) == "dict" and verdict["refuted"] == False:
            refuted_votes += 0
        else:
            refuted_votes += 1
    if refuted_votes * 2 < SKEPTICS:
        confirmed.append(candidates[i])
log(str(len(confirmed)) + " of " + str(len(candidates)) + " findings survived the panel")

# ---- report: echo the confirmed set as the final structured findings --------
phase("report")
agent(
    "Echo these CONFIRMED bugs back verbatim as the findings list. Do not add new "
    + "findings.\nCONFIRMED:\n- " + "\n- ".join(confirmed),
    provider = "codex",
    label = "report",
    schema = {"findings": "list of short, concrete bug descriptions"},
)
