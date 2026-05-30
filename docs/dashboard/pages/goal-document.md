# Goal Document Page Spec

```text
status: planned
owner_role: product-design
canonical_for: Goal workbench document and acceptance proof
route_or_surface: /goals/:goalId and Work tab Goal surface
```

## Purpose

Primary user question: why does this Goal exist, how was it designed, and what
proof says it is complete or still incomplete?

Why it exists: Goal is a durable outcome. It owns GoalDesign, team design,
branch/integration policy, dynamic TaskGraph, evidence, decisions, evaluation,
and distance-to-vision. It is not a task list.

Non-goals:

- do not infer completion from task status alone;
- do not bury GoalDesign or GoalEvaluation;
- do not show task cards without assignment/evidence/decision context.

## Objects And Proof

Canonical objects:

- Goal;
- GoalDesign evidence;
- AgentTeam;
- TaskGraph;
- Message;
- Evidence;
- Proposal;
- Review;
- Decision;
- GoalEvaluation;
- GoalCase;
- branch/worktree/PR refs.

Workflow proof:

- objective and success criteria before operations;
- GoalDesign gate before implementation state;
- team plan and active team visible together;
- Graph/Kanban visible for task execution and dependencies;
- Decision and GoalEvaluation determine completion.

Source docs:

- [../../goal-learning-loop.md](../../goal-learning-loop.md)
- [../../workflow-git-pr.md](../../workflow-git-pr.md)
- [../read-model.md](../read-model.md)

Read-model inputs:

- `goalDocument(snapshot, goalId)`;
- goal learning status;
- tasks, messages, evidence, proposals, decisions by goal;
- graph/Kanban model.

## Page-Level Agent Loop

Designer options:

- audit document: proof sections in durable document order;
- cockpit: health strip, queues, graph/Kanban, decision queue;
- learning document: evaluation and next-round planning first.

Questioner challenges:

- Does the page prove Goal is more than tasks?
- Is completion backed by Decision/Evaluation?
- Are team design, branch refs, and evidence close to the proof chain?

Reviewer decision: use audit document. Borrow cockpit health strip and learning
closeout block.

Rejected options:

- cockpit primary: reduces Goal to operations;
- learning primary: underplays execution proof.

Borrowed ideas:

- health strip;
- distance-to-vision closeout.

## Information Architecture

Selected IA:

```text
goal header
  -> objective and success criteria
  -> GoalDesign and team design
  -> branch/worktree/PR policy
  -> Graph/Kanban block
  -> tasks and proof chain
  -> evidence/review/decision
  -> GoalEvaluation and next round
```

Primary actions: open task, open graph/Kanban, request review, inspect
Decision, open GoalEvaluation, open docs.

Secondary actions: propose follow-up task/goal when API exists.

Empty/loading/error states:

- empty: Goal exists but lacks GoalDesign/evaluation, show explicit gap;
- loading: preserve document section order;
- error: show source/API failure and last-good snapshot.

Responsive requirements:

- desktop: document plus right evaluation/decision context;
- tablet: document with sticky section navigation;
- mobile: Work tab sections with graph as secondary focus.

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | active Vision | selected Goal | search | dbg |
+-----+----------------------+-----------------------------------+---------------+
| app | section rail 248     | goal document 760                 | proof 400     |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | goal identity    | | | title/objective 104          | | | health    | |
|     | | status/branch    | | | status, owner, success        | | | strip     | |
|     | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | doc nav          | | | GoalDesign 168               | | | decision  | |
|     | | - objective     | | | scenario, non-goals, team     | | | queue     | |
|     | | - design        | | +-------------------------------+ | +-----------+ |
|     | | - team          | | | branch/worktree/PR 88        | | | evidence  | |
|     | | - graph/board   | | +-------------------------------+ | | review    | |
|     | | - evidence      | | | graph/Kanban preview 220     | | | eval      | |
|     | | - evaluation    | | | deps + lanes + selected task  | | +-----------+ |
|     | +------------------+ | +-------------------------------+ | proof scroll |
|     | | related docs     | | | proof chain + evaluation      | |             |
|     | +------------------+ | | continues below first fold    | |             |
|     | rail scroll          | document scroll                   | proof scroll |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- section rail `240px` to `260px`;
- document column min `720px`;
- proof inspector `380px` to `410px`;
- title/objective block `96px` to `120px`;
- GoalDesign block target `160px` to `190px`;
- graph/Kanban preview target `200px` to `240px`.

First viewport content:

- Goal objective, success criteria, owner, status, and goal branch;
- GoalDesign with scenario, non-goals, designed team, and acceptance gate;
- branch/worktree/PR policy before task execution details;
- graph/Kanban preview with dependency and lane context;
- proof inspector with health, missing Decision/Evaluation, evidence, review,
  and warnings.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | active Vision | selected Goal | search | dbg  |
+-----+-------------------------------+----------------------------+
| app | goal document 560             | proof drawer 284           |
| 56  | +---------------------------+ | +------------------------+ |
|     | | title/objective/status    | | | health + decision     | |
|     | +---------------------------+ | | evidence/review/eval   | |
|     | | sticky section tabs 48    | | | warnings              | |
|     | +---------------------------+ | +------------------------+ |
|     | | GoalDesign                | | proof scroll             |
|     | | branch/worktree/PR        | |                          |
|     | | graph/Kanban preview      | |                          |
|     | | proof chain/evaluation    | |                          |
+-----+-------------------------------+----------------------------+
| section rail collapsed into tabs; graph can open full-width focus          |
+------------------------------------------------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: Goal | live/source | debug   |
+--------------------------------------+
| header 112: title/status/branch      |
| objective + success + missing proof  |
+--------------------------------------+
| tabs 52: Design Graph Proof Eval Doc |
+--------------------------------------+
| active tab 584                       |
| Design: GoalDesign + team + policy   |
| Graph: Kanban first, graph focus btn |
| Proof: evidence -> review -> decision|
| Eval: GoalEvaluation + next proposal |
| Doc: related docs and warnings       |
+--------------------------------------+
```

Scroll ownership:

- desktop: section rail, goal document, and proof inspector scroll separately;
- tablet: document scrolls; proof drawer scrolls separately;
- mobile: only the active tab scrolls.

Screenshot acceptance:

- the first viewport must read as a Goal work document, not a task card grid;
- GoalDesign must appear before execution proof;
- branch/worktree/PR policy must be close to proof and review;
- Decision and GoalEvaluation gaps must be visible before the goal can look
  complete.

## Failure Modes

- Goal reduced to task list;
- completion inferred from done tasks;
- GoalDesign hidden;
- GoalEvaluation absent;
- branch/PR/evidence detached from decision.

## Screenshot Acceptance Questions

- Can the reviewer see why the Goal exists?
- Is GoalDesign visible before implementation proof?
- Does completion require Decision/Evaluation?
- Does the page read as a durable work document rather than cards?
