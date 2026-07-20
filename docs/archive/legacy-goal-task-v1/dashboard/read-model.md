# Agent Workbench Read Model

The Agent Workbench read model turns canonical harness objects into an
operator-facing control plane. It is a projection, not the source of truth.

## Source Boundary

```text
Harness store / CLI / API
  -> DashboardSnapshot
  -> read model selectors
  -> advisory warnings
  -> React panels
```

The read model may filter, group, count, and link objects. It must not create
canonical task assignment, delivery, evidence, proposal, review, or decision
state.

Append-only objects that represent mutable state, such as `Message` delivery
updates, must be projected as the latest row per object id before Dashboard
warnings are computed. Otherwise stale rows such as an old `queued` assignment
can create false warnings after a later `acknowledged` row exists.

## Goal Scope

The default product unit is the selected goal:

```text
selected goal
  -> tasks where task.goal_id matches
  -> members referenced by owner / assignee / reviewer / messages / sessions
  -> teams containing those members
  -> warnings linked to the goal, task, member, proposal, decision, or session
```

This keeps the Dashboard focused on one operational workflow instead of a raw
repository-wide object dump.

## Required Selectors

The frontend design in [frontend-design.md](../../../dashboard/frontend-design.md) should be built
from named projections instead of ad hoc component filtering.

| Selector | Owns | Degraded state |
| --- | --- | --- |
| `activeVisionContext(snapshot, selectedGoalId)` | vision summary, final acceptance signal, selected goal relation, missing-context warning | show explicit missing Vision/read-model gap |
| `goalCollection(snapshot)` | proposed, active, blocked, complete, archived/rejected goal groups | show ungrouped goals with grouping warning |
| `goalDocument(snapshot, goalId)` | Goal, GoalDesign evidence, goal learning status, team design, decisions, evaluation, related docs | mark missing GoalDesign, Decision, or GoalEvaluation separately |
| `teamWorkspace(snapshot, teamId, goalId?)` | full roster independent of task refs, role groups, queues, activity, decision queue | show team/member source gap instead of inferring disposable workers |
| `memberWorkbench(snapshot, memberId)` | identity, current work, runtime layers, prompt/skills, sessions, safe-action disabled reasons | show member as durable but incomplete |
| `memberTimeline(snapshot, memberId)` | chronological messages, delivery updates, provider sessions, events, reports, evidence refs, proposals | fall back to grouped sections with timeline warning |
| `taskDocument(snapshot, taskId)` | proof-order assignment, report, evidence, proposal, review, decision, Git refs, object-local warnings | mark each missing protocol link |
| `graphKanbanModel(snapshot, scope)` | synchronized graph nodes/edges and lane projections for Vision/Goal/Task scopes | disable graph focus while preserving Kanban/document access |
| `decisionQueue(snapshot, scope)` | proposals, missing reviews, warning repairs, waivers, pending decisions, follow-ups | show queue unavailable warning |
| `docsContext(snapshot, objectRef)` | registry-backed related docs and owner/status/lifecycle metadata | link to docs index and show missing relation warning |
| `warningsByObject(snapshot)` | advisory warnings grouped by affected canonical object | show global warnings only |

Each selector should consume latest-row projections for append-only mutable
objects before computing React keys, status counts, or warning state.

### Vision→Goal→Task derived projections (ADR 0019)

The unified Work board and the Notion document detail pages
([the archived Work Board design](work-board-design.md)) add these pure derivations to the
read model. They are derived views, never stored fields:

| Selector | Owns |
| --- | --- |
| `taskGraph(tasks)` | dependency-graph projection from `depends_on_task_ids` alone: `nodes`, dependency→dependent `edges`, `ready` (all dependencies `done` and self planned/assigned), `waiting` (taskId → unfinished dependency ids). The derived `waiting` is **distinct from** the stored `status==="blocked"`. |
| `tasksBlockedBy(taskId, tasks)` | reverse edges — the tasks that depend on `taskId`. |
| `taskGitMetadata(task)` | effective git context, preferring `git_metadata` and falling back to the flat `branch_ref`/`pr_ref`/`workspace_ref`/`owned_paths` fields retained for back-compat. |
| `displayGoalStatus(goal)` | product status column, folding legacy `complete` into `done`; `archived` stays hidden from the board. |

The board lays out `displayGoalStatus` (Goals: active/blocked/review/done) or
`status` (Tasks: planned/assigned/running/blocked/review/done), overlaying the
`taskGraph` ready/waiting chip on each task card.

### Phase-scoped projections (goal-task-board-model)

Tasks are viewed under Goal -> Phase; the flat global task board is retired and
the Work board defaults to the goal collection, with a goal-scoped task lane
view as the drill-in fallback. Phase-scoped derivations in
`apps/agent-dashboard/src/model/readModel.ts`:

