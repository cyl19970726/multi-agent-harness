# Workflow Evaluation Harness

A system for answering the central question: **does multi-agent workflow
orchestration produce materially better outcomes than a single agent (baseline),
on which task types, at what cost — and is a given workflow actually exploiting
its structure, or just adding ceremony (being "naive")?**

This is a measurement system, not a demo. It must be reproducible, objective
where possible, variance-aware, and honest about cost.

## The comparison

Every eval task is run under at least two **arms**, both executed through the
*same* harness runtime (`harness workflow run-script`) so the only variable is
the orchestration structure:

| Arm | What it is | Role |
| --- | --- | --- |
| `baseline` | a single `agent(prompt)` (one subagent) | the naive control |
| `workflow` | an orchestration program (fan-out / verify / loop) | the treatment |

Same model and budget. Each arm is run `repeats` times — agents are
nondeterministic, so a single run proves nothing.

## Task categories

A task is tagged with the structural advantage it *should* exercise. A workflow
that wins on the right categories and does **not** waste cost on the control is
the bar.

- `cross-check-verify` — needs cross-examination; structure should resist false
  positives / catch what one pass misses.
- `parallel-breadth` — needs coverage; parallel finders should out-recall one.
- `iterate-refine` — needs iteration; loop-until-dry should converge better.
- `single-step-control` — a trivial task. **Negative control**: the workflow must
  NOT cost materially more for no quality gain. Detects ceremony.

## Task contract (`evals/tasks/<id>/`)

```
task.json      { id, title, category, repeats, prompt_ref?, grader, ground_truth }
subject.*      optional input the task reviews (passed to programs via args)
baseline.star  the single-agent arm
workflow.star  the orchestration arm
```

`evals/graders/<id>.mjs` exports `grade(arm, run, output) -> { score: 0..1, signals }`.

## Grading (three layers)

1. **Objective grader** (gold standard, where a task admits one): a programmatic
   check — does the produced patch pass tests? does the answer match ground
   truth? did it report the planted real bug AND reject the planted false one?
   Grades the **structured** findings (schema output) so it is deterministic.
2. **LLM judge** (for quality that resists a hard check): a Starlark judging
   program (dogfood), schema'd verdict `{score, reasoning}`, run **blind** (the
   judge is not told which arm produced the output), ideally a panel.
3. **Structural-value probes** — task-specific: did adversarial verify reject the
   false finding (false-positive resistance)? did parallel finders cover more
   (recall)? These directly test whether the structure *did its job*.

## Metrics & report

Per `(task, arm)` across repeats: mean quality, cost (tokens), wall-clock,
variance. Then:

- **win-rate**: how often `workflow` ≥ `baseline` per task / category.
- **cost-adjusted quality**: quality per 1k tokens.
- **structural-value**: e.g. false-positive rate, recall — does the structure
  deliver what it claims.
- **naive detection**: on `single-step-control`, the workflow's extra cost for no
  gain. A workflow that loses here is ceremony.

Output: `evals/report.json` + a markdown summary. Every run is recorded as
evidence (the harness's own model). For real repo tasks, the merged/closed PR is
the ground-truth outcome.

## Phasing

1. **Contract + runner + one objective-graded task** (the false-positive task) —
   prove the loop end to end.
2. **Graders**: objective + blind LLM-judge + structural probes.
3. **Task suite**: real tasks across all four categories incl. the control.
4. **Report + analysis**: win-rate, cost-adjusted, naive detection, variance.
5. **Dogfood**: run a workflow on a real repo task; PR-as-ground-truth.

Runner lives as `scripts/eval-workflows.mjs` (mirrors the acceptance scripts);
it may graduate to a first-class `harness eval` subcommand later.
