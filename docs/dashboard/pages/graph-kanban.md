# Graph And Kanban Page Spec

```text
status: planned
owner_role: product-design
canonical_for: Goal/Task relationship graph and execution lanes
route_or_surface: /goals/:goalId/graph, /goals/:goalId/board, Work tab
```

## Purpose

Primary user question: what depends on what, what is blocked, and what work can
move next?

Why it exists: graph explains relationships; Kanban explains execution state.
Both are projections of the same Goal/Task read model.

Non-goals:

- do not make graph the default AgentTeam UI;
- do not use graph as decoration;
- do not let Kanban replace proof documents.

## Objects And Proof

Canonical objects:

- Goals;
- Tasks;
- dependencies;
- blockers;
- follow-up tasks;
- decisions;
- graph-change proposals;
- GoalEvaluation links.

Workflow proof:

- graph and Kanban share selected object;
- blockers, splits, killed tasks, follow-ups, and next goals are semantic;
- graph-change proposals require Decision;
- lane changes reflect canonical task/goal state.

Source docs:

- [../../data-model.md](../../data-model.md)
- [../../decisions/0009-task-graph-as-derived-view.md](../../decisions/0009-task-graph-as-derived-view.md)
- [../read-model.md](../read-model.md)

Read-model inputs:

- `graphKanbanModel(snapshot, scope)`;
- task dependencies;
- parent/follow-up edges;
- goal learning status and decisions.

## Page-Level Agent Loop

Designer options:

- Kanban default plus graph focus;
- split synchronized graph and lanes;
- graph canvas default with bottom lanes.

Questioner challenges:

- Does graph explain relationships or just look complex?
- Does Kanban preserve execution clarity?
- Does mobile avoid a broken canvas?

Reviewer decision: use Kanban default plus graph focus. Borrow selection sync
from split view and defer minimap/search/collapse from canvas option.

Rejected options:

- graph canvas default: too high risk for mobile/accessibility and visual drift;
- split view first: too crowded for initial implementation.

Borrowed ideas:

- graph/card selection synchronization;
- future focus-mode controls.

## Information Architecture

Selected IA:

```text
scope header
  -> segmented Graph/Kanban switch
  -> Kanban/list default
  -> graph focus mode
  -> selected object inspector/document link
  -> graph-change proposals
```

Primary actions: select node/card, open object document, focus graph, search,
filter by status, inspect blocker.

Secondary actions: accept/reject graph-change proposal when API exists.

Empty/loading/error states:

- empty: no tasks/goals in scope, show scope reason;
- loading: preserve lane/focus geometry;
- error: show read-model/source failure.

Responsive requirements:

- desktop: Kanban default, graph focus or side preview when space allows;
- tablet: segmented tabs;
- mobile: list/Kanban first, graph focus secondary with textual fallback.

Links to hard layout specs: pending.

## Failure Modes

- graph becomes whole product;
- graph and Kanban select different objects;
- mobile graph becomes unusable;
- topology changes appear mutable without proposals/decisions.

## Screenshot Acceptance Questions

- Are graph and Kanban both reachable?
- Is Kanban the operational default?
- Does graph explain dependencies/blockers/follow-ups?
- Does selecting node/card open the same object context?