| Selector | Owns |
| --- | --- |
| `phaseTaskDag(phaseId, tasks)` | layers a phase's live tasks into the same layer/group shape the Starlark compiler emits (in-phase `depends_on_task_ids` only; superseded tasks excluded). |
| `phaseKanban(phaseId, tasks)` | phase-scoped status lanes, the Kanban counterpart of `phaseTaskDag`; covered by the phase-board fixture. Since the goal workflow workbench, the Goal page renders the phase DAG plus workflow panel rather than a per-phase lane toggle. |
| `phaselessGoalTasks(goalId, tasks)` | the "(no phase)" set — live tasks on the goal without a live phase, kept visible under phase-driven goals. |

Workflow-run projections in `apps/agent-dashboard/src/model/workflowSelectors.ts`
join `WorkflowRun.goal_id`/`phase_id` forward links with
`goal_orchestration_runs` checkpoints (e.g. `selectPhaseWorkflowRuns`) so each
phase shows the runs that executed it.

## Detail Panels

Task detail must show the evidence chain that proves work happened:

```text
task
  -> assignment proof: Message(kind=task)
  -> reports: Message(kind=report)
  -> provider sessions and child threads
  -> evidence refs
  -> proposal
  -> review / critic evidence
  -> decision
```

Observer proposal detail must show the autonomous loop that creates future
work:

```text
Evidence(source_type=goal_proposal|graph_change_proposal|blocker|follow_up)
  -> proposal Message from Observer/member to Lead
  -> linked evidence such as next_round_plan
  -> Lead Decision disposition
  -> follow-up task ids and goal ids
```

Member detail must show whether an `AgentMember` is a durable harness actor:

```text
member
  -> inbox / outbox
  -> runtime health
  -> current task / proposal
  -> provider sessions
  -> child threads
```

## Warning Promotion

Warnings in `apps/agent-dashboard/src/model/warnings.ts` are advisory until they
are promoted.

Promotion rule:

```text
read-model warning
  -> stable failure mode
  -> Rust validation / CLI gate / review gate / CI check
  -> Dashboard displays canonical result
```

Do not let a UI-only warning become an invisible acceptance rule. If a warning
blocks a goal, move the rule to a canonical surface first.

## Current Advisory Warnings

The `kind` strings below are owned by `apps/agent-dashboard/src/model/warnings.ts`
(`deriveWarnings`); treat that file as the source of truth and re-verify this
table against it rather than trusting the prose.

| Warning | Meaning |
| --- | --- |
| `fake_assignment_risk` | Task has an assignee and is past `planned` but no task assignment message exists. |
| `assignment_not_delivered` | Task assignment exists but has not reached delivered or acknowledged. |
| `missing_report` | Task is in review or done without a report message. |
| `missing_evidence` | Task is in review without linked evidence. |
| `proposal_missing_evidence` | Proposal has no evidence refs. |
| `decision_missing` | Reviewable task (review/done) lacks a Leader decision. |
| `lead_owner_role_mismatch` | Team `owner_agent_id` resolves to a member whose role is not `lead`, or does not resolve to any member. |
| `review_needs_decision` | A review verdict (fail/blocked/needs_changes or with blockers) needs a Leader decision. |
| `gap_p0_open` | A P0 `Gap` is still open. |
| `gap_unresolved` | A non-P0-open `Gap` is unresolved (open/in_progress/blocked). |
| `failed_provider_session` | Provider session failed or was canceled. |
| `goal_learning_gap` | Goal learning status reports a missing workflow link. |
| `goal_close_without_evaluation` | Goal task graph is complete (or goal is closed) without a closeout Decision + GoalEvaluation. |
| `waiver_without_follow_up` | A closeout waiver lacks a follow-up task or evidence. |

## Implementation Files

| File | Role |
| --- | --- |
| `apps/agent-dashboard/src/types.ts` | Snapshot and UI object types. |
| `apps/agent-dashboard/src/model/readModel.ts` | Selectors, goal scope, member/team/task grouping. |
| `apps/agent-dashboard/src/model/warnings.ts` | Advisory warning derivation. |
| `apps/agent-dashboard/src/model/workflowSelectors.ts` | Workflow-run projections (per-phase runs, step status counts, run progress/liveness). |
| `apps/agent-dashboard/src/model/workflowShape.ts` | Derived serial/parallel shape and Gantt geometry for a run's steps. |
| `apps/agent-dashboard/src/surfaces/*` | Page surfaces (display and navigation only). |
| `apps/agent-dashboard/src/components/{ui,workbench}/*` | shadcn/ui primitives and product atoms. |

## Current Autonomous Projection

The Rust snapshot includes `autonomous_proposals`, a Dashboard-only projection
derived from canonical `Evidence`, `Message`, `Decision`, and `Task` records.
It is intentionally not a new stable schema yet.

Current proposal source types:

```text
goal_proposal
graph_change_proposal
blocker
follow_up
```

`next_round_plan` is supporting evidence linked through the proposal message.
The projection adds convenience fields such as source member, target Lead,
disposition, linked evidence ids, and follow-up task/goal ids. If these fields
become required by gates outside the Dashboard, they should graduate into a
versioned schema or explicit CLI/API contract.
