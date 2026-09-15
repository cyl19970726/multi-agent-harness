# NodeDaemon Runtime

```text
status: canonical runtime contract
owner: lead-operations
last reviewed: 2026-08-09
canonical_for: one-machine NodeDaemon, Team placement, child supervision, and recovery
```

## Model

One logical Firm may span machines. Each machine has exactly one stable
`ExecutionNode` identity and one `NodeDaemon`. An `AgentTeam` is placed on one
Node and its Members never execute across machines.

```text
Firm
├── Node A → one NodeDaemon → Team A runs, Team B runs
└── Node B → one NodeDaemon → Team C runs
```

There is no public per-TeamRun daemon and no fallback that silently starts one.
`firm team-run start` and HTTP validate the exact Team, Node,
Execution Space, and project binding, then delegate to the NodeDaemon. An
unreachable daemon is an explicit `NODE_DAEMON_UNAVAILABLE` failure.

## Durable authority

- `ExecutionNode` is the stable machine identity.
- `NodeProjectRegistration` admits an Execution Space/project binding to a Node.
- `NodeDaemonLease` gives one daemon generation machine-scoped authority over
  all local Teams in every Execution Space registered to that Node. It is not
  an Execution-Space-scoped lease.
- `TeamSupervisorLease` is a child lease fenced by Node id, daemon id, daemon
  generation, Execution Space, and project binding.
- A stale child cannot heartbeat or write after its parent generation changes.

`NodeDaemonLease` is machine-scoped authority for all local Teams across
registered Execution Spaces; each machine has one machine-scoped NodeDaemon and
the lease is never scoped to one Execution Space.

Machine scope is a property of the daemon's *bundle*, not of any single stored
row. Each registered Execution Space keeps its own `node_daemon_leases.jsonl`
row for this Node, and the `generation` counter inside it is a Space-local
counter — never a machine-wide ordering
(`crates/firm-cli/src/main_modules/daemon_predecessor_recovery.rs:75`). Before
any Team may admit a provider effect, the daemon acquires and revalidates the
complete set of per-Space leases for every registered Space owned by this Node
and treats them as one all-or-nothing authority; a partial first acquisition
rolls back only the leases this instance acquired
(`crates/firm-node-daemon/src/supervisor_daemon/machine_authority.rs:157-251`).
Read the two together: the rows are per Space, the authority they reconstruct
is per machine.

Because that bundle is all-or-nothing, an authority failure in one Space is
never isolated to that Space. It closes the shared process admission gate,
latches `NODE_DAEMON_MACHINE_AUTHORITY_LOST` for this daemon instance, and
starts a machine-wide drain
(`crates/firm-node-daemon/src/supervisor_daemon/machine_authority.rs:52-72`,
`:200-215`).

The daemon scans every registered Execution Space independently. Store-read
isolation applies to *discovery* only: a Space whose TeamRun listing cannot be
read is reported and skipped for that pass, and supervision in another Space
continues
(`crates/firm-node-daemon/src/supervisor_daemon/team_supervision.rs:40-48`).
That isolation never extends to the authority bundle above, so "reported and
isolated" is a statement about discovery reads, not about losing authority.
On restart the new daemon generation recovers eligible non-terminal
TeamRuns without duplicating provider delivery.

## Timing constants

Every value below is the shipped default in this checkout. They are stated
here because an operator reading only prose cannot tell which bounds move
together.

| Bound | Value | Source |
| --- | --- | --- |
| Execution Space scan interval | 5 s (`--scan-interval-secs`) | `crates/firm-cli/src/main_modules/daemon_cli.rs:286-293` |
| NodeDaemon lease TTL | `max(scan × 4, 15 s)` = 20 s at the default scan | `crates/firm-node-daemon/src/supervisor_daemon/machine_authority.rs:273-285` |
| NodeDaemon lease renewal cadence | `min(remaining / 4, clamp(scan, 1 s, 5 s))` = 5 s at the default scan | `crates/firm-node-daemon/src/supervisor_daemon/machine_authority.rs:35-38`, `:410-420` |
| Renewal lock-wait budget | the full remaining TTL, taken as one cancellable FIFO ticket | `crates/firm-store/src/store_node_runtime.rs:375-386` |
| Team Supervisor lease TTL | 15 s (`FIRM_TEAM_SUPERVISOR_LEASE_MS`, then `HARNESS_TEAM_SUPERVISOR_LEASE_MS`) | `crates/firm-cli/src/main_modules/supervisor_control.rs:133-140` |
| Supervisor heartbeat interval | `(ttl / 3).clamp(50 ms, 1 s)` = 1 s at the default TTL | `crates/firm-cli/src/main_modules/runtime_effects.rs:1021` |
| Supervisor heartbeat retry after a failure | `min(interval, 100 ms)` | `crates/firm-cli/src/main_modules/supervisor_control.rs:45-52` |
| NodeDaemon drain TTL extension | 60 s | `crates/firm-node-daemon/src/supervisor_daemon/machine_authority.rs:752` |
| `daemon stop` upper drain bound | 20 s control + 20 s scanner + 30 s supervisors + 5 s forced = 75 s | `crates/firm-node-daemon/src/supervisor_daemon.rs:105-121` |
| `daemon start` readiness wait | 60 s | `crates/firm-cli/src/main_modules/daemon_cli.rs:358` |
| Member drive tick | 50 ms | `crates/firm-cli/src/main_modules/member_admission_drive.rs:408` |
| Provider input-acceptance boundary | 300 s (`--idle-timeout-secs`) | `crates/firm-cli/src/main_modules/daemon_cli.rs:276-285` |
| Host binding lease TTL | 30 s default, 5–300 s accepted, renewed only by an explicit CLI call | `crates/firm-cli/src/main_modules/host_binding.rs:3-5`, `crates/firm-cli/src/main_modules/team_run_cli.rs:701-710` |

