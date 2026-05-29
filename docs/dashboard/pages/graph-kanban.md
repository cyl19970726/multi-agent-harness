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

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | scope: Goal/TaskGraph | search | debug       |
+-----+----------------------+-----------------------------------+---------------+
| app | scope/filter 248     | graph-kanban workspace 760        | inspector 400 |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | scope summary    | | | mode switch 48                | | | selected  | |
|     | | goal/status gap  | | | Kanban | Graph | Split        | | | object    | |
|     | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | filters          | | | Kanban lanes 420              | | | blockers  | |
|     | | status/owner     | | | proposed active blocked       | | | deps      | |
|     | | dependency type  | | | review done killed follow-up  | | +-----------+ |
|     | +------------------+ | +-------------------------------+ | | proposal  | |
|     | | graph legend     | | | graph focus preview 260       | | | decision  | |
|     | | edge meanings    | | | selected node, edges, mini    | | +-----------+ |
|     | +------------------+ | +-------------------------------+ | | docs      | |
|     | rail scroll          | workspace scroll by lane/canvas    | inspector scr |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- scope/filter rail `240px` to `260px`;
- workspace min `720px`;
- inspector `380px` to `410px`;
- mode switch `48px`;
- Kanban lane area first viewport target `400px` to `460px`;
- graph focus preview target `240px` to `300px`.

First viewport content:

- current scope and goal/task count;
- mode switch with Kanban as default;
- operational lanes for proposed, active, blocked, review, done, killed, and
  follow-up states;
- graph focus preview that explains dependency/blocker/follow-up edges;
- selected object inspector with blockers, dependencies, proposal/decision
  state, and document link.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | graph scope | mode switch | search | debug    |
+-----+---------------------------------------+--------------------+
| app | board/canvas workspace 548           | inspector 288      |
| 56  | +-----------------------------------+| +----------------+ |
|     | | scope summary + filters row       || | selected       | |
|     | +-----------------------------------+| | blockers/deps  | |
|     | | tabs: Kanban Graph Proposals      || | decision/docs  | |
|     | +-----------------------------------+| +----------------+ |
|     | | Kanban lanes as horizontal scroll | inspector scroll   |
|     | | Graph tab uses focus canvas       |                    |
+-----+---------------------------------------+--------------------+
| filters collapse into drawer; graph never replaces object document          |
+------------------------------------------------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: Graph/Kanban | source | dbg  |
+--------------------------------------+
| scope 88: goal + counts + filters    |
+--------------------------------------+
| tabs 52: Board Graph Proposals Obj   |
+--------------------------------------+
| active tab 604                       |
| Board: status sections + task rows   |
| Graph: focus node + edge list + btn  |
| Proposals: graph changes + decisions |
| Obj: selected object proof links     |
+--------------------------------------+
```

Scroll ownership:

- desktop: filter rail, workspace lanes/canvas, and inspector scroll
  separately;
- tablet: workspace and inspector scroll separately; filters are drawer-only;
- mobile: only the active tab scrolls; graph has textual edge fallback.

Screenshot acceptance:

- Kanban must be the operational default;
- graph must explain relationships, not occupy the whole product;
- selecting a card/node must show one synchronized selected object;
- graph-change proposals must be visibly decision-backed before topology
  changes look accepted.

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
