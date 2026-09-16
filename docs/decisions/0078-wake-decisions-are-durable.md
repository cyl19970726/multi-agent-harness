# ADR 0078: Wake decisions are durable; the ClaimHint vocabulary is retired

```text
status: Accepted — Owner 2026-09-16 (core 6, decision D6); implementation tracked by Task
        CORE6-X4-20260916
date: 2026-09-16
amends: no ADR. ADR 0050's pull model (a member is woken only when a state-change predicate
        holds) is unchanged, and so is every arm's placement, including ADR 0077's
        informational-idle arm. This adds a record of the decision and renames one variant.
canonical_for: what durable evidence exists for why a member ran a cycle, and what an idle
        stretch costs in rows
baseline: master 537dad37
```

## Context

Everything a wake causes is durable. The provider turn is in the native session, the Work
transition is in `work_operations.jsonl`, the message ACK is on the
`CanonicalMessageDelivery`, the cycle outcome is a `MemberAction`. The wake itself was not:
`decide_wake` returned a `WakeDecision` value, the driver matched on it, and the value went
out of scope. Nothing wrote down why.

So "why did this member run a cycle at 03:14?" could only be answered backwards, by inferring
a cause from its effects. That inference is lossy in the exact places it matters:

- Three different arms deliver an `ActiveWorkContinuation`. From the effect alone they are
  indistinguishable.
- Two paths deliver a `Work`: the eager claim at the top of the poll, which runs **before**
  `decide_wake` is consulted at all, and the `DeliverPending` arm.
