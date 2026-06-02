# 0018: Headless Exec-Stream as Primary Provider Substrate

## Status

Accepted.

Amended by [0021 (resident-daemon warm-child host)](0021-resident-daemon.md):
clarifies that keeping documented exec-stream children warm behind an internal
Unix-socket host is the "small warm pool" this ADR allows, not a return to a
bespoke provider protocol. 0018 stays Accepted.

Supersedes [0008 (Persistent Codex Agent Runtime)](0008-persistent-codex-agent-runtime.md)
**in part**: it inverts which substrate is primary versus fallback. It
**reinforces** [0011 (Provider-Neutral Runtime)](0011-provider-neutral-runtime.md):
one uniform exec-stream substrate for every provider is the cleanest realization
of the neutral contract and finally lets Claude implement it for real.

(Requested as "0012"; `0012` is already taken by
[0012-dashboard-is-control-plane.md](0012-dashboard-is-control-plane.md), so the
next free number, 0018, is used.)

## Context

ADR 0008 committed V1 to **persistent Agent Members backed by
`codex app-server`**, one process per member, with `codex exec` as a fallback.
The implemented Codex path therefore drives `codex app-server --listen
unix://…`: it spawns and `setsid`s the server, polls for the socket file,
performs a WebSocket HTTP upgrade over the Unix domain socket, and hand-rolls
the JSON-RPC state machine (`initialize` → `initialized` → `thread/start` →
`turn/start`), blocking on frames until a terminal event.

Two facts undermine that as the *primary* substrate:

1. **The app-server WebSocket-over-UDS path is a Tier-3, undocumented protocol
   family.** It is the same persistent bidirectional protocol that external
   integration guidance explicitly flags as "avoid for now — undocumented, spec
   may change." We maintain a large, fragile, bespoke transport surface
   (WS framing + JSON-RPC + socket lifecycle, plus dead test-only framer code)
   to obtain an event stream that `codex exec --json` emits as NDJSON on stdout.
2. **The Claude side is a non-functional stub.** It spawns no process, invokes
   bare `claude` with zero CLI args (which launches the interactive REPL, not a
   headless turn), and its output ingest is "not implemented". The one place
   ADR 0011's provider-neutral promise was supposed to be proven is unbuilt —
   and the stub already *assumes* a per-delivery exec paradigm, directly
   contradicting 0008's persistent-process commitment. The architecture is
   internally inconsistent: Codex is persistent-socket, Claude is (intended)
   per-delivery exec, both behind one runtime type.

Meanwhile, the **documented** external-integration substrate for *both*
providers is headless exec + a structured event stream:

- Codex: `codex exec --json` emits newline-delimited JSON, one event per state
  change (tool call, output, completion), resumable via `--session`.
- Claude: `claude -p --output-format stream-json --verbose` emits NDJSON in real
  time (`system` init, `stream_event` frames with text deltas, tool_use,
  tool_result, terminal `result` with `session_id`), resumable via `--resume`.
  The Claude Agent SDK is the same loop with typed events.

This is exactly the substrate a dashboard renders, and it is uniform across
providers — one mental model, one parser family.

## Decision

**Headless exec-stream is the primary provider integration substrate.**

- Drive each provider through its documented headless mode:
  `codex exec --json` and `claude -p --output-format stream-json` (or the
  Claude Agent SDK as a thin typed sidecar), one run per claimed delivery
  (optionally a small warm pool if latency matters).
- Normalize the NDJSON/event stream into the **same** neutral `AgentEvent` /
  `ProviderSession` rows the existing path writes, served to the Dashboard over
  SSE. The neutral object model, the harness-owned mailbox, and the atomic
  claim/lease are substrate-independent and are **kept verbatim**.
- Generalize the `control_endpoint` from `unix://socket` to a provider-neutral
  delivery handle (process/session descriptor); neither provider needs a
  long-lived socket in the target design.
- The integration contract is the three pillars + neutral launch spec in
  [../agent-integration-model.md](../agent-integration-model.md). New platforms
  integrate via their documented exec/stream mode first.

**Persistent `app-server` is retained only as an optional, explicitly-flagged
fallback** for members that genuinely require live **mid-turn approval** (see
risk below). It is no longer the default or the acceptance target.

## Consequences

- **Keep:** the neutral object model (`AgentMember`, `AgentProviderConfig`,
  `AgentEvent`, `ProviderSession`), the neutral seam, harness-owns-mailbox +
  atomic claim/lease, JSONL-as-canonical, and SSE-as-delivery.
- **Retire (later WP):** the Codex app-server WebSocket-over-UDS path — WS
  framing, socket lifecycle, the `--listen` spawn + socket-poll, and the dead
  test-only LSP framer — once an exec path reaches projection parity behind a
  delivery-mode flag.
- **Replace (later WP):** the bare-`claude` stub with a real
  `claude -p`/SDK integration and a working Claude branch of the event reducer;
  this is the single biggest value unlock and makes ADR 0011 true.
- **Abstract the launch surface:** the operator composer and Dashboard bind to
  the neutral launch spec (a `permission` enum + `writable_roots`), not to the
  Codex `app-server` vocabulary (`approval_policy`, `sandbox_policy`,
  `service_tier`, `collaboration_mode`, `developerInstructions`) that currently
  leaks into `AgentProviderConfig`. Abstracting those schema fields is additive
  future work under [0017](0017-generic-object-model.md); this ADR fixes only
  the substrate direction.
- **Honest capability state:** providers declare unsupported surfaces (e.g.
  Claude exec has no mid-turn interrupt) so the Dashboard does not fake them.

### Risk: mid-turn interactivity

The genuine thing dropping the persistent socket loses is **mid-turn
tool-approval and steering**. In one-shot `codex exec`, tool approval must be
pre-resolved by policy; an interactive mid-turn approval request would otherwise
fail. Claude's bidirectional stream could in principle support this but is
undocumented/Tier-3. Mitigation: adopt exec-stream as primary with
policy-driven pre-approval, and retain the persistent app-server path as the
documented fallback for members that require live mid-turn approval. For the
dashboard-as-UI goal (operator drives agents, watches the stream), exec-stream
is sufficient and simpler.

## Validation

```bash
npx pnpm@9.15.4 check
```

This decision is documentation-level: it sets substrate direction and the
integration model. Behavior-changing work (exec delivery path behind a flag,
real Claude integration, store/SSE correctness fixes, then retiring the
app-server path) lands in separate gated work packages. See
[../agent-integration-model.md](../agent-integration-model.md),
[../integration/codex.md](../integration/codex.md), and
[../integration/claude.md](../integration/claude.md).
