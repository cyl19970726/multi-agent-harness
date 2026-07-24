# 0021: Resident-daemon warm-child host

## Status

Accepted for resident process reuse. Its former mirrored-session/NDJSON storage
claims are superseded by ADR 0032.

(0020 is the latest prior ADR; 0021 is the next free number.)

## Context

ADR 0018 made **headless exec-stream** the primary provider substrate and
retired the bespoke `codex app-server` WebSocket-over-UDS *provider protocol* as
the default. It explicitly allowed, in the Decision section, "one run per claimed
delivery (optionally a small warm pool if latency matters)."

The resident Claude path ([../resident-claude.md](../resident-claude.md),
`crates/harness-cli/src/resident.rs`) holds a `claude` child open across turns to
amortize model + MCP warmup. That `ResidentPool` lives **in-process**, but the
harness CLI is short-lived: every `harness agent deliver` is a fresh process, so
an in-process pool dies with the command and never actually keeps a child warm
across deliveries. The warm pool 0018 anticipated needs a cross-process host.

## Decision

Add an **internal, unix-only daemon** (`crates/harness-cli/src/resident_daemon.rs`,
`harness daemon start|status|stop`) that hosts one `Arc<Mutex<ResidentPool>>`
behind a per-workspace Unix domain socket (`<store-root>/resident.sock`). Each
short-lived delivery connects, sends one line-delimited JSON `DaemonRequest`
(exactly the `ResidentPool::run_turn` arguments), and reads one `DaemonResponse`.
The daemon serializes turns under the pool lock — the same one-turn-at-a-time
discipline a single stream-json child already requires — and reaps idle children
opportunistically after each turn.

**This is the "small warm pool" 0018 allows, not a return to a bespoke provider
protocol:**

1. The children are STILL `claude -p --input-format stream-json
   --output-format stream-json --verbose` — the exact documented headless
   contract 0018 blessed. The daemon never speaks JSON-RPC, never frames a
   provider socket, never invents a turn state machine; it writes one
   user-message stream-json frame to stdin and reads to the `result` frame
   (`resident.rs` already does this).
2. The Unix socket is **internal harness IPC** between the short-lived
   `harness deliver` CLI and a long-lived harness-owned host — it is NOT a
   provider transport. 0018's objection was to a *provider* protocol that was
   undocumented/Tier-3; this socket carries `run_turn` arguments between two
   pieces of OUR own code, both of which still drive the provider via the
   documented exec-stream contract.
3. It does not resurrect the app-server fallback path 0018 retains for mid-turn
   approval; resident children have the same no-mid-turn-interrupt capability as
   exec, so the honest-capability consequence of 0018 is preserved.

## Consequences

- **Opt-in and degrading:** the resident path is still gated by
  `HARNESS_CLAUDE_RESIDENT=1` (0018-era behavior). Within it, the daemon is used
  ONLY when a probe-connect to the socket succeeds; otherwise delivery falls
  through to the existing inline single-turn resident path. Flag unset → exec
  path → daemon code never reached. Default behavior is byte-for-byte unchanged.
- **No new crates; unix-only.** Synchronous `std::*` throughout (thread-per-
  connection, like `serve_command`). Signal handling uses a minimal `signal(2)`
  FFI to set an `AtomicBool`; a hard `SIGKILL` cannot run it, but daemon death
  closes the children's stdin pipes and `claude` exits on EOF (the load-bearing
  safety net).
- **Execution seam:** resident and one-shot modes return the same in-memory
  outcome and Claude native session id; neither mode retains a Harness copy of
  the provider stream.
- **Conservative concurrency:** the global pool `Mutex` serializes turns across
  all members in v1 (delivery is already serial per agent). Per-member lock
  sharding is future work.
- **Failure mode:** a connect that succeeds but whose round-trip fails mid-turn
  is reported as a failed delivery (no silent inline retry), because the turn may
  have partially run against the warm child.

## Validation

```bash
cargo test -p harness-cli
```

The core proof is the integration test
`resident_daemon::tests::daemon_keeps_child_warm_across_two_connections`: two
SEPARATE socket connections for the same member are served by ONE child (the fake
records one PID line) with continuous `session_id`.
