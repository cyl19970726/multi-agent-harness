# BUILD AND GATE — the WRITABLE engineering loop, shaped for this runtime.
#
# In this runtime a `writable=True` worker runs in its OWN throwaway worktree, so
# a separate implement step and a separate verify step do NOT share a tree. The
# faithful port of the internal design->implement->review->verify loop is therefore:
#
#   plan    a leading TYPED design (read-only), injected forward
#   build   ONE writable worker that implements it, runs the gate, and FIXES until
#           the gate is green or N attempts — the whole edit+gate+fix loop lives
#           inside the worker because only it can see its own worktree
#   verdict status keys off the gate result, not "the worker ran"
#
# Note the prompt shape: ROLE, the injected DESIGN as ground truth, hard
# CONSTRAINTS, numbered DELIVERABLES, the exact GATE command, and a report
# contract — the internal bar, not a one-liner.
#
# Run:  harness workflow run-script ./build-and-gate.star \
#   --args '{"task":"add a --json flag to the `report` command that prints the report as JSON","gate":"cargo test -p report-cli && cargo clippy --all-targets -- -D warnings"}'

workflow(
    "build-and-gate",
    "Produce a typed design, then ONE writable worker implements it in a throwaway " +
    "worktree and loops implement->run-the-gate->fix until the gate is green (bounded), " +
    "then a typed verdict keys the run's status off the gate result.",
    budget_usd = 12.0,
    success_criterion = "the declared gate command exits 0 after the change (gate_green == true)",
)

task = args["task"]
gate = args["gate"]

# ---- typed contracts ----------------------------------------------------------
DESIGN = {
    "approach": "2-3 sentences: how to implement the task with the smallest correct change",
    "files_to_touch": "the files/functions to change or add, one per line, with why",
    "test_plan": "the tests to add/extend to prove the change, one per line",
    "risks": "what could break or regress, one per line",
}
BUILD_RESULT = {
    "gate_green": "bool: true ONLY if the gate command exited 0 after your changes",
    "summary": "one line: what you implemented and the final gate status",
    "files_changed": "the files you created or modified, one per line",
    "blockers": "anything that blocked a green gate, one per line; empty if green",
}

# ---- plan: a typed design, read-only -----------------------------------------
# Long prompts read better as triple-quoted strings with .format() injection than
# as `+ "\n" +` concatenation; the body is flush-left so no indentation leaks in.
phase("plan")
design = agent(
    """You are the tech lead. Read the relevant code, then design the SMALLEST correct
implementation of this task:
  {task}

Produce: the approach, the exact files/functions to touch (with why), the test
plan that will prove it, and the risks. Do NOT implement anything yet — design it.""".format(task=task),
    label = "design",
    schema = DESIGN,
)
design_json = json.encode(design) if type(design) == "dict" else "{}"

# ---- build: ONE writable worker implements + gates + fixes until green --------
phase("build")
result = agent(
    """You are implementing a change in a throwaway git worktree. Do NOT git commit;
leave the changes in the working tree.

TASK: {task}

FOLLOW THIS DESIGN (verify it against the real files; correct it where the code
disagrees):
{design_json}

HARD CONSTRAINTS:
- Make the smallest correct change that satisfies the task; match the surrounding style.
- NEVER weaken, skip, or delete a test to make the gate pass — fix the real cause.
- Do NOT touch unrelated code or change public behavior outside the task's scope.

REQUIRED DELIVERABLES:
1. The implementation, per the design.
2. Tests covering the new behavior (do not weaken existing ones).

THEN GATE: run `{gate}`. If it fails, read the errors, FIX the ROOT CAUSE, and
re-run — up to 5 honest attempts. Never use --no-verify and never fake a pass.

Report: gate_green (true ONLY if the gate exited 0), a one-line summary, the files
you changed, and any blockers if it could not go green.""".format(
        task=task, design_json=design_json, gate=gate,
    ),
    label = "build",
    writable = True,          # edits + shell run in its own worktree; the diff is the evidence
    schema = BUILD_RESULT,
)

# ---- verdict: status reflects the GATE, not merely that the worker ran -------
green = type(result) == "dict" and result["gate_green"] == True
verdict(
    green,
    reason = result["summary"] if type(result) == "dict" else "build worker produced no result",
)
