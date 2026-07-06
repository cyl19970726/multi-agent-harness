---
name: star-planner
description: "Use when decomposing an explored Goal into an executable plan in this harness: turn a written design_md (+acceptance_md) into an ordered list of sequential phases and, within each phase, a task DAG where tasks with disjoint owned_paths and no deps run in parallel. Covers `harness goal plan` (the agent-driven planner), the phase/task shape it produces, and how that plan feeds the phase→Starlark compiler and `goal run-phases` orchestrator."
---

# Author Planner

Once a Goal is `explored` — it has a real `design_md` (key problems first, then
the architecture) and a real `acceptance_md` — the next step is to **decompose it
into an executable plan**: a sequence of phases, each owning a DAG of tasks. This
skill is how you write that plan, and how the planner CLI writes it for you.

The plan is the bridge between a design (prose) and execution (workers). A good
plan is CUT FROM the design: each phase is a checkpoint the design implies, and
each task is a mini-goal with its own grounded slice of the design. A plan with
tidy-but-shallow phases yields deviated work, exactly like a shallow design does.

## The shape of a plan

A goal's plan is two append-only structures on the Goal:

| Structure | What it is |
| --- | --- |
| `phases[]` (`GoalPhase`) | The agent-planned, **sequential** checkpoints. A phase must pass its gate (steps ok + verdict + declared `outputs` present) before the next begins. |
| Tasks (`Task` with `phase_id`) | The per-phase DAG. Each task is a mini-goal: `design_md` + `acceptance_criteria` + `owned_paths` + `depends_on_task_ids` + `outputs`. |

A **phase** is `{ id, name, intent, acceptance, outputs, inputs, retry }`. A **task**
is `{ id, title, design_md, acceptance(list), owned_paths(list), depends_on(list of
task ids in the SAME phase), outputs }`. `phase_id` is the canonical task↔phase join
key (the goal is implied) — a task's `phase_id` must name a real phase of its goal.

### The two ordering rules (this is the whole model)

1. **Phases run SEQUENTIALLY.** Phase N+1 starts only after phase N's verdict
   passes. Put a hard checkpoint between stages of work that genuinely depend on
   each other (explore → build → verify). A simple goal may be **one phase**.
2. **Within a phase, tasks run in PARALLEL iff their `owned_paths` are disjoint
   AND neither depends on the other.** This is not advisory — it is literally how
   `compile_phase_to_starlark` groups the DAG: it layers tasks by longest
   dependency path, then within a layer greedily partitions into groups with
   pairwise-disjoint `owned_paths`. A group of >1 → `parallel([...])`; a singleton
   → `agent(...)`. Overlapping paths force serialization (one worker per file
   region, so concurrent writers never collide on disk).

So the way you GET parallelism is: keep tasks' `owned_paths` disjoint and avoid
unnecessary `depends_on`. The way you SERIALIZE is: add a `depends_on`, or let two
tasks share an owned path.

## Each task is a mini-goal

Do not write tasks as one-line chores. A task carries:

- **`design_md`** — a grounded SLICE of the goal's design for this task (what to
  change, where, before/after), the same way the goal's `design_md` grounds the
  whole. The compiler feeds this verbatim as the worker's prompt.
- **`acceptance`** — concrete, checkable criteria for THIS task.
- **`owned_paths`** — the files/dirs this task may write. This is load-bearing:
  it drives both parallelism (disjoint → parallel) and worktree isolation (a task
  with owned_paths compiles to `writable=True, isolation="worktree"`).
- **`depends_on`** — task ids IN THE SAME PHASE that must finish first. Cross-phase
  ordering is the phase sequence's job, not a task dep.
- **`outputs`** — the artifacts this task/phase commits to producing (an
  `ArtifactSpec` manifest): `{ id, kind, path?, purpose, required, acceptance? }`,
  `kind` ∈ {design_doc, adr, code, test_report, migration_doc, registered_doc,
  screenshot, other}. The compiler injects "you MUST produce these" into the worker
  prompt, and the gate **fails the phase if a `required` output's `path` is absent**
  (a `registered_doc` must also be in `docs/registry.json`). Empty `outputs` = today's
  behavior. Declare outputs so a phase can't "pass" having produced nothing.

A **phase** also takes `inputs` (artifacts it requires from a PRIOR phase — checked
before it runs, fail-fast if missing) and `retry` (per-phase replan budget,
overriding `--max-phase-retries`).

## The planner CLI (`harness goal plan`)

You do not have to hand-write phases one `goal phase-add` at a time. The planner
runs ONE worker that reads the goal's `design_md` + `acceptance_md` and returns a
structured decomposition, which the CLI persists as `phases[]` + Planned tasks.

```bash
# Plan an explored goal (real provider): design_md + acceptance_md → phases + tasks
harness goal plan <goal>

# Dry-run: exercises the full path with NO provider (the structured output is a
# mock object, so it plans NOTHING and reports that — used by tests/CI).
harness goal plan <goal> --dry-run
```

Mechanism (reuses the EXISTING execution path — no new provider seam):