Raising `--scan-interval-secs` lengthens the NodeDaemon lease TTL and the
renewal cadence together: the TTL is derived from the scan interval
(`max(scan × 4, 15 s)`) and the renewal delay is capped by
`clamp(scan, 1 s, 5 s)`. A longer scan therefore buys a longer grace period
for a slow store, and it also delays how quickly a lost lease is observed.
Tune it deliberately, not as a throughput knob. The Supervisor lease TTL is
independent of the scan interval and moves only through its environment
variables.

## Control protocol

The machine socket is `<FIRM_HOME>/nodes/<node_id>/daemon.sock`, with a hashed
`/tmp` fallback for Unix path-length limits. Requests are bounded newline JSON:

```json
{"cmd":"start","execution_space_id":"space-a","run_id":"team-run-1"}
{"cmd":"status"}
{"cmd":"stop","execution_space_id":"space-a","daemon_generation":12}
```

Repeated start is idempotent and returns `reused: true`. A request naming a
Team placed on another Node, an unregistered project, or a mismatched Execution
Space is rejected before child supervision starts.

Stop is generation-fenced and two-phase. The daemon first stops accepting new
control, keeps its lease alive while accepted mutations and Team Supervisors
drain, and drops standalone session handles. If a Supervisor misses the bounded
cooperative deadline, the daemon terminates only provider process groups
registered by that exact daemon process and waits for their threads to observe
EOF. Each registration has a process-local unique token, so an old guard cannot
remove a later registration that reused the same pid. The first forced drain
atomically closes new provider-group admission; a late spawn is terminated
synchronously and included in the final drain before admission can reopen. It
also linearizes child reap and exact-token unregister under that admission
mutex, so shutdown cannot signal a pid after its original child became
reusable. Provider-side kill/reap holds that mutex only for a bounded interval;
each non-blocking reap observation and terminal-token removal uses one short
critical section, while every poll wait happens outside the mutex. On timeout
it keeps the exact pid/token registration. Shutdown signalling likewise never
removes that ownership evidence: only the provider guard's terminal
`try_wait` observation may remove it. A Supervisor that exits after SIGKILL but
without terminal reap therefore leaves a typed completion residual, and the
daemon reports `NODE_DAEMON_DRAIN_INCOMPLETE` instead of releasing authority.
If process-group signalling fails, an immediate-child terminal reap still
removes its exact pid/token to prevent a later drain signalling a reused PID;
the independent signal-failure diagnostic keeps completion closed.
Registration attempted after admission closes
returns a typed `PROVIDER_PROCESS_GROUP_ADMISSION_CLOSED` failure only after the
shared registration path has signalled and bounded-reaped the actual child
outside the registry mutex; a provider cannot continue assembling that
transport as an ordinary spawn. The typed failure survives every provider
adapter into the Supervisor lifecycle boundary: it is StopNoRetry and preserves
the current Member lifecycle because NodeDaemon shutdown superseded the spawn.
Signal failures, reap failures, and reap timeouts remain typed residual
diagnostics. Admission reopens only through a
checked completion result; any residual registration or diagnostic keeps it
closed. It
releases authority only after this converges; an unkillable group or unfinished
thread is `NODE_DAEMON_DRAIN_INCOMPLETE`, never a successful stop.

## Operator surface

```bash
firm node init
firm node project register --project-binding-id <id>
firm daemon start
firm daemon status
firm team-run start --id <run-id>
firm daemon stop
```

`daemon serve` exposes concurrency, input-acceptance (`--idle-timeout-secs`),
and scan-interval tuning for foreground operation. Tests that claim runtime
behavior must start a real NodeDaemon and cover at least two isolated
Execution Spaces.
