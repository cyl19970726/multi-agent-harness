# Evidence, Review, And Decision Page Spec

```text
status: planned
owner_role: product-design
canonical_for: acceptance proof and decision flow
route_or_surface: global queue plus Goal/Task object-local sections
```

## Purpose

Primary user question: what claim is being accepted, what evidence supports it,
who reviewed it, and what decision was made?

Why it exists: Harness work is accepted through evidence-backed proposals,
review, and Leader decisions. The Workbench must show acceptance state without
raw JSON.

Non-goals:

- do not collapse evidence, review, and decision into one status chip;
- do not hide missing review behind done tasks;
- do not treat provider chat as acceptance proof.

## Objects And Proof

Canonical objects:

- Evidence;
- Proposal;
- review evidence/report;
- Decision;
- Task;
- Goal;
- checks, screenshots, PR refs, GoalEvaluation.

Workflow proof:

- evidence source and task/object link are visible;
- proposal changed paths and evidence refs are close together;
- review output is visually distinct from Leader Decision;
- waivers show rationale and follow-up task;
- missing evidence/review/decision remains incomplete.

Source docs:

- [../../workflow-git-pr.md](../../workflow-git-pr.md)
- [../../goal-learning-loop.md](../../goal-learning-loop.md)
- [../acceptance.md](../acceptance.md)

Read-model inputs:

- `decisionQueue(snapshot, scope)`;
- evidence by task/goal/proposal;
- proposals by task/member;
- decisions by task/goal;
- checks and screenshot refs when available.

## Page-Level Agent Loop

Designer options:

- four-lane acceptance strip: Evidence, Proposal, Review, Decision;
- timeline-only chain;
- packet summary card.

Questioner challenges:

- Are review and decision separate?
- Can missing evidence block acceptance?
- Are waivers explicit and follow-up backed?

Reviewer decision: use four-lane strip for object-local proof plus global queue
for pending decisions. Borrow packet summary for compact inspector.

Rejected options:

- timeline-only: can hide missing stages;
- packet-only: too condensed for proof order.

Borrowed ideas:

- compact packet summary in inspector.

## Information Architecture

Selected IA:

```text
global decision queue
  -> pending proposals and waivers
object-local acceptance strip
  -> evidence
  -> proposal
  -> review
  -> decision
  -> evaluation/follow-up
```

Primary actions: open evidence, open proposal, request review, record decision
when API exists, open follow-up.

Secondary actions: filter by missing evidence/review/decision.

Empty/loading/error states:

- empty: no pending acceptance work;
- loading: preserve queue/strip geometry;
- error: show source/API failure.

Responsive requirements:

- desktop: global queue in Team workspace plus object-local strip;
- tablet: queue drawer and object sections;
- mobile: Decisions tab and compact object-local proof.

Links to hard layout specs: pending.

## Failure Modes

- evidence shown without source;
- review missing but task appears accepted;
- decision collapsed into status;
- waiver without rationale/follow-up;
- provider transcript treated as proof.

## Screenshot Acceptance Questions

- Can the reviewer distinguish Evidence, Proposal, Review, and Decision?
- Is missing proof visually incomplete?
- Are waivers explicit?
- Does the first viewport show decision pressure when it matters?