- A predicted arm whose claim has already been taken by another generation produces no effect
  whatsoever (#584). It leaves nothing behind. **This ADR does not change that** — see
  [What is deliberately not recorded](#what-is-deliberately-not-recorded).

This is not a hypothetical cost. The X4 scope that produced this ADR opened by proposing to
delete `WakeDecision::ClaimHint` as dead code, and the evidence for "dead" was that nobody
could see it working — see [What the hint vocabulary was](#what-the-hint-vocabulary-was).

## Decision

### One row per wake decision

Every wake that starts a cycle writes one `team_run_event`:

```text
entity_type: member_run
entity_id:   <MemberRun id>
operation:   wake_decided
summary:     <arm> <trigger key>
```

The arm is one of `EagerClaim`, `CanonicalMessages`, `HostAttentions`, `DeliverPending`,
`Continue`, `ClaimBoardWork`, `DeliverInformational`, `Acceptance`. The trigger key is
Harness-owned ids only — `work=<id> version=<n>`, `messages=<n> first=<delivery id>`,
`acceptance=<event id> work=<id>`, `attentions=<n> first_work=<id>`. **No provider content**:
not a prompt, an answer, a Work title, or a message body. The same writer contract the
`MemberAction` doc comment states applies here unchanged.

The row is written when the decision is made, which is before the provider turn it
authorizes. It records the decision, not the outcome; the `MemberAction` rows own the
outcome. A decision whose cycle then fails still happened.

**The arm is named, not derived.** `record_wake_decision` is handed the `WakeDecision` that
actually fired, because the three ambiguities above cannot be recovered from the delivery.
The `ClaimBoardWork` case is the reason this matters: a row that called it `Continue` would
assert the member resumed a Work it owned, and it owned nothing a moment earlier.

Two arms are deliberately excluded. `Degraded` and `CloseRequested` already write their own
events at the same instant (`degraded` in the wake loop, `updated` in `close_member_runtime`),
and a second row would break "exactly once per decision".

### An idle stretch costs two rows, not sixty

`Sleep` is the common case and would be the expensive one: the backoff is 500 ms doubling to a
30 s ceiling, so a member idle for an hour polls about 120 times. A row per poll would be 120
rows saying the same thing.

Instead an idle **episode** — the unbroken run of polls that found nothing — writes at most two
rows:

- `wake_idle_capped`, once, when the backoff first reaches `backoff_max_ms`. Past the ceiling
  every poll is identical, so one row describes the whole tail. A latch on `WakeBackoff` makes
  it once per episode, and `reset` clears the latch so the next episode records its own.
- `wake_idle_ended`, written by the wake that ends the episode, carrying the poll count. This
  is the one fact no other row holds: without it a reader cannot tell a two-poll gap from a
  two-hour one.

An episode shorter than the ceiling writes only the closing row. An episode that never ends
(the member is closed while idle) writes only the cap row.

`WakeBackoff` holds the episode state because that is where `reset` already lives — the wake
that resets the backoff is the same event that ends the episode. It reads no clock: the
episode is measured in polls, and `firm-runtime-supervisor` stays I/O-free.

### What is deliberately not recorded

A **mispredicted** arm — `decide_wake` said `DeliverPending`, the delivery was already taken
by another generation, the poll returns `Retry` — writes nothing. The row is written on the
wake, not on the prediction.

That is a deliberate omission and not an oversight. Recording predictions would put a write on
the one path that has no effect and no bound: a member whose board keeps being emptied by
faster peers would write a row per poll forever, which is exactly the cost this ADR spends its
other half avoiding. A misprediction is also not a coordination fact about this member — the
member did nothing — it is contention, and the row that matters belongs to whoever won the
claim, who writes their own.

The consequence is honest and worth stating: the rows answer "why did this member run a
cycle" completely, and "why did this member NOT run one" only through the idle-episode rows,
which cannot distinguish a member that found nothing from one that lost every race. #584's
symptom (a mispredicted arm re-entering the loop with no sleep at all) is closed by the
`Retry` gate rather than by evidence, and stays that way.

### Why a team_run event and not a new record type

A wake decision is a coordination fact about one MemberRun, produced by the Supervisor,
ordered against the rest of that run's history, and read by humans and by
`firm team-run events`. That is the definition of a `TeamRunEvent`, and it already has the
sequence discipline (`fold_event` assigns `seq` under the run's write lock), the
`member_run_id` attribution, and a reader. A new ledger would add a journal, a schema, a
reader, a retention story, and a second ordering to reconcile — for a row that is one line of
text. `operation` uses the existing flat vocabulary (`updated`, `created`, `degraded`, …)
rather than a namespaced `wake/decided`, so the existing reader needs no change at all.

### Write cost, stated honestly

§7 of the core-6 model page counts **16 durable writes per nominal cycle**. This adds **one**:
the `wake_decided` row. A cycle that also ends an idle episode adds a second. The `Sleep` path,
which is where a member spends almost all of its wall time, adds at most two rows per episode
no matter how long the episode runs — and that is the point of the cap latch.

The honest comparison is not 16 → 17 in isolation. A member idle for an hour between two
cycles previously wrote 0 rows for that hour and now writes 2. A member in a tight
continuation loop writes one extra row per cycle, a ~6% increase on the nominal 16.

## What the hint vocabulary was

`WakeDecision::ClaimHint(Vec<String>)` carried the ids of every board Work the member was
eligible for, and its comment said it would "inject a lightweight prompt so the member can
discover and claim eligible Works."

Both halves were false. The driver bound the payload as `ClaimHint(_work_ids)` and discarded
it. No prompt was injected: the arm called `active_work_continuation_for`, which re-derived
the Work itself. The core-6 audit's Q13 read the discarded payload as the feature and
concluded the arm was dead, and the X4 scope inherited that conclusion.

The arm is not dead. It is the **only** wake that reaches board Work. Every other path filters
on `owner_member_id == this member`:

| path | filter | file |
| --- | --- | --- |
| eager claim | `owner_member_id == member` | `member_work_coordination.rs:170` |
| `DeliverPending` | `queued_works_for` — same | `member_work_coordination.rs:939` |
| `Continue` | `is_active_work_continuation_candidate` — same | `member_work_coordination.rs:402` |
| **this arm** | `owner_member_id.is_none() && claim_mode == TeamClaim` | `member_work_coordination.rs:1007` |

An unclaimed board Work has no owner, so only the last row can see it. Downstream,
`active_work_continuation_prompt` has a whole branch keyed on
`Open && Normal && owner_member_id.is_none()` that emits `SHARED WORK AVAILABLE` and tells the
member to claim atomically — text reachable through this arm and nothing else
(`provider_interactions.rs:1201`).

So: the **payload** was dead and the **name** was a lie; the **wake** was load-bearing. The
variant is now `ClaimBoardWork` with no payload, the comments on both sides say what actually
happens, and "hint" is gone from the module doc, the predicate and the test names. One
deliberate historical note stays on each side so the next reader learns why the two were
conflated instead of repeating it.

### The test that was missing

Neither the arm nor the `SHARED WORK AVAILABLE` branch had a single test anywhere in the
repository. That is why deleting the arm looked safe: nothing would have gone red.

`idle_member_claims_an_unclaimed_board_work` is that test. An idle member with no Active Work
of its own, an unclaimed `team_claim` Work placed on the board by nobody in particular, driven
through the real serve loop; it asserts the member is woken with that Work on the shared-board
branch, and that the decision row says `ClaimBoardWork`. Red-verified by deleting arm 6 from
`decide_wake`: the test fails at its deadline with only the member's own initial prompt in the
provider log (93.4 s), and passes in 3.8 s with the arm restored.

This is the test that would have caught the regression this slice almost shipped, and it
exists because the premise was checked instead of assumed.

## Consequences

- `firm team-run events` gains three operations with no reader change. An operator can read
  why a member ran each cycle, and how long it sat between them.
- Adding a wake arm without a row is now hard to do by accident: the row is written by the
  wrapper around `poll_idle_member_wake`, not at the nine `Ready` sites inside it, so a new arm
  is recorded by construction. What a new arm must still do is teach `wake_decision_arm` its
  name — an arm that forgets is recorded under a neighbouring name, which is the failure mode
  this ADR exists to prevent, so that mapping is the thing to review.
- The rows make the eager/predicate split visible for the first time: response-required mail
  is claimed by `claim_canonical_messages_for_member` above `decide_wake` and records
  `CanonicalMessages`, never `DeliverPending`. That was true before and invisible before.
- `Acceptance` is recorded as `Acceptance` although `decide_wake` returned `Sleep` — the
  driver has one durable check inside the `Sleep` arm that the pure view cannot see. The wake
  is the fact, so the wake wins over the decision there.
- `WakeBackoff::consecutive_sleeps` is no longer test-only. `at_cap`, `cap_recorded` and
  `mark_cap_recorded` are new and pure.
