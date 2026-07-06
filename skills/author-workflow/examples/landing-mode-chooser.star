# LANDING MODE CHOOSER - decide standalone run-script vs goal run-phases.
#
# The same Starlark runtime powers both surfaces, but landing differs:
# standalone run-script preserves writable diffs as pending WorkflowPatch rows;
# goal run-phases lands passing phase diffs via a per-phase commit.
#
# Run:
#   harness workflow run-script ./landing-mode-chooser.star \
#     --args '{"scenario":"one-off code fix that needs review before landing"}'

workflow(
    "landing-mode-chooser",
    "Classify whether a requested workflow should run as standalone run-script " +
    "with pending WorkflowPatch review, or through goal run-phases where passing " +
    "phase diffs land by the goal layer.",
    budget_usd = 2.0,
    success_criterion = "the output names the correct landing surface and the operator commands",
)

scenario = args["scenario"] if "scenario" in args else "one-off workflow work"

CHOICE = {
    "mode": "one of: run-script | goal-run-phases",
    "reason": "why this landing surface matches the scenario",
    "risk": "what can go wrong if the other surface is used",
}

phase("classify")
choice = agent(
    """Choose the right Dynamic Workflow landing surface for this scenario:
{scenario}

Facts:
- standalone `harness workflow run-script` runs a .star program directly; writable
  leaves create pending WorkflowPatch rows unless the script calls apply_patch or
  reject_patch after a review gate.
- `harness goal run-phases <goal>` compiles goal phases to the same runtime; a
  passing phase lands writable diffs via the goal layer's per-phase commit.

Return mode=run-script for ad-hoc exploration, review-before-apply, patch queues,
or artifact/report generation. Return mode=goal-run-phases when the work is an
accepted goal phase whose passing result should land automatically after the
phase gate.""".format(scenario=scenario),
    provider = "codex",
    label = "classify",
    schema = CHOICE,
)

mode = choice["mode"] if type(choice) == "dict" else "run-script"
commands = [
    "harness workflow run-script <prog.star> --args '<json>'",
    "harness workflow patch list --run <run_id>",
    "harness workflow patch show <run_id> --step <label>",
    "harness workflow patch apply <run_id> --step <label> --reason '<reviewed>'",
]
if mode == "goal-run-phases":
    commands = [
        "harness goal plan <goal>",
        "harness goal run-phases <goal> --resume",
        "harness goal learning-status --id <goal> --strict --require-evaluation",
    ]

output({
    "scenario": scenario,
    "choice": choice,
    "commands": "\n".join(commands),
    "landing_rule": "run-script leaves patches pending unless explicitly applied/rejected; goal run-phases lands passing phase diffs.",
})
verdict(type(choice) == "dict", reason = choice["reason"] if type(choice) == "dict" else "classifier produced no JSON")
