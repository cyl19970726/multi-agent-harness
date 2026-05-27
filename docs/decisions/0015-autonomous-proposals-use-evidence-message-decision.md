# ADR 0015: Autonomous Proposals Use Evidence, Message, And Decision First

## Status

Accepted.

## Context

The product needs standing Agent Teams that can keep working after one task.
Observer, Critic, Dashboard, Runtime, or domain members must be able to propose
new goals, blockers, graph changes, and follow-up tasks. Lead must then accept,
reject, defer, or request more evidence.

This is central to the product vision, but a large new proposal schema would
freeze fields before the workflow is proven. The current stable objects already
cover the needed source-of-truth chain:

```text
Evidence -> Message -> Decision -> optional Goal / Task
```

## Decision

First-version autonomous proposals are represented with existing objects:

- `Evidence(source_type=goal_proposal|graph_change_proposal|blocker|follow_up)`
  records the proposal artifact.
- `Evidence(source_type=next_round_plan)` records the planning basis for
  automatic next-round work.
- `Message` sends the proposal to Lead or another reviewer.
- `Decision` records Lead disposition.
- Accepted proposals may create `Goal`, `Task`, and assignment
  `Message(kind=task)` records.

The Dashboard may expose an `autonomous_proposals` read-model projection, but
that projection is not the canonical schema. Stable fields should graduate into
schemas only after repeated gates require them across CLI, API, Dashboard, and
review flows.

## Consequences

- The system can prove the autonomous loop immediately without introducing a
  premature `GoalProposal` object.
- Rejected and deferred proposals remain visible as evidence and decisions.
- The same message-first workflow applies to user-requested work and
  team-generated work.
- Dashboard code must stay clear that `autonomous_proposals` is a projection.
- Future schema promotion should preserve the Evidence/Message/Decision event
  order instead of bypassing it.

## Validation

```bash
npx pnpm@9.15.4 acceptance:autonomous-team
```

The gate proves a standing team, same-member reuse, peer messages, goal
evaluation, Observer next-round proposal, Lead acceptance, follow-up goal/task
creation, and Dashboard projection.
