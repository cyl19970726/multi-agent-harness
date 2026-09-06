# ADR 0064: HostAttention Is a Delivery Ledger, Not a Fourth Authority Plane

```text
status: accepted; Owner decision 2026-09-05 (SPEC-ADAPTATION-REFACTOR-01 D-A, Review 04 Pass)
date: 2026-09-05
amends: ADR 0060 (HostAttention fold); SPEC-ARCH-BOUNDARY-01 plane wording (Notion errata)
canonical_for: the authority-plane count of the coordination model and HostAttention's exact role in Work mutations
```

## Context

Two accepted authority documents disagreed. Notion `02 · Work 与 Message`
states that Work, Message, and RuntimeCommand are the three independent
planes; SPEC-ARCH-BOUNDARY-01 listed HostAttention as a fourth. The
architecture review in #765 (finding F5) asked for one answer.

What the code does at master `875adb05` (the merge base of the pull request
that introduced this ADR):

- `HostAttention` (`crates/firm-core/src/work.rs`) has twelve kinds. Most are
  derived from Work operations by the Store
  (`store_work_graph.rs`
  `ensure_downstream_host_attentions_for_work_operation_unlocked`) and can be
  rebuilt (`store_host_attention.rs` `reconcile_work_host_attentions`);
  `HostBindingStale` is minted from the TeamRun, the Host binding lease, and
  the clock. Three kinds have no producer and `escalate_host_attention` has no
  production caller.
- Each attention has its own transport lifecycle in `host_attentions.jsonl`:
  `Actionable → Claimed → Delivered → Acknowledged` (or
  `EscalationRequired`), with exact claim fences (external Host thread or
  managed session plus daemon generation) and a provider receipt (ADR 0060).
- Exactly two Work-plane decisions read it. `retarget_work_execution`
  refuses with `HOST_ATTENTION_PENDING` until the Host acknowledges (or an
  escalation clears) every
  attention that still needs Host action (`store_work_mutations.rs`); and the
  terminal-Work provenance fold uses a well-formed `WorkReviewRequested` row as
  evidence of the submitting MemberRun (`store_work_graph.rs`). Every other
  reader is a projection (RoleView, dashboard, inbox).

So HostAttention is derived and rebuildable like a delivery projection, yet
its acknowledgement is state only the delivery ledger holds, and one Work verb
depends on it.

## Decision

1. **Three authority planes remain**: Work, Message, RuntimeCommand.
2. **HostAttention is the Host-notification delivery ledger**, the peer of
   `CanonicalWorkDelivery` and `CanonicalMessageDelivery`: three authority
   planes, three delivery ledgers. Its source facts are derived from the
   planes; its lifecycle is transport state.
3. **It authorizes nothing.** S9 removes the two historical readers described
   above. The exact Host's versioned `ExecutionRetargeted` expresses its Work
   disposition; notification ACK is not a prerequisite and no Work ACK state
   or extra operation is introduced. Terminal execution provenance comes from
   immutable Work submission operations or the atomic Result and its exact
   execution binding/admission. Missing or ambiguous provenance fails closed.
4. The three producer-less kinds are historical decode-only values, rejected
   by current creation; old rows and `EscalationRequired` remain readable. The
   unreachable escalation writer is removed. Folding Host notifications into
   Messages remains a separate future decision.

S9 clarification (2026-09-06): accepted
[DEV-235-SPEC-v1](https://app.notion.com/p/3d349a4fa3798151a2d0cd6c104b1e1d)
records the Owner-delegated decision. It replaces the old Task's illustrative
ACK field and un-ACKed-refusal test, while retaining A-narrow. The local CLI
remains a trusted same-user operator proxy, with exact Host membership and
Work CAS checks; the old HTTP ACK's target-run-derived surface/thread was not
independent caller authentication. S9 introduces no HTTP retarget endpoint.

Rejected: declaring a fourth authority plane (it would legitimize a second
inbox ledger, which the AgentInbox invariant forbids); folding HostAttention
into Messages now (it would move a Work precondition onto the Message plane,
against Hard Invariant 2).

## Consequences

- Work decisions no longer read notification lifecycle records. Existing
  Work version, exact Host, successor Team scope, nonterminal state, and
  claimed-delivery reconciliation checks remain in force.
- Historical enum decoding is preserved rather than narrowing persistent
  schemas. HostAttention remains a delivery ledger, not a second Work ledger.
- ADR 0060's notification fold rules are unchanged.
