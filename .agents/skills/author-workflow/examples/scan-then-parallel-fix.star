# The Starlark twin of scan-then-parallel-fix.json — fan-out width is
# determined AT RUNTIME from the scan's output (a comprehension), which the flat JSON-IR cannot express.
phase("scan")
scan = agent("Scan " + args["area"] + " for defects. Return a numbered list, one per line, each with file path + one-line description.", provider = "codex")

phase("fix")
parallel([
    { "prompt": "Fix this defect in " + args["area"] + ": " + line + ". Make the minimal change and explain it.", "provider": "codex", "isolation": "worktree" }
    for line in scan.splitlines() if line
])
