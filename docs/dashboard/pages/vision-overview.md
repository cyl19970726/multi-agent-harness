# Vision Overview Page Spec

```text
status: planned
owner_role: product-design
canonical_for: Vision overview, goal collection, distance-to-vision
route_or_surface: /visions/:visionId plus compact shell strip
```

## Purpose

Primary user question: are completed, active, blocked, and proposed goals
moving the product toward the Vision?

Why it exists: Vision is a long-lived target and a collection of goals. It is
not the same thing as the selected Goal. The page must show goal completion
state, distance-to-vision, evaluation gaps, and next-round proposals.

Non-goals:

- do not turn Vision into a single active goal card;
- do not show only task progress;
- do not mark a goal complete without Decision and GoalEvaluation proof.

## Objects And Proof

Canonical objects:

- Vision (first-class object; goals link via `Goal.vision_id`);
- Goal collection;
- GoalDesign;
- GoalEvaluation;
- Decision;
- GoalCase;
- autonomous proposal;
- follow-up Task or next Goal.

Implemented: `Vision` is now a first-class object. VisionOverview renders the
`visions` snapshot array and links goals through `Goal.vision_id`; goal rows
show GoalDesign/GoalEvaluation presence (dual-read with legacy evidence) and the
closeout Decision/GoalEvaluation state. This resolves the prior open question
below in favor of a schema object.

Workflow proof:

- goals grouped as proposed, active, blocked, complete, archived/rejected;
- completed goals show Decision and GoalEvaluation or explicit blocked/killed
  closeout;
- active goals show distance-to-vision gaps;
- next-round proposals link to prior evidence/evaluation.

Source docs:

- [../../dashboard.md](../../dashboard.md)
- [../../goal-learning-loop.md](../../goal-learning-loop.md)
- [../read-model.md](../read-model.md)

Read-model inputs:

- `activeVisionContext(snapshot, selectedGoalId)`;
- `goalCollection(snapshot)`;
- `goal_learning_status`;
- decisions and evidence by goal.

## Page-Level Agent Loop

Designer options:

- ladder: goals grouped by status with distance-to-vision summary;
- map: goal graph as the primary view;
- evaluator: evaluation and next-round plan first.

Questioner challenges:

- Does the page prove that Vision is a goal collection?
- Does it distinguish done from not-done using Decision/Evaluation proof?
- Does next-round planning remain connected to evidence?

Reviewer decision: use ladder as the default, borrow map focus for later graph
mode, borrow evaluator summary as a right-side context block.

Rejected options:

- graph primary: too likely to hide completion proof;
- evaluator primary: too close to a report and weak for active work.

Borrowed ideas:

- graph can explain goal dependencies in a secondary focus;
- evaluator summary shows distance-to-vision and next proposal.

## Information Architecture

Selected IA:

```text
Vision header
  -> status-grouped goal collection
  -> selected goal context
  -> distance-to-vision / next-round panel
  -> evaluation gaps and warnings
```

Primary actions: select goal, open Goal document, inspect next proposal,
request evidence, open related docs.

Secondary actions: filter by status, open GoalEvaluation, open GoalCase.

Empty/loading/error states:

- empty: explain missing Vision context and show available goals;
- loading: preserve header and goal group geometry;
- error: show source/API failure and last-good snapshot when available.

Responsive requirements:

- desktop: goal collection and next-round context both visible;
- tablet: next-round context becomes side drawer;
- mobile: tabs for Goals, Progress, Next, Warnings.

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Agent Workbench | live/source | selected vision | search | debug        |
+-----+----------------------+-----------------------------------+---------------+
| app | vision rail 280      | goal collection workspace 720      | context 376   |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | Vision title     | | | status tabs 48: Proposed     | | | distance  | |
|     | | final outcome    | | | Active Blocked Complete      | | | to vision | |
|     | | scenario/pilot   | | +-------------------------------+ | +-----------+ |
|     | +------------------+ | | Active goals lane 220         | | | next goal | |
|     | | goal filters     | | | - selected goal document row  | | | proposals | |
|     | | owner/status     | | | - branch/eval state           | | +-----------+ |
|     | | evaluation gaps  | | +-------------------------------+ | | eval gaps | |
|     | +------------------+ | | Complete goals lane 180       | | | warnings  | |
|     |                      | | Blocked/proposed lanes        | | +-----------+ |
|     | rail scroll          | workspace scroll below tabs       | context scroll |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- vision rail `280px`;
- workspace min `680px`;
- context panel `360px` to `380px`;
- status tabs `48px`;
- lane rows target `92px` to `128px`.

First viewport content:

- Vision title and final outcome;
- goal groups for proposed, active, blocked, complete, archived/rejected;
- selected Goal row with Decision/GoalEvaluation state;
- distance-to-vision summary;
- next-round proposal list with source evidence.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | live/source | selected vision | debug         |
+-----+---------------------------------------+--------------------+
| app | goals workspace 548                  | context 288        |
| 56  | +-----------------------------------+| +----------------+ |
|     | | vision header + final outcome     || | distance       | |
|     | +-----------------------------------+| | next proposals | |
|     | | tabs: Proposed Active Blocked     || | eval warnings  | |
|     | | Complete Archived                 || +----------------+ |
|     | +-----------------------------------+| context scroll     |
|     | | grouped goal lanes                |                    |
|     | | selected goal details inline      |                    |
+-----+---------------------------------------+--------------------+
| vision rail drawer closed; opens from app rail                    |
+------------------------------------------------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: live/source | search | debug |
+--------------------------------------+
| vision header 96                    |
| title + final outcome + gap count   |
+--------------------------------------+
| tabs 52: Goals Progress Next Warn   |
+--------------------------------------+
| active tab 648                      |
| Goals: grouped compact goal rows    |
| row: title/status/decision/eval     |
| Progress: distance + complete gaps  |
| Next: proposals + source evidence   |
| Warn: blocked/missing evaluation    |
+--------------------------------------+
```

Scroll ownership:

- desktop: vision rail, workspace lanes, and context panel scroll separately;
- tablet: workspace and context scroll separately; rail is drawer-only;
- mobile: only the active tab scrolls.

Screenshot acceptance:

- the first viewport must show Vision as a goal collection, not one Goal;
- complete goals must expose Decision/GoalEvaluation proof;
- next proposals must show source evidence;
- the page must not look like a metric dashboard.

## Failure Modes

- Vision collapsed into selected Goal;
- all tasks done treated as goal complete;
- proposed goals shown without source evidence;
- next-round planning detached from GoalEvaluation;
- page reads as a metrics dashboard.

## Screenshot Acceptance Questions

- Can the reviewer see completed and not-complete goals without reading code?
- Is the selected goal visibly part of a larger Vision?
- Does completion require Decision/Evaluation proof?
- Does the first viewport read as product planning context, not a card wall?

Resolved questions:

- Vision is now a first-class schema object (`schemas/vision.schema.json`), not
  read-model-only context; goals reference it via `Goal.vision_id`.
