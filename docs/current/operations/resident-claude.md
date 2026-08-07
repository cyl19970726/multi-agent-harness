# Resident Claude process (opt-in)

The default Claude delivery path spawns a fresh `claude -p <prompt> …` per turn
and relies on the process **exiting** to terminate its stdout stream. That is
simple and correct, but it re-pays model + MCP warmup on every turn.

The resident path holds one `claude` process **open across turns** and feeds each
turn as a single JSON frame on stdin. It is **opt-in** and **additive**: when the
flag is unset, nothing changes.

## The keep-alive principle: hold stdin = no EOF

A one-shot `claude -p` exits when its single turn finishes. A resident must not
exit between turns. The switch is the input contract:

- default: `claude -p <prompt> --output-format stream-json --verbose`, stdin
  closed (`Stdio::null`) so claude does not block on stdin.
- resident: `claude -p --input-format stream-json --output-format stream-json
  --verbose`, stdin held open (`Stdio::piped`, the `ChildStdin` kept alive).

Because we keep the child's stdin open, the child **never sees EOF**, so it stays
resident waiting for the next frame. Each turn is one user-message frame written
to stdin and flushed:

```json
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"…"}]}}
```

**Closing stdin is the explicit shutdown signal** — dropping the `ChildStdin`
sends EOF and the process exits cleanly.

### Why we must not drain the pipes to EOF

A resident never reaches stdout/stderr EOF between turns. The default path reads
stdout to EOF and then `read_to_string`s stderr to EOF — both would block forever
against a live resident, and a full stderr pipe blocking against a blocked stdout
read is a classic two-pipe deadlock. The resident path avoids this two ways:

1. **stderr is redirected to a file** (`Stdio::from(File::create(…/claude.stderr))`)
   so the OS absorbs it — no buffer backpressure, no EOF dependency. This is the
   single highest-risk item and the reason a resident does not deadlock.
2. **stdout is read incrementally and stopped at the per-turn `result` event**,
   leaving the buffered reader positioned for the next turn (we never loop to
   EOF).

## Hot / cold hybrid

The resident is a **hot path** with **cold-recovery** fallbacks:

- **Hot:** a pooled child serves every queued message for a member at near-zero
  per-turn startup cost.
- **Cold recovery (crash):** if the held child has died, the pool respawns it
  with `--resume <session_id>` — the real session id parsed from the first
  `system` frame — so conversation memory carries across the crash. The session
  id is the source of truth for resume (it is also written back to
  `member.provider_thread_id`, exactly as the default path does).
- **Idle reclaim:** a pooled child idle beyond a max-idle duration is dropped
  (stdin closed → clean EOF shutdown) and respawned on the next turn, so resident
  processes do not linger indefinitely.

Config drift safety: the pool keys children by `(member_id, config fingerprint)`.
A running child cannot honor a changed model / permission / tools / mcp / cwd /
system prompt mid-flight, so a turn with a different fingerprint gets a different
(new) resident rather than a silently-wrong one.

Leak safety: `ResidentClaude` closes stdin and reaps its child on `Drop`, so a
pool that goes out of scope never leaks PIDs (the harness does not persist a
resident PID today).

## The `HARNESS_CLAUDE_RESIDENT` flag

The resident path is gated entirely by an environment variable:

```bash
# Default: fresh `claude -p` per turn (unchanged behavior).
harness agent deliver --agent <id>

# Resident: hold `claude --input-format stream-json` open across turns.
HARNESS_CLAUDE_RESIDENT=1 harness agent deliver --agent <id>
```

When set, `run_claude_delivery` routes through `run_claude_resident_delivery_real`
instead of `run_claude_exec_delivery_real`. Both return the same in-memory
delivery outcome and Claude native session id. Harness binds a
`NativeSessionRef`; it does not retain a second NDJSON/stdout transcript.

