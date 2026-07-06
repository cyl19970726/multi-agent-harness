# ADR 0024: GoalPhase Execution Modes

## Context

`GoalPhase` started as a task graph checkpoint: the planner created tasks, the
compiler converted those tasks into a Starlark workflow, and `goal run-phases`
used task status, messages, and evidence as the operational truth.

That is still the right model for durable `AgentMember` assignment, message-first
execution, and task-level review. But some phases are naturally authored as a
workflow from the beginning: a Starlark program already expresses the plan, the
fan-out, the gates, and the evidence stream. For those phases, forcing an extra
task graph duplicates the plan and makes the compiler harder to reason about.

## Decision

Each `GoalPhase` chooses exactly one primary executor:

- `execution_mode = "task_graph"`: the default. The phase owns tasks and
  `compile_phase_to_starlark` turns those tasks into the executable workflow.
- `execution_mode = "workflow"`: the phase has no tasks and runs the authored
  Starlark program named by `workflow_ref`.

`workflow_ref` supports:

- `repo:path/to/workflow.star` for user-authored repository workflows.
- `builtin:<id>` for shipped building phases.

The CLI rejects mixed phases:

- workflow-mode phases must set `workflow_ref`;
- task-graph phases must not set `workflow_ref`;
- workflow-mode planner phases must not include tasks;
- repo workflow refs must be safe repo-relative `.star` paths.

## Consequences

The product keeps both long-term execution styles:

- Use `task_graph` when the phase should allocate durable tasks to persistent
  agents and preserve message/report/review proof per task.
- Use `workflow` when the Starlark program is the plan and the runtime truth is
  `WorkflowRun -> WorkflowStep -> Evidence`.

Dashboard rendering should branch on the same field. A task-graph phase shows
the task DAG and task detail. A workflow phase shows the phase plan/spec, the
compiled/authored workflow, live execution, and results/evidence. It should not
invent a task graph just for display.

Existing built-in building phases remain compatible: old records with
`kind=Building` and `builtin=<id>` can still compile even if they do not have a
new `workflow_ref` field.

## Consequences

Because both modes run under the same `goal run-phases` orchestrator, both are
ORCHESTRATED runs with one landing authority: per-phase landing. Neither mode
persists `WorkflowPatch` rows; an authored `workflow`-mode script's
`apply_patch()` is journaled intent only, and `reject_patch()` /
`persist_changes="discard"` exclude a step's diff from the phase's landing
commit. The verdict gate is also per-mode: `task_graph` phases keep the strict
"every step `ok`" clause, while `workflow`-mode phases gate on the run's own
`Completed` status plus required artifacts, so an authored script that
deliberately tolerates a failed leaf (retry/fallback via `return_status=True`)
is not wrongly failed by the phase gate.

## Validation

- `cargo check -p harness-core -p harness-cli`
- `cargo test -p harness-cli workflow_mode`
- `cargo test -p harness-cli plan_into_goal_creates_workflow_mode_phase_without_tasks`

