# Research

```text
status: active research index
owner_role: Docs Governance
authority_class: entry
canonical_for: navigation and lifecycle rules for unresolved repository research
```

Research records evidence, comparisons, and hypotheses that may inform a
product or architecture decision. It is not product authority, implementation
proof, or default Agent context.

## Active studies

| Study | Decision it informs | Status |
| --- | --- | --- |
| [Agent Team shared task list](agent-team-shared-task-list.md) | Whether Agent Team needs a TeamRun-scoped task board, and the smallest provider-neutral contract | Active; ADR and implementation not yet accepted |

## Lifecycle

An active study must:

1. name the current contract it questions;
2. preserve reproducible evidence and distinguish observation from inference;
3. link the WorkItem, implementation plan, or ADR it is intended to inform;
4. state explicit non-goals so exploration does not silently expand product
   scope; and
5. be promoted into canonical product docs, an ADR, schemas, APIs, tests, and
   operator surfaces before anyone claims the behavior exists.

When a decision absorbs the useful findings, update the canonical authority and
delete or retain the study according to
[Documentation Governance](../documentation-governance.md). Git history is the
default archive for research that no longer explains a live decision.
