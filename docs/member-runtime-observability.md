# Member Runtime Observability

This document is the canonical contract for **observing an `AgentMember`'s
real-time state**: what "live" means for a member, which signals compose that
state, how they are delivered to the Agent Dashboard, and what is real versus
aspirational per provider. It is provider-neutral; provider-specific shapes live
under [integration/](integration/).

It sits between the runtime object model in [agent-runtime.md](agent-runtime.md)
and the control-plane doctrine in [agent-control-plane.md](agent-control-plane.md).
It does not redefine `AgentMember`, `AgentRuntime`, `ProviderSession`,
`AgentEvent`, or `Message`; it defines how a viewer reconstructs a member's live
state from those objects.

## Purpose And Scope

Observing a member means answering, at any moment and without opening provider
logs: *is this member alive, is it executing right now, what message is it
reacting to, and can I trust that answer?*

Two doctrines bound this contract:

- **Process-alive is not execution-ready.** A live pid or an open socket proves
  only that a process exists. It does not prove the member can accept work,
  reach a terminal turn, or deliver a message. The Dashboard must not present
  process health as execution readiness when protocol or delivery health is
  unknown (see [agent-control-plane.md](agent-control-plane.md), "Lifecycle").
- **The store is canonical; the provider transcript is evidence.** Real-time
  signals describe the canonical harness store. Provider stdout, hooks, and
  sessions are evidence inputs reduced into that store, never the source of
  truth (ADR [0011](decisions/0011-provider-neutral-runtime.md) and
  [0008](decisions/0008-persistent-codex-agent-runtime.md)).

Out of scope: the runtime object semantics ([agent-runtime.md](agent-runtime.md)),
the delivery queue policy ([agent-control-plane.md](agent-control-plane.md)),
and provider wire protocols ([integration/codex.md](integration/codex.md),
[integration/claude.md](integration/claude.md)).

## The Three Signals

Member real-time state is composed from three independent signals. No single one
is sufficient; each covers a failure mode the others miss.

| Signal | Direction | Granularity | Trust | Failure mode it covers |
| --- | --- | --- | --- | --- |
| AgentEvent stream | hook / parser **PUSH** | fine, fast (sub-second) | best-effort, **can drop** | "what is the member doing right now" |
| `runtime_health` probe | **PULL** (on demand) | coarse, slow | trustworthy fallback | "is the member actually executable" |
| ProviderSession lifecycle | reduced from events | one in-flight turn | terminal-backed | "is there a live turn, and did it finish" |

### (a) AgentEvent stream — fine, fast, can drop

`AgentEvent` rows are pushed as the provider acts: prompt submit, tool calls,
generation start/complete, turn start/complete, child-thread spawn. They are the
lowest-latency view of activity and feed the Member conversation + action
stream. They are **best-effort**: a hook can be missed, a parser can fail to
classify a line, a connection can drop. A gap in the event stream is not proof
the member stopped — it is the reason the other two signals exist.

### (b) `runtime_health` probe — coarse, slow, trustworthy fallback

`runtime_health` (Rust `AgentRuntimeHealth`) is computed on demand by the CLI and
attached to the runtime. It is the authoritative "is this member executable"
answer when the event stream is silent. It has four layers plus a timestamp:

| Layer | Field | Means | Codex sense |
| --- | --- | --- | --- |
| process | `process_alive` | runtime pid exists and has not exited | app-server process alive |
| socket | `socket_exists` | control endpoint accepts connections | unix control socket present |
| protocol | `protocol_probe` | provider `initialize`/probe succeeds | JSON-RPC initialize ok |
| delivery | `delivery_probe` | a message reached a terminal delivered/failed state with a provider-session record | `turn/start` reconciles to a terminal event |
| (checked) | `checked_at` | when the probe last ran | freshness of the answer |

`protocol_probe` and `delivery_probe` are `Option<String>` (e.g. `pass` /
`fail` / `unknown`); `process_alive` and `socket_exists` are booleans;
`checked_at` is an ISO timestamp.

**Amber-on-unknown rule.** When a layer is `unknown` (or `checked_at` is stale),
the Dashboard renders it amber, **not** green. Green requires a positive probe.
Process/socket green with protocol/delivery unknown is amber overall — never
"ready" — because that is exactly the process-alive-not-execution-ready trap.

### (c) ProviderSession lifecycle — the live in-flight turn

A `ProviderSession` is one provider interaction (the current or most recent
turn). Its lifecycle answers "is a turn in flight, and did it terminate?" A turn
is terminal only on a real provider signal (`turn/completed`, thread-idle plus
rollout, or a Stop-hook report) — not on a timeout. This is the signal that
distinguishes *busy* from *idle* and prevents declaring success from activity
alone.

### Why no single signal suffices

- Events alone can drop, so silence is ambiguous (covered by the health probe).
- The health probe alone is coarse and on-demand, so it cannot show what the
  member is doing token-by-token (covered by the event stream).
