# Multi-Team Supervisor Daemon

## Design

### Mental Model

```
BEFORE (per-team foreground supervisor):
  $ harness team-run start --id run-A    # process blocks, drives A
  $ harness team-run start --id run-B    # separate process, drives B

AFTER (multi-team daemon):
  $ harness daemon serve                 # one long-lived process
  $ harness team-run start --id run-A    # sends control message to daemon
  $ harness team-run start --id run-B    # sends control message to daemon
```

The daemon watches the store for active team-runs, runs one supervisor
context per run on dedicated threads, and exposes a Unix-domain socket
for control (start, status, stop).  A single daemon heartbeat confirms
it is alive; per-run supervisor leases maintain execution authority as
they do today.

### Daemon Loop

```
serve_loop:
  for each active team-run:
    if no managed context:
      prepare_team_run_start() -> spawn drive_prepared_team_run() on thread
    if managed context finished:
      reap thread, record outcome

  poll control socket:
    "start <run-id>"  -> mark ad-hoc start (next scan picks it up)
    "status"          -> report managed runs
    "stop"            -> graceful shutdown

  sleep scan_interval
  repeat
```

### Per-Team Supervisor Context

Each managed team-run owns a `SupervisorContext`:

```rust
struct SupervisorContext {
    run_id: String,
    registration: TeamSupervisorRegistration,  // lease + heartbeat
    thread: JoinHandle<CliResult<()>>,
    started_at: Instant,
}
```

The daemon spawns one `std::thread` per context.  The thread calls
the existing `prepare_team_run_start()` + `drive_prepared_team_run()`
(the same path CLI `team-run start` uses today).  When
`drive_prepared_team_run()` returns, the context is reaped.

### Shared Heartbeat

The daemon writes a single `daemon-heartbeat.json` file (or a
well-known pidfile + socket) that external monitoring can check.
Per-run supervisor leases are separate — they continue exactly as
today via `TeamSupervisorRegistration`.  The daemon's liveness is
provable through:

- The control socket accepting connections
- The process pidfile
- `Store::list_recent_team_supervisor_leases()` showing active
  renewals for all managed runs

Crash recovery: on restart, the daemon enumerates non-terminal
team-runs, inspects supervisor leases, and re-attaches to runs whose
last supervisor is stale (expired lease).

### Control Socket Protocol

Line-delimited JSON over a Unix-domain socket at `<store-root>/supervisor.sock`:

```
→ {"cmd":"start","run_id":"team-run-..."}
← {"ok":true,"run_id":"team-run-..."}

→ {"cmd":"status"}
← {"ok":true,"runs":[{"run_id":"...","status":"running","members":3},...]}

→ {"cmd":"stop"}
← {"ok":true}
```

Unauthenticated (local IPC only).  The supervisor socket is separate
from the resident daemon socket (`resident.sock`) — they belong to
different subsystems.

### CLI Integration

`harness team-run start --id <run-id>` changes behavior:

1. If `supervisor.sock` is reachable, send `{"cmd":"start",...}` and
   print the response.  The CLI returns immediately (no foreground
   blocking).
2. If the daemon is absent, fall back to the existing foreground path
   (compatibility).

`harness daemon serve` is the new command that runs the multi-team
daemon.  The existing `harness daemon start|status|stop` commands
(which manage the resident claude pool) are renamed to `harness daemon
resident-start|resident-status|resident-stop` to avoid confusion, with
legacy aliases preserved.

### Crash Recovery

```
on_startup:
  runs = store.list_non_terminal_team_runs()
  for each run:
    supervisor = store.latest_supervisor_lease(run.id)
    if supervisor.is_expired():
      // No live supervisor; daemon takes over.
      spawn_context(run.id)
    else:
      // Active supervisor elsewhere; skip.
      log "run {run.id} already supervised by {supervisor.id}"
```

When the daemon itself crashes, the OS closes the socket and the
supervisor leases expire naturally (TTL-based).  A restarted daemon
picks up orphaned runs.

### Concurrency Model

- One daemon **process** (not one per team-run)
- One **thread** per managed team-run (via `std::thread::spawn`)
- Threads are independent — they share no mutable state beyond the
  store (which is append-only/compare-and-swap safe)
- A `Mutex<HashMap<String, SupervisorContext>>` guards the context
  registry for the control-socket handler
- The scan loop and control handler are on the main thread

### Graceful Shutdown

SIGTERM/SIGINT:

1. Stop accepting new start requests (close control listener)
2. Signal each managed context to drain: interrupt active member turns,
   let in-flight turns finish naturally
3. Wait for all context threads to join (with a deadline)
4. Remove socket and pidfile
5. Exit

### No-Regression Guarantee

The foreground `team-run start` path is preserved unchanged as a
fallback.  All existing integration tests in `tests/team_run_start.rs`
and `tests/team_run_api.rs` continue to exercise the foreground path
(because no daemon socket exists in test environments).  The daemon
path is exercised through new focused tests.

## Implementation Plan

### New Files

- `crates/harness-cli/src/supervisor_daemon.rs` — multi-team daemon

### Modified Files

- `crates/harness-cli/src/main.rs`:
  - Add `mod supervisor_daemon;`
  - Route `harness daemon serve` to `supervisor_daemon::run_serve()`
  - Change `harness team-run start` to prefer daemon when socket present
  - Rename resident daemon commands (aliased for back-compat)
  - Expose `prepare_team_run_start` and `drive_prepared_team_run` as
    `pub(crate)` (already done)

### Tests

- `crates/harness-cli/tests/team_run_daemon.rs` — integration tests
  covering:
  - Daemon spawn/adopt lifecycle (start daemon, start team-run via
    socket, verify member execution)
  - Crash-respawn (kill daemon, restart, verify reattachment)
  - Multiple concurrent team-runs in one daemon
  - Existing team-run start tests remain green (foreground fallback path)
