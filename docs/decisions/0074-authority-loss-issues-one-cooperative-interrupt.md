# ADR 0074: Authority Loss Issues One Cooperative Interrupt Before the SIGKILL Backstop

```text
status: accepted
owner_role: architecture
amends: ADR 0065 (two runtime epochs), ADR 0068 (interrupt is one of the two
  ways to change a member's course)
canonical_for: what happens to a running provider turn when this process loses
  machine authority or a TeamRun Supervisor lease, and why that one interrupt
  is deliberately not a RuntimeCommand
```

> **Amended by ADR 0075 (E2a-2a).** "Machine authority" is now one document per machine rather than
> a bundle of per-Space lease rows, so the loss this ADR responds to is detected on that document.
> Two lease-renewal acceptance tests in this lineage were retargeted rather than deleted; see the
> retarget mapping table in
> [ADR 0075](0075-machine-lease-leaves-the-space-data-lock.md#what-executing-this-checklist-found-e2a-2a),
> rows 2 and 3. What this ADR decides — one cooperative interrupt before the SIGKILL backstop — is
> unchanged.

## Context

Two latches fence a running provider turn, and until this decision neither of
them reached the provider.

`latch_machine_authority_loss`
(`crates/firm-node-daemon/src/supervisor_daemon/machine_authority.rs`) closes
process-wide provider-effect admission, latches the loss, snapshots the served
runs, publishes Stop and drains the durable Node authorities.
`latch_supervisor_lease_lost`
(`crates/firm-cli/src/main_modules/supervisor_control.rs`) flips one
`AtomicBool` and prints a diagnostic. Both make every *future* Harness write
fail closed. Neither touches the provider process that is executing right now.

On the **machine** path a drain followed: `graceful_shutdown_with_deadlines`
(`crates/firm-node-daemon/src/supervisor_daemon/shutdown.rs`) revoked each
run's heartbeat, waited up to `SUPERVISOR_DRAIN_TIMEOUT` (30 s) for a
*cooperative* exit, then SIGKILLed every registered provider process group
(`crates/firm-runtime-host/src/lib.rs`), then joined for 5 s.

The gap is between those two sentences. A member in the middle of a turn
cannot exit cooperatively: the supervisor thread is inside
`TeamRuntimeAdapter::run_cycle`, waiting on the provider's terminal frame, and
nothing between cycle start and the terminal consults authority. So the
30-second "cooperative" window is, for exactly the case it exists to handle,
a 30-second window in which a provider keeps spending tokens and running tools
on behalf of an authority that is already gone — and is then killed mid-tool
with no chance to stop cleanly.

On the **Supervisor-lease** path there was no backstop at all, and there still
is none. `graceful_shutdown()` has exactly one call site — the daemon's own
stop path in `supervisor_daemon.rs` — and `terminate_registered_process_groups`
is called only from `shutdown.rs`. A TeamRun that loses its Supervisor lease
while the daemon keeps serving other runs reaches neither: its supervisor
thread returns `SupervisorLeaseLost`, the provider handle stays in
`session_runtimes` until a Close path removes it, and
`OwnedProcessGroupRegistration::drop` deliberately does not signal. Nothing
stopped that turn; it ran to completion under a lease it no longer held.

All five adapters already declare `interrupt_current_cycle` as `Supported`,
and the supervisor's control poll already turns `CycleControl { interrupt:
true }` into each adapter's own native interrupt primitive. The mechanism
existed; it was simply not keyed to authority loss.

Hard Invariant 7 said "Replacing a runtime drains or interrupts active turns
first". That was aspirational for the drain path: nothing interrupted.

## Decision

### 1. Both latches hand every live turn in scope exactly one cooperative interrupt

A process-local registry
(`crates/firm-runtime-host/src/authority_loss_interrupt.rs`) tracks the turns
this process is driving or is about to drive. The supervisor takes one guard
just before it queues for a turn slot
(`crates/firm-cli/src/runtime_adapter.rs`) and holds it until the cycle
returns; dropping it is what keeps a finished turn out of any fan-out.

On authority loss the latch calls `request_authority_loss_interrupt` with its
scope:

- `AuthorityLossScope::Process` for the machine latch — every live turn in
  this process was admitted under the lost machine lease;
- `AuthorityLossScope::TeamRun(id)` for the Supervisor-lease latch — only that
  run's turns. One run losing its lease must never interrupt another run.

The turn's own control poll consumes the request and returns
`CycleControl { interrupt: true }`, so the interrupt travels the adapter's
existing `interrupt_current_cycle` path. Per adapter that is:

| Provider | Execution mode | Native primitive issued |
| --- | --- | --- |
| Codex | `codex_app_server` | `turn/interrupt` on the app-server bridge |
| Kimi | `kimi_acp` | `session/cancel` ACP notification |
| Claude | `claude_agent_sdk` | `{"command":"interrupt"}` frame to `apps/claude-member-runner`, which calls the SDK's `query.interrupt()` |
| DeepSeek | `deepseek_sdk` | `{"command":"interrupt"}` frame to `apps/deepseek-member-runner` |
| Pi | `pi_rpc` | JSON-RPC `abort` request |

Exactly one interrupt reaches a turn for the life of that turn. A second latch
(the machine latch following a Supervisor latch, say) never republishes a
request the turn already carries, so a provider never sees a storm.

**Two things close the registration window, and both are required.**

*First, each latch invalidates its own scope before it fans out.* The
Supervisor latch already did: `latch_supervisor_lease_lost` swaps
`supervisor_valid` false before calling the fan-out. The machine latch did
**not** — it writes only NodeDaemon lease rows and process-wide flags
(`authority_lost`, the Store's process admission gate), and
`TeamRunLedger::require_supervisor_lease` reads neither of those, so nothing it
did was observable on the drive path. Its first machine-path observation was
`require_provider_session_authority` in the acceptance callback, *after* the
provider had accepted the prompt — and even that passes when the loss trigger
was an unreadable Store, because the NodeDaemon lease row is then still Active.
So `latch_machine_authority_loss` now quiesces every served Supervisor scope
first (`shutdown.rs::quiesce_served_supervisor_scopes`). The drain sets the same
flag moments later; this only moves it to the instant authority is known lost.

*Second, a turn registers* **before** *`acquire_prepared_cycle_turn`, not after
it.* Between the last admission check and the drive sits a blocking wait for a
turn slot plus Store write-lock IO. Registering after that wait would leave a
window — milliseconds to hundreds of milliseconds under lock contention, and
lock starvation is this repo's actual loss trigger — in which a turn is
invisible to the fan-out and starts anyway.

Together they leave no window, because the registry and the fan-out serialize on
one mutex and every latch now invalidates its scope before it fans out:

- a latch that lands after registration finds the turn in the registry;
- a latch that lands before it is caught by the `require_supervisor_lease()`
  that `acquire_prepared_cycle_turn` performs *after* the wait, and the turn
  never drives.

The registry deliberately does **not** remember a latched scope, so a turn
registered later is not born interrupted. Remembering it would permanently break
a TeamRun that a successor Supervisor generation legitimately re-adopts in the
same process. Covering the window is the caller's and the latch's job, not the
registry's.

Quiescing from the machine latch is taken without the per-Supervisor
`authority_gate`, and that race is benign for a stronger reason than timing:
`close_process_node_daemon_admission` is the *first* statement of
`latch_machine_authority_loss`, so in every interleaving where a concurrent
Close gets past the flag, process admission is already closed and
`require_current_node_daemon_unlocked` refuses its durable effect at the Store.
There is no ordering in which a Close both passes the flag and lands a provider
effect.

One evidence consequence, deliberate: quiescing here means
`latch_supervisor_lease_lost` finds those scopes already false and prints no
per-run line, so the quiesce loop prints that line itself. Machine-path evidence
is therefore the per-run quiesce line, the Process-scope fan-out report, and the
`cooperative_interrupt_dispatched` self-stop detail — which now also carries
`supervisor_scopes_quiesced`.

Codex's Close path additionally pauses an observed native Goal before
interrupting. That belongs to Close semantics and is not part of this path:
authority loss issues the interrupt only.

### 2. The interrupt is a process-local action, not a RuntimeCommand

This is the deliberate exception, and it is narrow.

Every provider effect in this system is prepared and settled through a durable
`RuntimeCommand` bound to exact NodeDaemon and AgentSession generations. Under
lost authority no such command can be admitted — process admission is closed
(`harness_store::close_process_node_daemon_admission`) or the Supervisor lease
no longer validates — and **that refusal is correct**. Weakening admission so
the shutdown path could write one more command would hand the losing generation
exactly the authority the fence exists to remove.

So the cooperative interrupt writes nothing durable. It prepares no effect,
settles no effect, and creates no `CancelProviderTurn`,
`InterruptCurrentCycle`, `StopSession` or `CloseMember` record. It is a
process-local instruction to code this process already owns, asking the
provider it already owns to stop.

The consequence is accepted: this interrupt has no durable ledger entry of its
own. Its evidence is the self-stop journal (below), the provider's own native
session record, and the daemon's stderr log.

One further consequence of routing it through the control poll: the
authority-loss check runs *before* the poll drains the Supervisor control lane,
so a Close or Interrupt RuntimeCommand already sitting in that channel is not
consumed on the poll that returns the authority interrupt. That is intended —
neither could be settled under lost authority, and the command stays `Prepared`
for ordinary recovery — and the pending request is answered on the next poll.

### 3. The interrupted terminal still hits the authority refusal

Interrupting the turn does not admit its terminal. When the cycle returns, the
terminal stage still calls `ledger.require_supervisor_lease()` and
`require_provider_session_authority` before any settlement, delivery mutation,
member action, or session transition, and under lost authority it refuses.

An interrupted turn therefore settles nothing: its admitted `StartCycle` keeps
`Unknown` certainty and `Unknown` postcondition. There is one observable
difference, and it is worth stating exactly rather than calling it identical to
the old behaviour.

The turn now *reaches* its prepared-effect scope guard
(`Drop for ProviderEffectAdmission`) instead of being killed inside the
provider wait, so the guard moves the row `Prepared -> RecoveryRequired` with
failure code `PREPARED_PROVIDER_EFFECT_SCOPE_EXITED_WITHOUT_RECEIPT`. That is a
recovery marker, not a settlement — `mark_prepared_runtime_command_recovery`
still requires the exact daemon identity, the expected version, and a current
`Prepared`/`Unknown` row — and it is the pre-existing guard's behaviour on every
drain in which the supervisor thread finishes rather than being killed.

It does change which rows the finished-Supervisor reaper sees.
`team_run_has_unresolved_runtime_command`
(`crates/firm-node-daemon/src/supervisor_daemon/recovery.rs`) matches only
`Prepared` + `Unknown` + `Unknown`, so a row already moved to
`RecoveryRequired` no longer causes
`block_finished_supervisor_if_unresolved` to write a
`TEAM_SUPERVISOR_EXITED_WITH_UNRESOLVED_RUNTIME_COMMAND` block. This slice makes
that transition happen more often, because turns now exit cooperatively. Whether
an explicit `RecoveryRequired` row should also hold adoption is a real question,
but it is a question about the reaper's predicate, not about this interrupt, and
it is left to the Issue Pool rather than changed here.

### 4. The fan-out is bounded; the drain is never blocked

The latch waits only a short window for turns to pick their request up — 2 s
for the machine latch, 250 ms for the Supervisor-lease latch, which can run on
a member's own supervisor thread. The wait exists so the latch can journal what
actually happened, not to gate anything:

- a turn owned by the calling thread is never waited on, because an authority
  latch is reachable from inside a turn's own supervisor thread;
- the request stays on that turn's registry entry after the window closes, so a
  turn that polls later still receives its interrupt;
- a turn that already took its interrupt is neither waited on again nor
  re-reported: a later latch reads the first result off the entry;
- a provider that never polls is reported `not_observed`. On the machine path
  the SIGKILL backstop still ends it; on the Supervisor-lease path nothing
  else does, which is why the registration ordering in §1 matters.

`graceful_shutdown_with_deadlines` is unchanged: the same cooperative wait, the
same process-group termination, the same forced join. The interrupt is strictly
additive in front of it.

### 5. The machine latch journals the fan-out

The machine latch adds one self-stop phase,
`cooperative_interrupt_dispatched`, carrying an additive `detail` object with
`scope`, `reason`, `turns_live`, `turns_interrupted`, and one entry per turn
(`provider`, `team_run_id`, `member_run_id`, `outcome`). Phases that carry no
extra evidence keep exactly the summary shape they had.

The Supervisor-lease latch has no self-stop event — it is not a daemon
self-stop — and reports its fan-out to stderr, which is the detached daemon's
durable log.

## Consequences

- Hard Invariant 7 becomes true and specific, and scoped to the latch that owns
  each step: on the machine path a live turn gets one cooperative interrupt,
  then the drain's bounded cooperative wait, then SIGKILL; on the
  Supervisor-lease path there is no drain and no SIGKILL, so that interrupt is
  the only thing that ends the turn.
- There is one process-local provider action that no durable command
  authorizes. It is permitted only because the alternative — admitting a
  command under lost authority — is worse, and it is confined to the interrupt
  primitive each adapter already exposes.
- A turn interrupted this way is still unproven work. Nothing about the
  interrupt implies the provider finished, answered, or succeeded, and nothing
  about it settles the admitted effect.
- The registry is process-global, so every test that registers a live turn or
  fans an interrupt out holds one shared guard
  (`crates/firm-cli/src/main_tests/live_turn_serialization.rs`). CI already runs
  `--test-threads=1`; a plain local `cargo test` does not.
- Whether an explicit `RecoveryRequired` row should also hold adoption is now a
  more frequent question (§3). It is a question about the reaper's predicate,
  and it is left open rather than answered here.

## Evidence

- `crates/firm-runtime-host/src/authority_loss_interrupt/tests.rs` — one
  interrupt per turn, no republication, scope isolation, the bounded wait, the
  self-wait guard, the idempotent second-latch report, that a turn registered
  before the fan-out is always reached, and that a latched scope is *not*
  remembered for turns registered afterwards.
- `crates/firm-cli/src/main_tests/general/acceptance_wake.rs` —
  `a_parked_turn_is_registered_before_the_occupied_slot_wait` drives the real
  `run_team_member_with_adapter` loop with the turn slot held, fans out while
  the member is parked, and asserts `turns_live == 1` for that member plus the
  interrupt crossing the provider bridge. Moving the registration back below
  the wait reports 0 — the ordering half of §1 has real signal.
- `crates/firm-cli/src/daemon_integration_tests/self_stop_events_tests.rs` —
  `machine_authority_loss_quiesces_every_served_supervisor_scope` and
  `a_turn_admitted_before_the_machine_latch_cannot_drive_after_it` pin the
  quiesce half: with the durable Supervisor row left Active, a turn registered
  after the machine fan-out refuses at `acquire_prepared_cycle_turn` and never
  drives. Both go red without the quiesce.
- `crates/firm-cli/src/daemon_integration_tests/self_stop_events_tests.rs` — a
  live turn is interrupted on machine authority loss and named in the
  journalled evidence with outcome `dispatched`.
- `crates/firm-cli/tests/team_run_api/authority_loss_cooperatively_interrupts_a_live_provider_turn.rs`
  — with the ACP shim holding a prompt open, losing the Supervisor lease puts
  exactly one `session/cancel` on the Kimi wire while the turn is live, settles
  nothing, and writes no RuntimeCommand.