1. `compile_planner_script` generates a tiny one-shot Starlark program:
   `workflow(...)`, then `out = agent("<planner prompt>", schema={...})`, then
   `output(out)`.
2. It runs through the SAME real-driver path `goal run-phases` uses, so it honors
   `--dry-run` and journals a `WorkflowRun` like any other.
3. The worker's structured reply lands under the run's `final_output.result`; the
   CLI parses it and appends new phases + creates Planned tasks (`goal_id` /
   `phase_id` / `design_md` / acceptance / owned_paths / depends_on).

**Idempotent-ish:** re-running `goal plan` skips phase/task ids that already exist
— it never duplicates them. So you can re-plan after a partial run to backfill
without clobbering live work.

**Dry-run plans nothing on purpose.** In `--dry-run` the structured result is the
harness's mock object (where `phases` is a placeholder string, not an array), so
the command degrades gracefully: it creates nothing and says so. A REAL plan needs
a real provider.

## From plan to execution

The plan is the truth; the `.star` is a derived, throwaway view:

```bash
# Compile ONE phase's task DAG into a Starlark workflow (inspect the parallelism)
harness phase compile <goal> --phase <phase-id>

# Run the whole plan: sequence phases, gate each on its verdict, advance the goal
harness goal run-phases <goal> [--dry-run]
harness goal run-phases <goal> --resume               # don't re-spend done work
harness goal run-phases <goal> --max-phase-retries 2  # replan budget per phase
```

`run-phases` walks `phases[]` in order, compiles each phase's live tasks, runs it,
and only advances past a phase when its acceptance verdict passes — the sequential
rule, enforced. See [[star-workflow]] for the Starlark runtime the compiled
phase runs on, and [[star-goal]] for getting a goal to `explored` first.

### Gating, replan, and resume (what the orchestrator does)

- **Gate.** A phase passes only if its run completed, **every task step is ok**, the
  compiled `verdict()` returned true (when the phase has an `acceptance`), **AND every
  `required` declared `output` exists** (a `registered_doc` must also be in
  `docs/registry.json`). Before a phase runs, its `inputs` preconditions are checked
  (fail-fast if a required upstream artifact is missing). On a pass the goal records a
  `Decision(decision_kind=phase_verdict)`, points `GoalPhase.verdict_decision_id` at it,
  writes each task → `Done`, and links each `WorkflowStep` to its task.
- **Land.** A passing phase **lands** its writable tasks' worktree diffs onto the branch
  (a per-phase landing commit + `GoalPhase.landed_commit`; clean-tree guard + rollback,
  never a force-merge) — so a passing phase leaves durable artifacts, not a dropped
  worktree. Sequential phases build on the prior phase's landed HEAD.
- **Replan.** On a failure with retries left (`GoalPhase.retry`, else
  `--max-phase-retries`), the orchestrator appends a `Knowledge` finding, asks the
  planner to **revise** this phase's task graph (dead tasks → `TaskStatus::Superseded`
  + `superseded_by_knowledge_id`; new tasks appended), recompiles, reruns. Tasks are
  *living*: superseded, never deleted, so the trail survives.
- **Resume.** `--resume` re-enters from the durable `GoalOrchestrationRun` checkpoint:
  passed phases are skipped, and a re-run phase replays its prior succeeded leaves (no
  re-spend). A kill mid-run is safe.
- **Auto-finalize.** The goal's stage is **derived** from its phases (else tasks) and
  re-synced on every completion seam — finishing the last phase advances the goal to
  `verified` (done) with no manual `goal stage`. `goal reconcile-phase` trues up a phase
  whose work shipped out-of-band; `goal finalize` closes a goal.

So the planner's output is not a one-shot script — it is a **living task graph** the
orchestrator edits (via replan) as execution surfaces new knowledge.

## Anti-patterns (reject these)

- **Phase soup.** Many tiny sequential phases where one phase with a parallel task
  group would do. Sequence only where work genuinely depends on a prior verdict.
- **Accidental serialization.** Tasks that could run in parallel but share an
  owned path (e.g. both claim the repo root) or carry a needless `depends_on`, so
  the compiler serializes them. Keep owned_paths tight and disjoint.
- **Chore tasks.** One-line tasks with no `design_md` / no `acceptance` /
  no `owned_paths`. A task is a mini-goal; give it the same grounding the goal got.
- **Planning before exploring.** Running `goal plan` on a goal whose `design_md` is
  thin or absent. The plan is cut from the design — fix the design first
  ([[star-goal]]), then plan.
- **Cross-phase task deps.** Using `depends_on` to reach into another phase.
  Ordering across phases is the phase sequence; `depends_on` is intra-phase only.
- **Outputs-less verify/doc phases.** A phase with no `required` `outputs` can "pass"
  having produced nothing (the gate only sees steps-ok + verdict). For a phase whose
  point IS a deliverable (a report, an ADR, a registered doc, a migration runbook),
  declare it as a `required` output so the gate enforces it.
- **Stale/loose `phase_id`.** A task whose `phase_id` doesn't name a real phase of its
  goal is rejected at create — don't hand-set `phase_id` to a phase that isn't there.
