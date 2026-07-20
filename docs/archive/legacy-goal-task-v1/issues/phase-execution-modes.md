# Issue: GoalPhase Should Choose Task Graph Or Workflow

## Problem

The goal layer previously assumed every phase had a task graph. That made the UI
and compiler awkward for phases that are already best represented as a Starlark
workflow. The result was two competing plans for the same phase: a task graph for
the goal page and a workflow for execution.

## Desired Product Behavior

A phase has one execution mode:

| Mode | Planning truth | Runtime truth | Best UI |
| --- | --- | --- | --- |
| `task_graph` | `Task` DAG under the phase | compiled workflow steps mapped back to tasks | phase plan plus task DAG/detail |
| `workflow` | authored `.star` workflow | `WorkflowRun` and `WorkflowStep` rows | phase spec, run plan, live execution, results/evidence |

The two modes can coexist across different phases in the same goal, but a single
phase must not mix them.

## Implementation Scope

- Add `GoalPhase.execution_mode`.
- Add `GoalPhase.workflow_ref`.
- Teach `phase compile` and `goal run-phases` to load `workflow_ref` directly for
  workflow-mode phases.
- Teach planner ingestion to persist workflow-mode phases without creating task
  rows.
- Preserve built-in phase compatibility.
- Keep the dashboard data contract simple: render by mode instead of showing a
  task graph placeholder for workflow phases.

## Acceptance

- A repo workflow-mode phase with zero tasks compiles and runs through
  `goal run-phases`.
- A workflow-mode failure does not invoke the task-graph reviser.
- Planner output can create a workflow-mode phase with no tasks.
- Unsafe repo workflow paths are rejected.
- Existing task-graph phases keep the same behavior.

