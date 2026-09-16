# ADR 0077: Informational Messages get a delivery guarantee

```text
status: Accepted — Owner 2026-09-16 (core 6, decision D5); implementation tracked by Task
        CORE6-X3-20260916
date: 2026-09-16
amends: no ADR. ADR 0068's delivery model (Queue is the only delivery; there is no mid-cycle
        injection) is unchanged, and #941's "fold queued mail into the next cycle" is
        unchanged. This adds the case that path cannot reach.
canonical_for: when an authored Message is guaranteed to reach its recipient's provider, and
        the worst-case latency of that guarantee
baseline: master 76ff8336; the S1 dogfood store copy for the evidence
```

## Context

An authored Message is queued as a `CanonicalMessageDelivery` and reaches the provider only
when a cycle runs. Two paths exist:

1. A **response-required** Message (or a `ProviderInteractionResponse`) wakes the member on
   its own — wake predicate arm 3, and `require_round_trigger` in `runtime_effects.rs`.
2. Anything else — ordinary **informational** mail — is folded into the next cycle that runs
   for some other reason: a Work delivery, a continuation, an acceptance, a Host attention
   (#941).

Neither path covers a member that is **idle with no Work**. Nothing wakes it, so no cycle
runs, so nothing to fold into. The mail simply waits.

That is not hypothetical. In the S1 dogfood store, **63 of 111 authored Messages never
reached a provider, 60 of them informational** — not delayed, not dropped by a bug, but
sitting correctly queued against a member the loop had no reason to run. The Host was
writing to an audience that could not hear it, and every layer reported success: the Message
was authored, the delivery was `queued`, the member was healthy and idle.

The honest reading is that "informational" had no delivery guarantee at all. It had a
delivery *opportunity*, conditional on unrelated work arriving.

## Decision

**An informational Message is delivered at the next cycle boundary (#941, unchanged) or,
when the recipient has been idle with no other reason to run a cycle for
`informational_idle_delivery_ms`, on a dedicated Messages boundary of its own.**

### The interval is 120 seconds, and it is not configurable

`WakePolicy.informational_idle_delivery_ms = 120_000`, alongside the other wake constants and
non-configurable for the same reason they are: `effective_wake_policy()` is one source, and a
per-run override would let two readers of the same policy disagree.

Why 120 s specifically — it is bounded on both sides:

- **Not shorter**, because the ordinary path is still #941. A member that is about to be
  given Work, or to pick one off the board, will receive its mail on that cycle for free. A
  short interval would interrupt it one cycle early and spend a provider turn on mail that
  was already going to be delivered — the exact cost the "fold into the next cycle" design
  exists to avoid. Two minutes is comfortably longer than the gap between a member going idle
  and the Host giving it the next thing to do.
- **Not longer**, because the failure it closes is a Host note going unread. In the S1
  evidence the wait was unbounded; tens of minutes would still read, to an operator, as "the
  member ignored me".

### Worst-case latency, stated honestly

`N + one backoff tick`. The predicate is evaluated by a timed poll, not delivered by an
event, and the idle backoff climbs 500 ms → 1 s → 2 s → … → 30 s. So an informational Message
authored the instant after a poll, to a member already at the top of its backoff, is
delivered at worst **120 s + 30 s = 150 s**. A member that has just gone idle is polling far
more often and sees it in about 120 s.

This is a guaranteed upper bound on *delivery to the provider*, not on the member acting on
it. Provider satisfaction never implies Host acceptance (invariant I6), and this ADR does not
change that.

### Where the arm sits, and why there

`decide_wake` gains one arm (`WakeDecision::DeliverInformational`), **below every arm that
already has a reason to run a cycle** — degraded, delivery/message pending, continuation,
probation, claim hint — and **above `Sleep`**:

- Below them, because any of those cycles carries the queued mail for free under #941.
  Firing above them would deliver the same mail one cycle sooner at the cost of an extra
  provider turn, and would let mail pre-empt Work.
- Above `Sleep`, because `Sleep` is the state in which the mail was being stranded.

Response-required mail therefore still wakes immediately and takes the informational batch
with it; the new arm only ever fires when nothing else would.

### The idle clock is durable, not process-local

`MemberWakeView.idle_for` is computed from `MemberRun.last_event_at` while the member's
status is `Idle`. That field is stamped by the status change that made the member idle and is
not rewritten while it stays idle, so it is a durable row in `member_runs.jsonl`.

This is deliberate and load-bearing. A fresh process-local counter would restart on every
daemon generation — on every restart, every Supervisor replacement, every recovery — and a
member whose daemon is recycled more often than the interval would never accumulate enough
idle time to earn a delivery. The guarantee would silently not hold in exactly the
circumstances where a Host is most likely to be sending notes. An unknown or unparseable
stamp yields `None` and never fires the arm: no clock, no wake.

Two things that rationale does not by itself cover, recorded here because the X3 review was
right that they were reasoning gaps rather than defects:

- **"Never started" is a real `None`, and it is unreachable where the arm runs.** A freshly
  created MemberRun genuinely carries `last_event_at: None` together with `status: Idle`
  (`team_run_setup.rs:239`, `:262`), so "unknown stamp forever" exists in the data. It cannot
  be observed by this predicate: the arm is only ever evaluated inside a member's own
  supervisor loop, and that loop exists only after a start which stamps the row before the
  first poll. The `None` default is therefore a fail-closed guard for legacy or corrupt
  stamps, not the live case.
- **`last_event_at` has roughly forty writers, and the error direction is delay-only.** It is
  a general member-row activity stamp, not a cycle-boundary field, so "not rewritten while
  idle" is a property of its call sites rather than an invariant. The ones worth naming,
  because they are the ones that could plausibly touch an idle row: the message claim chain
  writes no member row at all (`runtime_effects.rs` `claim_canonical_messages_*`); authoring
  or queueing a Message writes none; the Supervisor lease stamps on Close, a lifecycle
  transition, not on renewal (`supervisor_control.rs:423`); a provider-profile refresh returns
  early when unchanged (`team_provider_profiles.rs:1160-1165`); the gateway and health writers
  stamp a `ProviderProcess` row, not `member_runs` (`delivery_gateway.rs:273`,
  `gateway_runtime.rs:466`); and capacity writers are admission and recovery paths, whose
  members are `Blocked` and excluded by the degraded arm regardless.
  A future writer that did stamp an idle row would only *delay* delivery by one more
  interval. It cannot cause an early fire, and it cannot cause a loss: any cycle that ran at
  all already emptied the informational queue through #941, so there is nothing left for a
  later stamp to strand.

## Consequences

- An idle member with queued informational mail now runs one provider cycle it would not have
  run before, at most once per idle interval per queued batch. The batch is delivered whole,
  on one boundary, with that cycle's own provider receipt — not one turn per message.
- `member_actions` will show `turn_completed` (or `empty_provider_round`) rows for members
  that previously sat still. That is the guarantee working, not a regression.
- A member the loop has deliberately stopped driving is unaffected: the degraded arm is
  evaluated first, so mail cannot restart a member that is `Blocked` awaiting Host
  intervention.
- The wake remains a timed poll, not a push. This ADR bounds the latency; it does not make
  delivery event-driven, and the bound above is the honest number.
- `FIRM_TEST_INFORMATIONAL_IDLE_DELIVERY_MS` exists as a test-only seam, the same shape as
  `FIRM_MEMBER_SUPERVISOR_TEST_IDLE_MS`, because no deterministic test can wait 120 s.
  Nothing in production sets it.

## Alternatives rejected

- **Wake on every informational Message.** Simplest and wrong: it discards the #941 design,
  spends a provider turn per note, and lets mail pre-empt Work.
- **Make the interval configurable per run.** Same objection as every other `WakePolicy`
  constant — two readers of one policy would be free to disagree, which is how the
  degradation threshold and the recover classifier once desynced.
- **Push delivery instead of a poll.** A real improvement, and much larger than this slice:
  every predicate in this loop is a timed Store poll today. Bounding the latency of the one
  case that had no bound at all is worth doing before rebuilding the mechanism.
- **Count informational mail in the existing `DeliverPending` arm.** That arm wakes
  immediately, which is exactly the interruption the interval exists to avoid.
