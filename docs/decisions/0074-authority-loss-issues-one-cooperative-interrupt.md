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

What followed was `graceful_shutdown_with_deadlines`
(`crates/firm-node-daemon/src/supervisor_daemon/shutdown.rs`): revoke each
run's heartbeat, wait up to `SUPERVISOR_DRAIN_TIMEOUT` (30 s) for a
*cooperative* exit, then SIGKILL every registered provider process group
(`crates/firm-runtime-host/src/lib.rs`), then a 5 s forced join.

The gap is between those two sentences. A member in the middle of a turn
cannot exit cooperatively: the supervisor thread is inside
`TeamRuntimeAdapter::run_cycle`, waiting on the provider's terminal frame, and
nothing between cycle start and the terminal consults authority. So the
30-second "cooperative" window is, for exactly the case it exists to handle,
a 30-second window in which a provider keeps spending tokens and running tools
on behalf of an authority that is already gone — and is then killed mid-tool
with no chance to stop cleanly.

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
this process is driving. The supervisor registers one entry for the exact
duration of a driven cycle, next to the turn lease
(`crates/firm-cli/src/runtime_adapter.rs`); dropping that guard when the cycle
returns is what keeps a finished turn out of any fan-out.

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

### 3. The interrupted terminal still hits the authority refusal

Interrupting the turn does not admit its terminal. When the cycle returns, the
terminal stage still calls `ledger.require_supervisor_lease()` and
`require_provider_session_authority` before any settlement, delivery mutation,
member action, or session transition, and under lost authority it refuses.

An interrupted turn therefore settles nothing: its admitted `StartCycle` keeps
`Unknown` certainty and `Unknown` postcondition and becomes explicit recovery
work, exactly as an abandoned turn did before. The observable difference is
that the turn now *reaches* its prepared-effect scope guard instead of being
killed inside the provider wait, so the unproven command is recorded as
explicit recovery work rather than left as an abandoned `Prepared` row. That is
more auditable, and it is the existing guard's behavior, not a new settlement.

### 4. The fan-out is bounded; the drain is never blocked

The latch waits only a short window for turns to pick their request up — 2 s
for the machine latch, 250 ms for the Supervisor-lease latch, which can run on
a member's own supervisor thread. The wait exists so the latch can journal what
actually happened, not to gate anything:

- a turn owned by the calling thread is never waited on, because an authority
  latch is reachable from inside a turn's own supervisor thread;
- the request stays latched after the window closes, so a turn that polls later
  still receives its interrupt;
- a provider that never polls is reported `not_observed` and left to the
  SIGKILL backstop.

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

- Hard Invariant 7 becomes true and specific: on authority loss a live turn
  gets one cooperative interrupt, then the bounded cooperative wait, then
  SIGKILL.
- There is one process-local provider action that no durable command
  authorizes. It is permitted only because the alternative — admitting a
  command under lost authority — is worse, and it is confined to the interrupt
  primitive each adapter already exposes.
- A turn interrupted this way is still unproven work. Nothing about the
  interrupt implies the provider finished, answered, or succeeded, and nothing
  about it settles the admitted effect.
- The registry is process-global, so tests that use the `Process` scope must be
  serialized against other live turns in the same test binary.

## Evidence

- `crates/firm-runtime-host/src/authority_loss_interrupt/tests.rs` — one
  interrupt per turn, no republication, scope isolation, the bounded wait, and
  the self-wait guard.
- `crates/firm-cli/src/daemon_integration_tests/self_stop_events_tests.rs` — a
  live turn is interrupted on machine authority loss and named in the
  journalled evidence with outcome `dispatched`.
- `crates/firm-cli/tests/team_run_api/authority_loss_cooperatively_interrupts_a_live_provider_turn.rs`
  — with the ACP shim holding a prompt open, losing the Supervisor lease puts
  exactly one `session/cancel` on the Kimi wire while the turn is live, settles
  nothing, and writes no RuntimeCommand.
