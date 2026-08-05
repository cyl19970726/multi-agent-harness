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
| [AI-first multi-device Docs infrastructure](ai-first-multi-device-docs-infrastructure.md) | Future Docs service, revision, remote access, storage ADR, and Docs boundary for ADR 0052 AgentMember/nested Team/unified Work | PR #300 stacked on #302; awaiting PoC review |
| [Agent Team shared Works](agent-team-shared-task-list.md) | Evidence and alternatives that informed the Work/Message boundary | Absorbed by ADR 0050; retained until implementation and repeat dogfood |

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
