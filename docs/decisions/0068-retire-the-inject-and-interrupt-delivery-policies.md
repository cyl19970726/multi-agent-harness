# ADR 0068: Retire the Inject and Interrupt Delivery Policies

```text
status: accepted; implemented by the 2026-09-14 C2 cutover
date: 2026-09-14
amends: ADR 0039 durable mailbox delivery; ADR 0044 typed mail; ADR 0056 message cutover
canonical_for: which TeamDeliveryPolicy values exist; whether a mid-cycle injection path exists
```

## Context

`TeamDeliveryPolicy` had four values. Two of them did not survive contact with
the product:

- **`Interrupt` had zero references repo-wide.** Not one writer, reader, match
  arm, test or fixture. It was a name in an enum.
- **`Inject` was the mid-cycle steer path**, and by the time C1 landed it was
  the last thing that could put content into a running provider cycle. It ran
  through its own vertical slice: two `ControlIntent` variants, two
  `SemanticCapability` values, `SteerRequest` / `SteerProviderResult` /
  `CycleControl.injects`, an `on_steer_result` callback threaded through the
  `run_cycle` trait declaration and all twelve of its implementations plus
  Pi's `prompt` / `prompt_dyn` / `apply_cycle_control`, a
  `PendingSteerSettlement` with a
  `Drop` that had to fail a caller who never got a provider settlement, a
  `POST /v1/team-runs/{id}/members/{id}/steer` route, a
  `LiveMemberControlRequest::Steer`, a `TeamMessageDeliveryMode::InjectDelivered`
  that marked a provider member's mail *delivered at creation time*, and dead
  dashboard exports.

Only two providers ever claimed it — Codex `Experimental` (`turn/steer`, never
canaried) and Pi `Supported` (`steer` / `follow_up`) — and the durable record
agrees with that emptiness: **zero** persisted RuntimeCommand rows of kind
`inject_current_cycle` or `queue_at_native_boundary`, and **zero** persisted
`TeamDeliveryPolicy` values of any kind (`team_messages.jsonl` does not exist on
the live machine).

The cost was not the code. It was the second delivery semantics. `Inject` was
the one path that could mark mail delivered without a provider receipt, and the
one path that could put content into a cycle the Host had not started.

## Decision

1. **Two delivery policies: `Queue` and `ManualAck`.** `Inject` and `Interrupt`
   are deleted, not reserved. Nothing ever persisted them, so a document
   carrying one now fails decoding rather than implying a delivery mode no
   writer can produce. `ManualAck` keeps its exact meaning: the Host control
   plane receives member-originated mail at creation time.

2. **No mid-cycle injection path exists.** `ControlIntent` is down to
   `StartCycle` and `Interrupt`. `SemanticCapability` is down to nine values
   (`ALL` 11 → 9). `SteerRequest`, `SteerProviderResult`,
   `CycleControl.injects`, the `on_steer_result` callback,
   `supports_inject_current_cycle` and `supports_native_boundary_queue` are
   gone from the contract and from every `run_cycle` implementation; the
   Codex `turn/steer` client and bridge method, Pi's `steer` compilation and
   dead `follow_up()`, and the Claude/DeepSeek/Kimi "not applied" arms go with
   them.

   The Host still has two real ways to change a member's course: interrupt the
   current cycle, or send ordinary mail that the member picks up at its next
   one. Neither races the cycle.

3. **The command kinds are frozen, not deleted.**
   `RuntimeCommandKind::{InjectCurrentCycle, QueueAtNativeBoundary}` join the
   frozen list, exactly as ADR 0067 froze the continuation kinds: wire values
   stay readable and exactly replayable, and a new prepare is rejected with
   `RUNTIME_COMMAND_KIND_FROZEN` before any classification or capability check.
   The `runtime-command-record.schema.json` `command_kind` enum therefore
   **keeps** both values — a frozen kind must stay decodable for historical
   replay, which is the same reason every other frozen kind is still listed
   there.

4. **One delivery shape, not two.** `TeamMessageDeliveryMode` is deleted and
   `prepare_team_message_as` computes policy/status/attempt directly. The
   Host-inbound case (`ManualAck` / `Delivered` / attempt 1) and the ordinary
   case (`Queue` / `Queued` / attempt 0) are unchanged; only the third branch,
   which existed solely for `InjectDelivered`, is gone.

5. **Retire the dead reader clause with it.** `queued_messages_for` filtered
   `delivery.policy != Inject && delivery.status == Queued`, and dropping the
   first clause leaves the one production caller's count unchanged. The proof
   is not "Inject rows were always Delivered" — two sites created them
   otherwise — it is the caller:

   `member_lifecycle.rs` is the only caller, and it post-filters
   `.filter(|message| message.requires_response())` before counting.
   `requires_response()` is true only for `ResponseRequired`. Every message
   whose delivery carried `policy: Inject` was declared
   `response_intent: Informational` — the provider-interaction response in
   `http_member_control.rs`, and the transient in-memory projection in
   `runtime_effects.rs`, which is `Claimed` rather than `Queued` and is never
   persisted. So the `requires_response()` filter already excluded every row
   the `policy != Inject` clause could have excluded, and the wake count is
   identical before and after.

## What is kept

`TeamDeliveryPolicy::{Queue, ManualAck}`; the S6 member→Host request that sends
`ManualAck`; `legacy_team_messages()` and the whole legacy read/export path; and
every line of `interrupt_current_cycle` — interrupt is the control that
survives, and it is the one the Host should reach for. Pi keeps
`observe_native_queue`: `get_state` is a read-only snapshot, not a queue you can
write to.

## Consequences

- Capability fingerprints change again for all five providers, by the same
  mechanism and with the same bounded, fail-closed divergence window described
  in ADR 0067: a running member keeps driving on the fingerprint stored in its
  AgentSession, and an effect attempted between a profile re-finalize and the
  next session bind is refused with `RUNTIME_ADAPTER_FENCE_INCOMPLETE` or
  `RUNTIME_ADAPTER_PREFLIGHT_FAILED`.
- `POST /v1/team-runs/{id}/members/{id}/steer` is gone. The dashboard's
  `steerTeamMember` and `liveSteerCapability` were already dead exports with no
  call sites, and `apps/agent-dashboard/tests/operator-controls-check.mjs`
  asserted on `MemberRuns.tsx` / `Missions.tsx`, neither of which exists; all
  three are deleted.
- `LiveMemberControlRequest` is a `#[serde(tag = "command")]` wire enum, so an
  old client sending `{"command":"steer"}` now fails to decode. That is the
  fail-closed reading: the route it targeted no longer exists.

## Alternatives rejected

- **Keep `Inject` for a future provider.** The next provider that offers a real
  same-cycle injection primitive will need a fresh capability, a canary, and a
  settlement contract anyway. Keeping an unexercised one costs an entire
  vertical slice and a second delivery semantics in the meantime.
- **Delete the RuntimeCommand kinds.** Historical replay must stay exact, and
  the repository already has a freeze mechanism for precisely this case.