- Neither alone says whether the **current turn** terminated cleanly (covered by
  the ProviderSession lifecycle).

The Dashboard composes all three: live activity from events, executability from
`runtime_health`, in-flight turn state from `ProviderSession`.

## Provider-Neutral Seam

The PUSH channel that produces events is **provider-specific**. The objects that
land in the store are **provider-neutral**. This is the ADR-0011 line: a new
provider may produce events however it wants, but it must reduce them into the
same `AgentEvent` / `ProviderChildThread` / `Message` / `ProviderSession`
shapes, and it must not redefine core object meaning.

- **Codex** pushes via a hook configuration (SessionStart, PostToolUse,
  SubagentStart/Stop, Stop) plus app-server notifications. See
  [integration/codex.md](integration/codex.md).
- **Claude** has **no Codex-style hook**. Its real-time events come from
  **parsing the CLI / session output** (session-start, turn-start,
  generation-completed, turn-completed, subagent-spawn). Hook dispatch is a
  **no-op for Claude**; the parser is the event source. See
  [integration/claude.md](integration/claude.md).
- **Kimi** likewise has no hook surface. Its events come from parsing the flat
  `kimi -p --output-format stream-json` NDJSON (one frame per line, reduced to
  `AgentEvent`s). See [integration/kimi.md](integration/kimi.md).

| Concern | Provider-neutral (core) | Provider-specific (CLI layer) |
| --- | --- | --- |
| Event production | — | Codex hook push; Claude/Kimi CLI stream parsing |
| Hook dispatch | — | Codex hooks fire; **Claude hooks no-op** |
| Landed event | `AgentEvent` | provider event_type strings |
| Child thread | `ProviderChildThread` | Codex `collab_agent_spawn`; Claude `subagent-spawn` |
| Turn record | `ProviderSession` | terminal_source (`turn_completed`, `thread_idle`, `thread_read`, `hook_stop`, `dry_run`, `failed`, `unknown`) |
| Reply / report | `Message` | parsed stdout / rollout |
| Health shape | `runtime_health` four layers | which layers are real vs `unknown` |

The Dashboard reads only the neutral objects. It never branches on provider to
render a member's live state.

## Delivery To The Dashboard

### `/v1/events` — the SSE contract

The harness serves real-time events over Server-Sent Events at
`GET /v1/events` (headers: `text/event-stream`, `no-cache`, `keep-alive`,
`Access-Control-Allow-Origin: *`). The canonical endpoint spec lives in
[agent-runtime.md](agent-runtime.md), "Real-Time Event Streaming (SSE)"; this is
the observability-facing summary.

Frame kinds:

| Frame | Sent | Payload |
| --- | --- | --- |
| `snapshot` | once, on connect | `{ generated_at }` (timestamp only; client resyncs via `/v1/snapshot`) |
| `agent_event` | on each new `AgentEvent` append | the `AgentEvent` JSON |
| `message` | on `Message` create / delivery-status change | the `Message` JSON |
| `provider_session` | on `ProviderSession` create / status change | the `ProviderSession` JSON |
| `workflow_run` / `workflow_step` | on `WorkflowRun` / `WorkflowStep` append or update | the run / step JSON |
| `provider_turn_event` / `provider_turn_event_normalized` | on each raw turn-event line teed to `provider_turn_events.jsonl` | the raw line / its normalized `HarnessTurnEvent` expansion |

The stream is project-scoped (`?project=<id>`): a client subscribes to one
project's channel and frames from other projects never leak in.

Mechanism: a background **watcher thread** polls each project's jsonl store
files (`agent_events.jsonl`, `messages.jsonl`, `provider_sessions.jsonl`,
`workflow_runs.jsonl`, `workflow_steps.jsonl`, `provider_turn_events.jsonl`) for
byte growth (~150ms), parses each newly appended line, and broadcasts a frame
over a **crossbeam channel fan-out** to that project's subscribed clients. The
project registry is re-scanned every poll, so post-startup projects stream live
without a serve restart. End-to-end append→frame latency is **sub-second**. A
keepalive comment (`: keepalive`) is sent roughly every
15s of idle to defeat proxy/client timeouts. Each connection is handled on its
**own thread**, so a long-lived stream (which blocks for the life of the client)
cannot starve POST actions, snapshot polls, or other SSE clients.

### `/v1/snapshot` — initial load and reconnect fallback

`GET /v1/snapshot` returns the full dashboard read model. It is the initial-load
source and the reconnect resync point: the SSE `snapshot` frame carries only a
timestamp, so on connect (and after any drop) the client refetches
`/v1/snapshot` to rebuild a complete base before applying deltas.

### Frontend — EventSource, incremental merge, three modes

