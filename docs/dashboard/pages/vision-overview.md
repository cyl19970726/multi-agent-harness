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

- Vision context when available;
- Goal collection;
- GoalDesign evidence;
- GoalEvaluation evidence;
- Decision;
- GoalCase;
- autonomous proposal;
- follow-up Task or next Goal.

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

Links to hard layout specs: pending.

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

Open questions:

- whether Vision becomes a first-class schema object or remains read-model
  context for the next implementation slice.
