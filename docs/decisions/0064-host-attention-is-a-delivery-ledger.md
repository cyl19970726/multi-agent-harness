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

What the code does at master `875adb05` (the merge base of this ADR):

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
  refuses with `HOST_ATTENTION_PENDING` until the Host acknowledges every
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
3. **It authorizes nothing, with one documented exception.** An
   un-acknowledged HostAttention is the Host-intake precondition of exactly
   one Work verb, `retarget_work_execution`, and a `WorkReviewRequested` row is
   provenance evidence for a terminal Work. Both facts are stated in
   `docs/current/architecture/agent-runtime.md` instead of the previous
   absolute "delivery does not authorize Work mutation".
4. A follow-up slice (S9 in SPEC-ADAPTATION-REFACTOR-01) re-expresses those
   two gates on Work state itself, removes the three producer-less kinds and
   the unreachable escalation path, and only then may folding Host
   notifications into the Message plane be re-evaluated.

Rejected: declaring a fourth authority plane (it would legitimize a second
inbox ledger, which the AgentInbox invariant forbids); folding HostAttention
into Messages now (it would move a Work precondition onto the Message plane,
against Hard Invariant 2).

## Consequences

- Documentation is repaired: Notion `02` carries the delivery-ledger sentence,
  SPEC-ARCH-BOUNDARY-01 carries an errata note, and `agent-runtime.md` names
  the exception. No code changes in this ADR.
- `http_team_actions.rs` is the attention claim/acknowledge writer; a change
  to it is a kernel-tier change because it feeds the retarget gate.
- ADR 0060's fold rules are unchanged; this ADR only names what the folded
  projection is.