`openEventStream` opens an `EventSource` against `{base}/v1/events` and routes
each named frame; a malformed `data:` payload is dropped (logged), never tearing
the stream down. `applyFrame` performs an **incremental latest-wins merge**:
each `agent_event` / `message` / `provider_session` is upserted by `id` (replace
in place or append) into the in-memory snapshot, and `generated_at` advances so
the freshness chip reads fresh — **no full re-fetch per delta**.

`useEventStream` manages the connection and surfaces a mode for the chip:

| Mode | Chip | Meaning |
| --- | --- | --- |
| `sse` | **live (SSE)** | stream connected and pushing deltas |
| `polling` | **polling** | stream down; interval `/v1/snapshot` poll (~5s) took over |
| — | **not connected** | no live source loaded; honest empty workspace (no baked-in fixture) |

On error/close the source is torn down deliberately and a reconnect is scheduled
with **exponential backoff** (1s, 2s, 4s, 8s, capped 15s); a clean reconnect
resets the ladder. While not connected, the interval poll keeps the view fresh,
so SSE is an optimization over a polling floor, never a single point of failure.

## End-To-End Flow

```text
provider activity
  ├── Codex: hook push (SessionStart/PostToolUse/Stop) + app-server notifications
  └── Claude / Kimi: CLI stream parsing  (hook dispatch no-ops)
        |
        v
  reducer -> AgentEvent / Message / ProviderSession / ProviderChildThread
        |
        v
  append to jsonl store  (agent_events / messages / provider_sessions /
        |                  workflow_runs / workflow_steps / provider_turn_events .jsonl)
        |                                  ^ canonical store (source of truth)
        v
  serve watcher thread   (poll ~150ms per project, detect byte growth, parse new lines)
        |
        v
  crossbeam broadcast    (fan-out to the project's subscribed clients)
        |
        v
  GET /v1/events (SSE)   per-connection thread; snapshot frame + keepalive
        |   sub-second append->frame
        v
  EventSource (browser)  openEventStream -> named frame handlers
        |
        v
  applyFrame             incremental latest-wins merge by id into snapshot
        |   (reconnect / initial: GET /v1/snapshot resync)
        v
  read model             live(SSE) | polling | offline chip
        |
        v
  Member conversation + action stream
                         (timeline grouped by ProviderSession)
```

## Invariants And Limits

1. **The store stays canonical.** SSE is **advisory delivery**. If a frame is
   missed, the truth is still the jsonl store; `/v1/snapshot` reconstructs it.
2. **Near-real-time, not hard-real-time.** The ~150ms watcher poll and
   sub-second delivery window are intentional. Do not build logic that assumes
   millisecond delivery or guaranteed ordering across the watched jsonl files.
3. **Amber on unknown.** A layer that has not produced a positive probe is amber,
   never green. Process/socket alone never reads as "ready."
4. **Per-provider reality of the four health layers:**

| Provider / future shape | process | socket / endpoint | protocol | delivery |
| --- | --- | --- | --- | --- |
| Codex (local app-server) | real (pid) | real (unix socket) | real (initialize) | real (turn terminal) |
| Claude (CLI) | real-ish (binary present / pid per delivery) | endpoint = runtime dir | as available | as available (receipt proof) |
| Kimi (CLI) | real-ish (binary present / pid per delivery) | endpoint = runtime dir | as available | as available (receipt proof) |
| Future Claude HTTP/SDK | degrades to "session exists" | degrades to "API reachable" | "API reachable" | "request accepted / completed" |

   Codex local processes can satisfy all four layers truthfully. The Claude CLI
   has no persistent pid; process/endpoint are best represented as
   binary-present / runtime-dir-exists, and protocol/delivery are filled as the
   parsed session makes them available. A future Claude HTTP/SDK provider would
   degrade process/endpoint to "session exists / API reachable." A layer a
   provider cannot satisfy is `unknown` (amber) or not-applicable, never a
   green claim.
5. **Events can drop; health and session cannot lie.** Treat an event gap as
   missing information and fall back to the probe and the ProviderSession
   terminal state, never as a definitive "stopped."

## Cross-Links

- [agent-runtime.md](agent-runtime.md) — A-ROM objects and the `/v1/events`
  endpoint spec.
- [agent-control-plane.md](agent-control-plane.md) — lifecycle health layers and
  the process-alive-is-not-execution-ready doctrine.
- [concept-model.md](concept-model.md) — source-of-truth rules; provider
  transcript is evidence.
- [integration/codex.md](integration/codex.md) — Codex hook push and four-layer
  health.
- [integration/claude.md](integration/claude.md) — Claude CLI-output parsing and
  no-op hook dispatch.
- [integration/kimi.md](integration/kimi.md) — Kimi flat stream-json parsing and
  degraded capability surface.
- [decisions/0011-provider-neutral-runtime.md](decisions/0011-provider-neutral-runtime.md)
  and [decisions/0008-persistent-codex-agent-runtime.md](decisions/0008-persistent-codex-agent-runtime.md)
  — the provider-neutral seam and the canonical-store doctrine.
