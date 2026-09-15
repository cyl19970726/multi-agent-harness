# ADR 0073: Automatic predecessor recovery when death is proven; every authority-loss path leaves a settlement record

```text
status: accepted; implemented by the 2026-09-15 CORE5-E1a slice
date: 2026-09-15
amends: ADR 0065 two runtime epochs (unchanged); the NodeDaemonLease acquisition fence in store_node_runtime.rs
canonical_for: when a successor NodeDaemon may settle a predecessor generation without a human; what a generation writes about the lanes it could not settle
```

## Context

A machine-scoped `NodeDaemonLease` may be acquired only over an explicitly
`Released` predecessor. When the predecessor crashed, nothing releases it, so
acquisition refuses with `NODE_DAEMON_PREDECESSOR_RECOVERY_REQUIRED` and the
machine stops serving Teams until a human runs:

```text
firm daemon recover-predecessor --confirm daemon-recover-predecessor
```

That fence is right. The failure is that it is the *only* exit. In the live
dogfood store the two real authority losses waited **1 h 53 m** and **2.5 days**
for that human, and in both cases every proof the CLI demands was already
satisfiable the whole time: the process was gone, the lease was long past
expiry, there was exactly one predecessor instance, and no RuntimeCommand was
ambiguous. The machine was idle for days waiting for someone to retype
evidence it could read itself.

The second hole is quieter. A generation that loses authority writes *nothing*
about the lanes it was running:

| Path | Code | What it wrote |
| --- | --- | --- |
| The Space's latest lease already moved | `machine_authority.rs` release / settle / drain skips | nothing |
| `graceful_shutdown` fails `NODE_DAEMON_DRAIN_INCOMPLETE` | `supervisor_daemon.rs` skips the settle step | nothing, not even the self-stop phases |

Both skips are correct on their own terms — none of those writers may touch a
lease this generation no longer holds, and a drain that did not converge has no
proof its provider process groups are terminal. But the result is that a lane
which was mid-turn when its owner went dark is indistinguishable from a lane
that was always idle. `settle_node_daemon_shutdown_sessions` is deliberately
fail-closed (it demands process-group termination proof, all Supervisor leases
Released, and no unsettled RuntimeCommands), so the honest options were
"settle with proof" or "write nothing". There was no way to say *this lane was
not settled, and here is who failed to settle it*.

## Decision

### 1. The successor recovers a predecessor whose death is proven

In the successor's first scan (`ensure_node_authority_bundle`), when the latest
lease is unreleased and belongs to another daemon or instance, the daemon runs
the same proofs `recover-predecessor` runs, and — only if all of them hold —
performs the same recovery: settle the predecessor's sessions, release its
lease, acquire at `generation + 1`.

| Proof | Refusal when it fails |
| --- | --- |
| Exactly one unreleased predecessor instance across every registered Execution Space | `SUPERVISOR_GENERATION_FENCED` |
| The unreleased lease is not this exact instance's own | `NODE_DAEMON_PREDECESSOR_SETTLEMENT_REQUIRED` |
| Every selected lease is past its expiry by time | `NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE: lease_not_expired` |
| The predecessor process is absent, or provably a recycled pid | `NODE_DAEMON_PREDECESSOR_RECOVERY_LIVE: process_alive` |
| The probe itself is readable | `NODE_DAEMON_PREDECESSOR_RECOVERY_UNVERIFIED` |
| No ambiguous RuntimeCommand of that generation (Store-enforced) | `NODE_DAEMON_PREDECESSOR_RECOVERY_COMMAND_UNSETTLED` |
| Every registered Store is readable | `NODE_DAEMON_PREDECESSOR_RECOVERY_INCOMPLETE` |

What it still refuses is the point of the change. A daemon that is *alive but
starved* — the case that produced these losses — has a live pid, so it refuses
and stays human-gated. Ambiguous RuntimeCommands stay human-gated. Two
unreleased instances stay human-gated, because a machine in that state is one a
human should look at.

The one proof the successor adds, which a human at a terminal usually cannot
make, is **recycled-pid detection**. `kill(pid, 0)` answers "does this pid
exist", not "is this the predecessor". Pids are recycled, so a stranger holding
the predecessor's number would keep the machine fenced forever. The successor
reads the live process' start time (`ps -o etime=`, one second resolution,
locale-independent, portable across BSD and procps) and compares a *lower
bound* of it — `now - (etime + 1s)`, with `now` read before the probe — against
the last moment the predecessor was known alive: the maximum of its lease's
`renewed_unix_ms` and the unix-ms stamp inside its own instance id. A process
that started more than `REUSED_PID_START_TOLERANCE_MS` (2 s, covering `ps`
granularity and clock jitter) after that anchor cannot be the predecessor, and
counts as absent. Every unmeasurable case — no `ps`, an unparsable row, a start
time inside the tolerance — fails closed as alive.

Refusals are not silent. Each attempt records a `node_daemon_predecessor_
recovery` row in the heartbeat diagnostics, so `daemon status` shows under
`lease_renewals` exactly which proof refused and, for an unexpired lease, when
recovery becomes possible.

