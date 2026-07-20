# ADR 0029: Agent-programmable document runtime over canonical company facts

## Status

Accepted for target architecture; implementation is staged.

## Context

The Company OS needs the clarity, nesting, tables, and database views associated
with Notion-like products. It also operates in an Agent-native environment:
Agents can design and implement high-value pages directly, so every complex
surface need not be hand-assembled from one universal editor.

Arbitrary generated HTML cannot own business data or authority. Without a
stable semantic substrate, it would duplicate values, bypass permissions and
approvals, and become impossible to reorganize safely.

## Decision

Provide three progressive page tiers:

1. basic rich Documents with common Blocks and ordinary tables;
2. structured pages composed from standard saved Views over TypedRecords and
   Relations; and
3. optional registered Custom Pages implemented in HTML/React for stable core
   operating surfaces.

All tiers share the canonical substrate:

```text
Document / Block
TypedRecord / Relation / View / BusinessModule
Actor / WorkItem / Approval / FinancialRecord / Metric
```

A Custom Page consists of two distinct objects:

- `CustomPageDefinition`: governed registration containing purpose, owner,
  allowed queries, commands, component version, fixture, fallback, and policy;
- `CustomPagePackage`: versioned HTML/React implementation artifact.

The runtime exposes only scoped Queries, approved shared components, and named
Action Commands. Page code has no direct store client, credentials, arbitrary
network access, or authority to approve, pay, file, publish, grant permission,
or mutate canonical records.

Every Custom Page has a standard Document/View fallback. Core pages follow the
visual contract: expected design, approved fixture, implemented capture,
comparison, accessibility/command checks, and final acceptance.

## Consequences

- Star Harness adopts a Notion-like experience, not Notion's full technical
  architecture or assumption that every interface is manually composed.
- Most documents remain simple; custom code is justified only for stable,
  repeated, multi-information operating questions.
- `company-module-designer` establishes the business module contract;
  `company-page-builder` implements an approved page contract. Both are
  optional capabilities and cannot approve their own output.
- Renderer failure never makes company knowledge inaccessible.
- Visual success cannot compensate for missing relations, permissions,
  approvals, provenance, or canonical values.

## Validation

The Trademark Management module must expose the same application, WorkItem,
human Approval, ¥3,000 commitment, participants, and evidence through a basic
document, standard views, and a custom module page without duplicating or
directly mutating those facts.
