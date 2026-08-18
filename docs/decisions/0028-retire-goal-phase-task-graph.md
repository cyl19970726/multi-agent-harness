# ADR 0028: Retire the superseded coordination stack

## Status

Accepted. This document is the complete active retirement policy. The final
runtime export is preserved outside the repository and is independently
verifiable; detailed implementation history remains recoverable from Git.

## Active Decision

The former Goal, GoalPhase, task-graph, and ledger-backed planning surfaces are
not active product objects, authoring paths, navigation entries, or
compatibility UI. That retirement is still current policy.

> Historical note (DOC-108): at decision time this ADR named
> `Mission -> ordered Host-plan Wave` as the only planning hierarchy for new
> work, with Mission-linked independent Agent Teams keeping their own lifecycle
> under ADR 0034. That replacement hierarchy is itself retired — Teams are
> durable and Mission-less, Work is the coordination object, and Mission/Wave
> write authority fails closed on every surface. The retirement policy in this
> ADR survives that change unaltered.

Historical data must be exported and verified before removal when preservation
is required. For stores explicitly declared disposable, old rows may be
deleted. Neither case permits retired objects to re-enter current planning
context.

Residual field names in internal schemas are removal debt, not product
contracts. New code and documentation must not depend on them.

## Preservation result

The repository's final frozen export for this project was verified with 106
files, 2,814 relationship edges, referential closure, and one declared known
anomaly. Runtime archives remain external to active product documentation.

## Why this policy exists

Active architecture documents need a stable, resolvable policy reference
without loading the retired model into normal context. Deleted prose and
surface inventories remain recoverable from Git; runtime records remain in the
verified native export.