A successful automatic recovery journals a
`node_daemon / predecessor_recovered_automatically` TeamRunEvent on every
TeamRun whose Supervisor lease it released, carrying the whole receipt: the
process-death proof (pid, reason, anchor, start-time lower bound, raw evidence
row), the predecessor expiry, the per-Space settlements, and the recovering
instance. The event *is* the evidence rather than a summary of it. When the
dead generation was supervising no TeamRun there is no per-run journal to
write, and the detached daemon log is the record.

**Shared code, not a second copy.** The `daemon recover-predecessor` CLI, the
Operator HTTP role action, and this scan now answer through the same three
owners:

| Concern | Owner |
| --- | --- |
| instance-id parse, existence probe, start-time proof | `firm-runtime-host::predecessor_process` |
| which instance is the predecessor | `firm-store::select_exact_predecessor_spaces` |
| the per-Space transition and the receipt | `firm-store::recover_predecessor_generation_across_spaces` |

The daemon crate cannot depend on the CLI, and its package-boundary gate pins
its dependency set, so the process proof lives in `firm-runtime-host` — the
crate that already owns process-group liveness facts and `libc` — and the
Store-facing halves live beside `recover_node_daemon_predecessor`. The CLI
keeps only what is genuinely its own: the live-socket check and its error
envelope.

### 2. Every authority-loss path leaves a settlement record

Both skipped paths now write two things they can honestly write.

**The self-stop phases.** `journal_machine_authority_loss_phase` only fires for
a captured self-stop, so an unconverged drain used to journal nothing at all.
A drain-incomplete stop now captures one — under its own reason,
`NODE_DAEMON_DRAIN_INCOMPLETE`, never borrowing
`NODE_DAEMON_MACHINE_AUTHORITY_LOST`, because an unconverged drain is not a
lost lease.

**A per-session `settlement_incomplete` marker.** A new optional field on
`AgentSessionControlState` names the exact generation that went dark
(`node_id`, `node_daemon_id`, `node_daemon_generation`, `instance_id`), why
(`reason`), and when it was observed. `record_node_daemon_settlement_incomplete`
is its only writer, and it touches no lifecycle, residency, activity or cycle
field: a generation with no termination proof must not write `Interrupted`,
which would be an operator-asserted flag wearing evidence's clothes. It skips
lanes already at rest, because marking a settled lane is a false record, and it
treats an identical observation as a replay — `observed_at` is excluded from
that comparison, since it records when the daemon looked, not what it saw. Its
fence is the exact daemon Service actor plus the exact
node/daemon/generation/instance the marked Sessions already carry, deliberately
*not* a lease, because the whole case is a generation whose lease is gone.

#### Why the session, and not a MemberAction or a TeamRunEvent

The brief for this slice offered three homes and one rule: prefer whichever
recovery already reads. Recovery reads `fabric_agent_sessions` filtered by
node, daemon id and generation, `team_supervisor_leases`, `runtime_commands`
and the lease. It reads neither `member_actions` nor `team_run_events`, so
either of those would have needed a new reader — a second place to look for
settlement truth, next to the one that already exists. A MemberAction also
needs a `team_run_id` and `member_run_id` the Store-level settle path does not
have in hand, and it is defined as a member's own journaled action, which this
is not: it is a fact about a machine generation.

The control-state field is read by recovery for free, travels with the lane
across every reader that already loads the session, and — decisively — lets the
release fence use it. `require_node_daemon_settlement_unlocked` now treats a
lane still carrying a marker of that exact generation as unsettled, so no lease
can be released past a lane nobody proved terminal. By construction a marked
lane is also non-detached (the writer skips lanes at rest), so that clause is
defence in depth rather than the primary fence — kept for the same reason
`NativeContinuationActivation::Armed` is kept with no writer: a fail-closed
reader costs nothing and catches the writer nobody has written yet.

Recovery is the party that resolves the marker. It is the first with a
process-group termination proof, so a flagged lane is *not* treated as settled
however idle it looks: recovery settles it, clears the flag, and reports it in
`sessions_settlement_incomplete` on the receipt, so an operator reads which
lanes were known-unsettled rather than inferring it from silence. The shutdown
drain clears the flag the same way when it does converge.

## Consequences

- A machine whose daemon died cleanly recovers itself on the next scan instead
  of waiting hours or days for a human. A machine whose daemon is alive,
  ambiguous, or duplicated still waits for that human, and now says why.
- `AgentSessionControlState` gains one optional field. Pre-cutover rows decode
  unchanged (`#[serde(default)]` under `deny_unknown_fields`), and the schema
  plus fixtures carry the new shape.
- The predecessor-recovery receipt gains `sessions_settlement_incomplete` and
  `process_death_proof`. Existing consumers reading named keys are unaffected;
  one exact-equality test assertion was extended.
- The automatic path deliberately does **not** widen what recovery may settle:
  it recovers exactly the latest predecessor instance, exactly as the CLI does.
  A marker naming an older generation is evidence for a human, not an
  authorization to sweep it.
- `ps` is now on the recovery path, but only after the expiry proof passes, so
  it never runs for a predecessor that is still renewing its lease. Its absence
  is an unverified probe, which fails closed to "alive"; no recovery decision
  depends on `ps` succeeding.
- Writing the markers is Store IO on a path that is already failing, and lock
  starvation is one of the ways authority is lost in the first place. It is
  bounded by the ordinary 10 s write-lock budget per Execution Space, and a
  timeout falls back to the detached daemon log rather than delaying the stop
  further.
