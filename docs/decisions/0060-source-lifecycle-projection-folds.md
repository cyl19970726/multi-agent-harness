# ADR 0060: Source/Lifecycle Projection Folds Fail Closed

```text
status: accepted; implementation owned by DEV-74
date: 2026-08-24
amends: ADR 0040, ADR 0058
canonical_for: CanonicalWorkDelivery revision fold and HostAttention source/lifecycle fold
```

## Context

Two current projections combine an immutable coordination fact with later
lifecycle state. Canonical Work delivery is emitted as full revisions in trust
side records. HostAttention begins as a canonical causal source fact and then
records transport lifecycle in `host_attentions.jsonl`.

A generic latest-by-id fold silently lets a later row redefine identity,
downgrade a version, skip a revision, or invent an illegal lifecycle. Folding
HostAttention source rows after lifecycle rows can also reset an acknowledged
notification to Actionable. These are authority errors, not display defects.

The retired `ProviderWorkDispatch`, old WorkDelivery update JSONL, APIs, and
fallbacks are already deleted. No historical WorkDelivery data requires
migration.

## Decision

Application code owns two concrete fold contracts; Store code supplies ordered
records and fails closed on every violation.

### CanonicalWorkDelivery

- Trust side records are the only source. No legacy-only or mixed-authority row
  is admitted.
- Delivery id, Work id/revision, execution binding, recipient AgentMember,
  recipient AgentSession id/generation, target Node, and creation time are
  immutable.
- Version 1 is exactly Queued attempt 1 with no claim, receipt, or failure.
- Revisions advance by exactly one through `Queued -> Claimed`, followed by
  `Claimed -> ProviderReceived` or `Claimed -> Failed`.
- An exact same-version replay is a no-op. Same-version drift, version
  regression/gap, identity drift, and terminal-state mutation fail closed.

### HostAttention

- Canonical trust side records own the immutable causal fact: id, TeamRun,
  kind, Work id/version, source event, related MemberRun, and creation time.
- Canonical sources are initial Actionable attempt-zero snapshots and fold
  before lifecycle rows.
- Lifecycle rows may advance only through the existing reviewed claim,
  delivery, acknowledgement, retry, or escalation transitions. Claim and
  receipt fences must remain exact.
- A structurally valid legacy-only HostAttention remains readable for the
  current notification compatibility seam. It is never Work, Message, or
  WorkDelivery authority.

## Consequences

Projection corruption becomes an explicit read failure instead of a plausible
but false current state. Restart and replay are deterministic. Store writers
must produce legal full projections, and structural gates keep the deleted
WorkDelivery authority from returning.

This ADR does not add a generic event framework, migration ledger, dual-write,
retired Work containment topology, or provider retry semantics.