This mirrors the documented `HARNESS_*_DELIVERY` selector convention: the feature
ships dark and opt-in, the default path is byte-for-byte unchanged, and the
resident path can be enabled per invocation.

## The resident daemon (cross-invocation warmth, unix-only)

A pool that lives inside one `harness deliver` process dies when that command
exits, so it cannot keep a child warm across deliveries (each delivery is a fresh
CLI process). The **resident daemon** is a long-lived, harness-owned host that
keeps the pool alive between invocations behind a per-workspace Unix socket. See
[decisions/0021-resident-daemon.md](decisions/0021-resident-daemon.md) (amends
0018).

```bash
# Start the warm-child host (foreground; background it with & or a supervisor).
HARNESS_ROOT=.harness harness daemon start [--idle-secs <n>] [--socket <path>]

# Inspect / stop it.
harness daemon status        # running | stale | absent
harness daemon stop          # SIGTERM via the pidfile; clean shutdown
```

- **Socket:** `<store-root>/resident.sock` (i.e. `.harness/resident.sock` or
  `$HARNESS_ROOT/resident.sock`). Both the daemon and the delivery client derive
  this path from the store root, so no registry or handshake is needed. The path
  is validated against the AF_UNIX `sun_path` limit at startup. `--socket <path>`
  overrides it but must still name `<dir>/resident.sock` so discovery stays
  consistent.
- **Pidfile:** `<store-root>/resident-daemon.pid`, written at startup; `stop`
  reads it and sends `SIGTERM`. Both files are removed on clean shutdown.
- **Stale cleanup:** on `start`, if the socket path exists the daemon probe-
  connects: a live answer means another daemon owns it (refuses to start); a
  refused/leftover socket is removed and rebound. A lost bind race is reported as
  "already running" rather than panicking.
- **Lifecycle / IPC:** line-delimited JSON, one request and one response per
  connection. The request is exactly the `run_turn` arguments; the response
  carries the per-turn frames plus `session_id` and the stderr path (stderr is
  referenced by path, never inlined). Turns serialize under one pool `Mutex`
  (the one-turn-at-a-time discipline a single child already requires), and idle
  children are reaped opportunistically after each turn.

### Hot (daemon) / cold (`--resume`) / idle model

- **Hot:** when a daemon is running, successive deliveries for a member connect to
  the socket and reuse ONE warm child — near-zero per-turn startup cost across
  *separate* CLI invocations.
- **Cold (`--resume`):** if the warm child has died, the pool respawns it with
  `--resume <session_id>` so conversation memory carries across the crash.
- **Idle:** a child idle beyond `--idle-secs` (default 300s) is dropped (stdin
  EOF) and respawned on the next turn, so resident processes do not linger.

### How delivery chooses hot vs inline

`run_claude_resident_delivery_real` (still gated by `HARNESS_CLAUDE_RESIDENT=1`)
is daemon-first: if `daemon_is_available(store_root)` (a successful probe-connect)
it sends the turn over the socket and maps the response into the SAME
`(success, events, session_id, stderr)` tuple. When no daemon is present it falls
through to the inline single-turn resident path below — the exact code that ran
before the daemon existed. If a connect succeeds but the round-trip then fails
(daemon died mid-turn), the delivery is reported as failed rather than retried
inline, because the turn may have partially run against the warm child.

## Scope

Without a daemon, the resident path uses **one resident per delivery** (a pool of
size 1): it amortizes provider warmup within a single delivery and shuts down
cleanly on return. With a daemon, `resident::ResidentPool` is hosted across
invocations as described above. The daemon's pool is global (it serializes turns
across all members in v1); per-member lock sharding is future work.

This document covers **Claude only**. Codex stays on the `codex exec --json` +
`codex exec resume <id>` respawn model — see
[decisions/0020-codex-persistent-service-exploration.md](decisions/0020-codex-persistent-service-exploration.md)
for why a persistent Codex service was rejected.
