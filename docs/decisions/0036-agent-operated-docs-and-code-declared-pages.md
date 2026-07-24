# ADR 0036: Agent-operated Docs and code-declared pages

```text
state: active
date: 2026-07-24
supersedes: Notion-editor-first interpretation of Company OS Docs
depends_on: 0027-company-os-primary-model, 0029-agent-programmable-document-runtime, 0035-company-os-sql-read-model
```

## Context

Company OS Docs is the memory and operating-structure substrate for a company
run by Humans and Agents. Most durable edits are expected to be made by Agents
through CLI/API/skill commands. Humans primarily inspect, review, approve, and
understand company state.

Earlier discussion sometimes made Docs sound like a Notion clone. That is the
wrong product center. Notion was built around Human editing. Star Harness Docs
must be built around Agent operation, governed records, typed business truth,
and high-quality review surfaces.

## Decision

Docs is not a Notion editor clone. Docs is:

```text
Agent-operated company memory substrate
+ code-declared custom business pages
+ Human-facing inspection / review / approval UI
```

The primary write interface is CLI/API/skill. UI may expose selected low-risk
editing controls, but heavy collaborative editing, drag/drop authoring, and
Notion-style page construction are not P0.

Generic Document pages are fallback reading and light-maintenance surfaces.
Core business pages must be code-declared custom pages that consume native
Company OS objects:

- `Document`
- `Block`
- `TypedRecord`
- `Relation`
- `View`
- `BusinessModule`
- `WorkItem`
- `Approval`
- `FinancialRecord`
- `ActorRef`

Those pages are declared through `CustomPageDefinition` and implemented through
`CustomPagePackage`. A page package may render a rich business UI, but it must
not own a second data store or duplicate Company OS truth. It can only read
declared queries and dispatch allowed governed Actions.

Every important code-declared page should have a visual contract:

```text
expected design -> implementation -> actual screenshot -> review evidence
```

## Consequences

- Docs implementation should prioritize CLI/API capabilities over rich editor
  polish.
- Agents should create, update, validate, relate, diff, and migrate company
  memory through governed commands.
- Human UI should make state, provenance, relationships, risk, and required
  decisions clear.
- `PageDefinition`, `PagePackage`, action policy refs, data-query declarations,
  and visual contract refs are product contracts, not decorative metadata.
- SQL remains a future derived read/query/index layer per ADR 0035. It does
  not become the canonical write store.

## Validation path

The first accepted slices must prove:

1. CLI can scaffold, verify, and publish PageDefinition/PagePackage metadata.
2. CLI can search, traverse, and inspect references over latest Docs
   projections.
3. CLI can update saved Views without making Views a second truth.
4. CLI can validate TypedRecords against explicit schema JSON without implying
   a persistent module schema exists before it is modeled.
5. CLI can produce diff/snapshot/change-report review evidence without
   dispatching mutations.
6. Cleanup commands remain governed, dry-run-first, and no-physical-delete by
   default.

