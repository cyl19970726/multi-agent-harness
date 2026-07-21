# ADR 0028: Retire the superseded coordination stack

## Status

Accepted. The detailed removal record and historical evidence are archived at
[the original ADR](../archive/legacy-goal-task-v1/decisions/0028-retire-goal-phase-task-graph.md).

## Active Decision

`Mission -> ordered Wave -> executor` is the only coordination hierarchy for
new work. The former Goal, GoalPhase, task-graph, and ledger-backed planning
surfaces are not active product objects, authoring paths, navigation entries,
or compatibility UI.

Historical data must be exported and verified before removal when preservation
is required. For stores explicitly declared disposable, old rows may be
deleted. Neither case permits retired objects to re-enter current planning
context.

Residual field names in internal schemas are removal debt, not product
contracts. New code and documentation must not depend on them.

## Why this tombstone exists

Active architecture documents need a stable, resolvable policy reference
without loading the archived model into normal context. Implementation history,
migration evidence, and deleted-surface inventories remain in the archive.
