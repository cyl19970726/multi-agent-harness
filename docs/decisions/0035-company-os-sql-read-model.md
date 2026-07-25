# ADR 0035: Company OS SQL Read Model

```text
status: active
date: 2026-07-23
supersedes: none
amends: 0004 File Store Before Database, 0010 Harness Store Is Canonical, 0027 Company OS Primary Model
```

## Context

Company OS Docs is now defined as **Agent-operated and Human-reviewed**. Agents
need a stable machine interface for reading, editing, governing, and verifying
company memory; Humans need UI surfaces that make structure, relationships,
state, risk, and Agent-authored changes clear.

The current Company OS Store uses append-only JSONL ledgers plus latest-row-wins
projections. That remains useful while object contracts, command boundaries, and
acceptance slices are still evolving:

- append-only rows are easy to audit and replay;
- local development needs no database service;
- fixtures and acceptance stores are inspectable;
- Store effects can be traced directly from governed Actions to latest
  projections; and
- the object model is not prematurely locked to SQL migrations.

However, Agent-operated Docs needs stronger read/query capabilities than raw
ledger scans should own long-term:

- `harness company docs query` for one Document's complete operating context;
- search over Documents, Blocks, TypedRecords, Relations, Views, and modules;
- relation traversal and scoped repair suggestions;
- standard View filtering, grouping, sorting, calendar, and chart modes;
- health findings and cleanup queues over larger stores;
- diff, snapshot, export, and rollback review surfaces; and
- permission-filtered read models for multi-user deployments.

## Decision

Do **not** replace the canonical Company OS Store with SQL now.

Introduce SQL as a **derived read/query/index layer** for Company OS when the
query surface needs it. The canonical write and audit path remains append-only
Store ledgers until a future ADR explicitly changes that contract.

```text
Governed write path:
CLI/API Action
  -> schema and policy validation
  -> append canonical JSONL ledger row
  -> latest projection
  -> SQL read model sync

Read/query path:
CLI/API query/search/view/health
  -> latest projection and/or SQL read model
  -> machine-readable result for Agents
  -> UI review surface for Humans
```

The first SQL target should be local SQLite, because it preserves the current
single-repo development and acceptance workflow. A later hosted deployment may
add PostgreSQL as another read-model adapter.

## Source-of-truth rules

- JSONL ledgers remain canonical for Company OS write truth until superseded by
  a future ADR.
- SQL rows are derived. They may accelerate reads, search, joins, and view
  rendering, but they are not independent business facts.
- Rebuilding the SQL read model from canonical ledgers must be possible.
- A SQL read model must not authorize an Action by itself; policy and command
  validation still happen through the Company OS API/Store path.
- SQL query results must preserve owning-system boundaries. Docs queries may
  reference Work, Organization, Finance, and Execution records, but must not
  convert those linked projections into Docs-owned state.
- Acceptance for a capability must identify whether it is proving canonical
  writes, derived reads, or both.

## Initial implementation direction

The first Docs query slice does not wait for SQL. `harness company docs query`
is implemented first against current latest projections, with output shaped as
the stable Agent-facing read contract that a future SQL read model can serve:

```json
{
  "document": {},
  "blocks": [],
  "children": [],
  "templates": [],
  "typed_records": [],
  "relations": [],
  "views": [],
  "business_module": {},
  "health_findings": [],
  "available_commands": [],
  "boundaries": {
    "docs_only": true,
    "work_side_effects": false,
    "finance_side_effects": false,
    "organization_side_effects": false,
    "execution_side_effects": false
  }
}
```

After this projection-backed `docs query` contract stabilizes, add an optional
SQLite read-model sync for deeper query traversal, search, standard Views, and
health checks. PostgreSQL should remain a deployment adapter, not a prerequisite
for local acceptance.

## Consequences

Positive:

- Preserves simple, auditable local Store truth.
- Avoids premature SQL schema lock-in while Company OS objects evolve.
- Gives Agents a path to strong query/search without making UI the machine
  interface.
- Allows incremental SQLite/PostgreSQL adapters behind stable CLI/API results.

Tradeoffs:

- Derived read models need sync/rebuild checks.
- Query acceptance must distinguish canonical writes from derived reads.
- Some queries will be slower until SQLite/PostgreSQL indexes exist.
- Future deployments must decide operational ownership for read-model rebuilds.

## Validation path

- `harness company docs query` returns a stable machine-readable context from
  current projections before any SQL dependency is required.
- A future SQLite read-model wave proves rebuild from ledgers, deterministic
  query results, and no write authorization from SQL alone.
- Existing `check:company-os`, Docs CLI smoke/live acceptance, and dashboard
  checks continue to pass without requiring an external database.
