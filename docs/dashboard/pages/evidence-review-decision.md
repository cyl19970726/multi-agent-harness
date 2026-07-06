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
- Review (first-class object: `review_kind`, `verdict`, `blockers`,
  `residual_risk`, `missing_validation`, `evidence_ids`);
- Decision (`decision_kind`: `verdict`/`closeout`/`stop_gate`/`waiver`/...;
  `is_waiver`, `follow_up_task_id`);
- Task;
- Goal;
- checks, screenshots, PR refs, GoalEvaluation.

Implemented: `Review` is a first-class object whose `verdict`, `blockers`,
`missing_validation`, and `residual_risk` render visually distinct from the
Leader `Decision` in the Task document proof chain. The decision queue
(`decisionQueue`/`leadDecisionQueue`) is a read-model projection; the
DecisionCenter component that renders it is not reachable from the current
shell rail. `decision_kind`, `is_waiver`, and `follow_up_task_id` exist on the
`Decision` snapshot type but are not rendered yet; waiver legibility currently
comes from goal closeout status and the `waiver_without_follow_up` warning. A
`Review` is evidence for a `Decision`, never the decision itself.

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

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | acceptance queue | scope | search | debug    |
+-----+----------------------+-----------------------------------+---------------+
| app | queue rail 280       | acceptance workspace 720          | packet 376    |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | queue summary    | | | pending decision header 72   | | | selected  | |
|     | | missing proof    | | +-------------------------------+ | | proposal  | |
|     | +------------------+ | | four-lane proof strip 220     | | +-----------+ |
|     | | pending review   | | | Evidence | Proposal | Review  | | | evidence  | |
|     | | pending decision | | | Decision | Evaluation/Followup  | | | refs      | |
|     | | waivers          | | +-------------------------------+ | +-----------+ |
|     | +------------------+ | | queue items 360               | | | review    | |
|     | | filters          | | | claim, owner, object, gap     | | | decision  | |
|     | | severity/status  | | | next action, blocker          | | +-----------+ |
|     | +------------------+ | +-------------------------------+ | packet scr  |
|     | rail scroll          | workspace scroll                   |             |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- queue rail `270px` to `290px`;
- acceptance workspace min `680px`;
- packet inspector `360px` to `380px`;
- decision header `64px` to `80px`;
- four-lane strip target `200px` to `240px`;
- queue item row `88px` to `120px`.

First viewport content:

- global queue counts for missing evidence, missing review, pending decision,
  and waivers;
- selected claim and object scope;
- four-lane evidence/proposal/review/decision strip with incomplete states;
- selected packet with evidence refs, reviewer output, Leader Decision, waiver
  rationale, and follow-up if relevant.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | acceptance | selected scope | search | debug  |
+-----+---------------------------------------+--------------------+
| app | acceptance workspace 548             | packet 288         |
| 56  | +-----------------------------------+| +----------------+ |
|     | | summary + filters row             || | selected claim | |
|     | +-----------------------------------+| | evidence refs  | |
|     | | proof strip: E P R D Eval         || | review/decision| |
|     | +-----------------------------------+| +----------------+ |
|     | | queue: review/decision/waiver     | packet scroll      |
|     | | object-local proof sections       |                    |
+-----+---------------------------------------+--------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: Acceptance | source | debug |
+--------------------------------------+
| summary 88: pending + missing proof  |
+--------------------------------------+
| tabs 52: Queue Proof Waivers Packet  |
+--------------------------------------+
| active tab 604                       |
| Queue: claim rows + next action      |
| Proof: Evidence -> Proposal -> Review|
|        -> Decision -> Evaluation     |
| Waivers: rationale + follow-up       |
| Packet: selected claim details       |
+--------------------------------------+
```

Scroll ownership:

- desktop: queue rail, acceptance workspace, and packet inspector scroll
  separately;
- tablet: workspace and packet inspector scroll separately;
- mobile: only the active tab scrolls.

Screenshot acceptance:

- Evidence, Proposal, Review, and Decision must be visually distinct;
- incomplete proof must look blocked, not done;
- waiver rows must include rationale and follow-up task/goal;
- provider chat cannot appear as acceptance proof unless tied to Evidence.

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
