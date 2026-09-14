# ADR 0066: Retire the Work-Ledger Delegation Stack

```text
status: accepted; implemented by the W5a Work ledger slice
date: 2026-09-14
amends: ADR 0050, ADR 0058
canonical_for: whether the Work model carries a cross-Team edge; what happened to WorkDelegation, its ledgers and its HTTP readers
```

## Context

Two unrelated objects were both called "WorkDelegation".

The first was a **Work-ledger relation**: `WorkDelegation` with its state and
transition enums, `WorkDelegationEvent`, `WorkDelegationRevision`, the
`WorkOperation.delegation_revisions` side records, the crash-atomic
`work_delegation_operations.jsonl` composite and its `work_delegation_events.jsonl`
companion, the `create_work_delegation_with_target_work` / `cancel_work_delegation`
writers, the B16 actor gate, the acceptance rollup arm, and the
`/v1/work-delegations` readers. It was the only Work-to-Work edge besides
`depends_on`, and the only one that crossed Teams.

The second is `WorkDelegationV1` on the **Remote Fabric** — a different durable
object, in a different store, with its own schemas, routes and receipts. It is
untouched by this ADR.

The Owner decision for the current dogfood is that cross-Team responsibility is
out of scope for the Work model: a Team's Work is accountable to that Team, and
`depends_on` stays the only Work-to-Work edge. The Work-ledger relation had
already lost its CLI entrance to a `RETIRED_WRITE_AUTHORITY` refusal, so what
remained was a durable schema and a set of readers no current surface reached.

## Decision

Retire the Work-ledger delegation stack: types, validation, store writers and
folds, the B16 gate, the acceptance rollup arm, the HTTP readers, the two
schemas with their fixtures, and the dashboard types. The Remote Fabric's
`WorkDelegationV1` and everything under `firm-store/src/collaboration/`,
`firm-cli/src/fabric_runtime/`, `firm-fabric` and `schemas/collaboration/`
stays exactly as it is, including the DOC-108 exporter's classification of
`collaboration-v1` as current (not retired) state.

## Export obligation

Hard Invariant 3 requires an export before a ledger or its code is deleted.
The inventory found **no rows to export**: no `work_delegation_operations.jsonl`
or `work_delegation_events.jsonl` file exists under `~/.harness/`, the
`.visual-evidence/` stores, or the repository, and no `WorkOperation` anywhere
carries a non-empty `delegation_revisions`. No export path is added, because
there is nothing an export could contain.

## Read tolerance

Retirement never costs a store its history, and two shapes are pinned by
`retired_delegation_files_are_ignored_and_legacy_rows_still_fold`:

- `WorkOperation` is not `deny_unknown_fields`, so a legacy row that still
  carries `delegation_revisions` folds without a deprecated placeholder or a
  custom deserializer, and nothing serializes the field back.
- A store directory that still holds either delegation ledger opens normally.
  The files are left exactly as the store held them, no reader folds them, and
  their rows never become Work.

## Consequences

- The Work model has no cross-Team edge. Cross-machine collaboration is the
  separate Remote Fabric surface; where it lands responsibility on another node
  it creates an ordinary Work accountable to that node's Team, through the
  ordinary Work writer, never a second Work-to-Work edge.
- `WorkRef` retired with the relation that was its only user.
- `/v1/work-delegations` GETs are gone; the retired-writer guard on those paths
  stays, so a caller gets a typed refusal rather than a silent 404 shape change.
- ADR 0050 and ADR 0058 keep their historical text; this ADR supersedes their
  cross-Team delegation clauses only.
