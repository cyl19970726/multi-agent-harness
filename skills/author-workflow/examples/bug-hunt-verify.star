# Bug hunt with adversarial verification — a QUALITY workflow, not a naive
# fan-out. Three finders look for bugs from different angles (each returns a
# SCHEMA-D dict so we can read `res["findings"]` instead of parsing prose), then
# every candidate finding is cross-examined by a panel of skeptics that each try
# to REFUTE it. A finding survives only if a MAJORITY of skeptics fail to refute
# it. Finally we synthesize the confirmed set.
#
# Every workflow MUST declare a `workflow(name, design_intent)` header recording
# WHY it is shaped this way, so the run is auditable.
workflow(
    "bug-hunt-verify",
    "Fan out diverse bug-finders that emit schema-d findings, then adversarially " +
    "verify each candidate with a skeptic panel (majority must fail to refute) so " +
    "only cross-checked bugs survive, then synthesize the confirmed set.",
)

area = args["area"]

# ---- Phase 1: diverse finders, each returns a structured dict ----------------
phase("find")
finder_lenses = [
    "logic and off-by-one errors",
    "error handling and unchecked failures",
    "concurrency and shared-state races",
]
finds = parallel([
    {
        "prompt": "Hunt for " + lens + " in " + area + ". " +
                  "Report each concrete bug you can justify.",
        "provider": "codex",
        "label": "find:" + lens,
        "schema": {"findings": "list of strings"},
    }
    for lens in finder_lenses
])

# Flatten the finders' findings into one candidate list. A finder that produced
# no valid JSON comes back as a summary STRING, not a dict — skip those.
candidates = []
for res in finds:
    if type(res) == "dict" and type(res["findings"]) == "list":
        for finding in res["findings"]:
            candidates.append(finding)

log("collected " + str(len(candidates)) + " candidate findings")

# ---- Phase 2: adversarial verify — a skeptic panel per finding ---------------
# Each skeptic is prompted to REFUTE the finding and defaults to refuted=true
# when unsure, so a bug must actively survive scrutiny. We keep it only if a
# majority of skeptics do NOT refute it.
phase("verify")
SKEPTICS = 3
confirmed = []
for finding in candidates:
    panel = parallel([
        {
            "prompt": "You are a skeptical reviewer. Try to REFUTE this claimed bug " +
                      "in " + area + ": \"" + finding + "\". If you cannot clearly " +
                      "confirm it is a real bug, set refuted=true. Only set " +
                      "refuted=false when you are confident the bug is real.",
            "provider": "codex",
            "label": "skeptic",
            "schema": {"refuted": "bool", "reason": "string"},
        }
        for _ in range(SKEPTICS)
    ])

    # Default refuted=true when a skeptic gave no valid JSON (be conservative).
    refuted_votes = 0
    for verdict in panel:
        if type(verdict) == "dict" and verdict["refuted"] == False:
            refuted_votes += 0
        else:
            refuted_votes += 1

    # Survives only if a MAJORITY did NOT refute it.
    if refuted_votes * 2 < SKEPTICS:
        confirmed.append(finding)

log(str(len(confirmed)) + " of " + str(len(candidates)) + " findings survived the panel")

# ---- Phase 3: synthesize the confirmed set -----------------------------------
phase("synthesize")
agent(
    "Write a concise bug report for " + area + " from these CONFIRMED findings " +
    "(each survived an adversarial skeptic panel):\n- " +
    "\n- ".join(confirmed),
    provider="codex",
    label="synthesize",
)
