# SCAN -> PARALLEL FIX — the fan-out WIDTH is decided AT RUNTIME from a scan's
# output (a comprehension over its lines), which no static shape can express.
# A serial scan enumerates defects on the shared tree; then one writable fix per
# defect runs in its OWN worktree (so the parallel edits cannot collide), each
# fed the SHARED standards and the ONE defect it owns.
#
# Note the prompt shape this skill expects everywhere: a ROLE, an explicit
# what-to-READ, hard CONSTRAINTS, and an exact OUTPUT format — not a one-liner.
#
# Run:  harness workflow run-script ./scan-then-parallel-fix.star \
#         --args '{"area":"src/checkout","language":"TypeScript"}'

workflow(
    "scan-then-parallel-fix",
    "Scan once on the shared tree to enumerate concrete defects with locations, " +
    "then fan out one isolated-worktree fix per defect — the fan-out width is the " +
    "scan's defect count, decided at runtime — so the parallel fixes cannot collide.",
    budget_usd = 8.0,
)

area = args["area"]
language = args["language"] if "language" in args else "the project's language"

# ---- COMMON: the standards every fixer shares (the internal `COMMON` idiom) ---
COMMON = (
    "AREA: " + area + " (" + language + ").\n" +
    "STANDARDS (non-negotiable): make the SMALLEST change that fixes the defect; " +
    "do NOT refactor unrelated code; do NOT change public signatures or behavior " +
    "elsewhere; preserve existing tests; match the surrounding style."
)

# ---- typed contract for the scan --------------------------------------------
DEFECTS = {
    "defects": "every concrete defect, ONE PER LINE, each formatted exactly as " +
               "`<file>:<line> — <the specific defect and why it is wrong>`",
}

# ---- scan: a single grounded pass on the shared tree -------------------------
phase("scan")
scan = agent(
    "You are a meticulous code auditor.\n\n" +
    "READ every source file under " + area + ", plus the tests and config it " +
    "depends on. Build a precise mental model of the control flow before judging.\n\n" +
    "HUNT for REAL defects only (justifiable from the code, not style nits):\n" +
    "- correctness: off-by-one, wrong conditions, mishandled edge/empty cases, bad error paths\n" +
    "- robustness: unchecked failures, unhandled null/empty, resource leaks, swallowed errors\n" +
    "- contract: callers and callees that disagree on shape, type, or invariants\n\n" +
    "OUTPUT: emit each defect on its own line as `<file>:<line> — <defect + why>`. " +
    "No prose, no preamble, no speculation — only defects you can point at in the code.",
    provider = "codex",
    label = "scan",
    schema = DEFECTS,
)

# The scan's `defects` field is text (one per line); its length sets the fan-out.
defects = []
if type(scan) == "dict" and type(scan["defects"]) == "string":
    for line in scan["defects"].splitlines():
        line = line.strip()
        if line:
            defects.append(line)
log("scan found " + str(len(defects)) + " defects; fanning out one isolated fix each")

# ---- fix: one writable worker per defect, each in its own worktree -----------
phase("fix")
parallel([
    {
        "prompt": COMMON + "\n\nFix EXACTLY this one defect, nothing else:\n  " + defect +
                  "\n\nDeliverable: the minimal edit that resolves it. Then state in one " +
                  "line WHAT you changed and WHY it fixes the defect without affecting " +
                  "anything else. If the line turns out NOT to be a real defect on close " +
                  "reading, change nothing and say so.",
        "provider": "codex",
        "label": "fix",
        "writable": True,   # edits run in a throwaway worktree; the diff is the evidence
    }
    for defect in defects
])
