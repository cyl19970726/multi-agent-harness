# Scan, then fan out one fix per defect — the fan-out WIDTH is decided AT RUNTIME
# from the scan's output (a comprehension over its lines), which no static shape
# could express.
#
# Every workflow MUST declare a `workflow(name, design_intent)` header: the
# design_intent records WHY the program is shaped this way, so the run is
# auditable. A program without it (or with a blank / too-short intent) is rejected.
workflow(
    "scan-then-parallel-fix",
    "Scan once on the shared tree to enumerate defects, then fan out one isolated " +
    "worktree fix per defect so the parallel fixes cannot collide on the same files.",
)

phase("scan")
scan = agent("Scan " + args["area"] + " for defects. Return a numbered list, one per line, each with file path + one-line description.", provider = "codex")

phase("fix")
parallel([
    { "prompt": "Fix this defect in " + args["area"] + ": " + line + ". Make the minimal change and explain it.", "provider": "codex", "isolation": "worktree" }
    for line in scan.splitlines() if line
])
