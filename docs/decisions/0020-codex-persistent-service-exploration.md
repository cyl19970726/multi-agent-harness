# 0020: Codex persistent-service exploration — keep the respawn model

## Status

Accepted (2026-06-02).

Addendum to (does **not** supersede) [0018 (Headless exec-stream as primary
provider substrate)](0018-exec-stream-primary-substrate.md). Choosing "neither
persistent service" **reinforces** 0018; it does not conflict with it. The 0018
status stays **Accepted**; this ADR records the new evidence so the question is
not re-litigated.

This decision covers **Codex** persistence only. The companion code change in
this work package implements a resident process for **Claude** (`claude
--input-format stream-json`, opt-in behind `HARNESS_CLAUDE_RESIDENT`, see
[../resident-claude.md](../resident-claude.md)). **Codex is unchanged**: it stays
on the `codex exec --json` + `codex exec resume <id>` respawn model. A persistent
Codex service remains a follow-up spike, not part of this PR.

## Context

ADR 0018 made headless exec-stream the primary substrate and retired the old
Codex `app-server` WebSocket-over-UDS transport (the deletion shipped in commit
`4772af2`, "the most fragile hand-rolled WS transport", ~924 LOC removed). With
the Claude resident path now built, the open question was whether Codex should
gain an analogous persistent service. Two candidate Codex substrates were probed
against **codex-cli 0.135.0**:

1. **`codex exec-server`** — a JSON-RPC server intended to host conversations.
2. **`codex app-server`** — the existing experimental multi-turn JSON-RPC driver
   (the same family ADR 0018 retired in its WS-over-UDS form).

### Probe report key facts

**`exec-server` is a non-functional stub in 0.135.0.** Only `initialize`
responds. Every conversation method — `turn/start`, `thread/*`, `command/exec`,
`model/list`, `config/read` — returns JSON-RPC error `-32601` with the message
"exec-server stub does not implement X yet". It literally cannot host a
conversation. Its `--help` (`--remote`, `--environment-id`,
`--use-agent-identity-auth`) points at remote-environment registration, not local
conversation hosting. It is pre-alpha and **not usable for this harness today**.

**`app-server` works and is the real resident multi-turn driver**, but:

- It is **not** a clean blocking request/response API. `turn/start` returns
  immediately; real output arrives as a stream of notifications terminated by
  `turn/completed`. The server *also initiates requests*
  (`execCommandApproval`, `applyPatchApproval`, `item/*/requestApproval`,
  `elicitation`) that the client **must** answer or the turn deadlocks. A sync
  Rust harness would need a dedicated reader thread demuxing by JSON-RPC id and
  routing by `threadId`, plus inline `ServerRequest` responders — exactly the
  bidirectional transport surface ADR 0018 deleted.
- It is fully `[experimental]` with UNSTABLE/legacy fields and a fast-moving v2
  surface (must pin codex-cli + regenerate bindings per upgrade). The generated
  schema diverges from the runtime (e.g. `clientInfo` nested vs. a flat
  `clientName` mishandshake).
- It eagerly boots all configured MCP servers on first `thread/start` (seconds of
  warmup + child processes) and inherits global `~/.codex/config.toml` + auth, so
  member isolation needs explicit per-thread overrides and likely a dedicated
  `CODEX_HOME`.
- **Both probes confirm** `codex exec resume <id>` reads the **same** on-disk
  rollout JSONL under `~/.codex/sessions/` that `app-server`'s `thread/resume`
  reads. So conversation durability across deliveries is already available
  *without* a daemon.

## Decision

**Neither persistent Codex service. Keep the `codex exec --json` +
`codex exec resume <id>` respawn model.**

Three findings make this the right call:

1. **`exec-server` is disqualified by its own probe.** It is a stub; it cannot
   run a conversation. Full stop. Revisit only when the stub is implemented.

2. **`app-server` works but its differentiating value does not pay off here, and
   adopting it reverses a decision the harness already paid to make.** Its wins
   over respawn are amortized warm state / MCP boot across turns, one process
   multiplexing many `threadId`s, and a server→client approval/elicitation
   back-channel for mid-turn approval and steering. But (a) the harness drives
   **one conversation per AgentMember** with a turn-then-watch-stream model, so
   multiplexing buys little; (b) persistence across deliveries is **already
   solved** by `codex exec resume <id>` reading the same rollout JSONL; (c) the
   one genuinely unique capability — mid-turn approval/steering — was explicitly
   **de-scoped by ADR 0018** (policy-driven pre-approval is sufficient for the
   dashboard-as-UI goal), where app-server is retained only as an optional,
   flagged fallback.

3. **The cost of `app-server` is high and concrete for a SYNC runtime.** It is
   async + bidirectional (streamed turn notifications **and** server-initiated
   approval/elicitation requests). Re-adopting it reintroduces precisely the
   fragile hand-rolled bidirectional transport class the harness already built
   once as WS-over-UDS and then deleted (commit `4772af2`). On top of that it is
   experimental, fast-moving, schema-divergent, and inherits global config/auth.

Net: `exec-server` can't run; `app-server` can run but its unique value is unused
here, its persistence value is already covered by `exec resume`, and its
async/bidirectional cost is exactly the burden ADR 0018 chose to shed. The
current respawn model wins on simplicity, on uniformity with the Claude
`claude -p` / `--resume` path (the realization of ADR 0011 provider-neutrality),
and on not regressing a deliberate, already-paid-for architectural decision.

### Runner-up

The runner-up is **`app-server`** (`exec-server` is not a contender — it is a
stub). app-server becomes the correct choice **if/when** the harness's
requirements change in one of these directions:

1. **Mid-turn tool-approval / steering becomes a hard requirement** (an operator
   must gate or redirect a tool call while a turn is in flight). exec-stream
   physically cannot do this (one-shot, pre-resolved policy); app-server's
   `ServerRequest` approval back-channel + `turn/steer` / `turn/interrupt` is the
   only documented way. This is exactly the "flagged fallback" ADR 0018 reserves.
2. **Per-turn cold-respawn + MCP-boot latency becomes a measured bottleneck**
   across many rapid turns per member — a resident app-server amortizes warm
   model/MCP state to ~free per additional turn.
3. **The runtime gains a real async event loop** (a reader thread demuxing by
   `threadId`), at which point the bidirectional cost is already paid and
   multiplexing N members on one daemon becomes attractive.

If adopted, prefer app-server over the deleted WS-over-UDS path: use
`--listen stdio://` (spawn child, NDJSON over pipes) or `unix://`, **never**
`ws://` (ws on non-loopback needs capability/bearer-token auth and was the
fragile surface). Vendor `codex app-server generate-json-schema` /
`generate-ts` output for serde types and validate the handshake against the live
binary (nested `clientInfo`, not flat).

## Consequences

- **Codex: no change.** `ProviderKind::Codex` keeps routing through
  `start_codex_exec_runtime` (no persistent PID/socket; `control_endpoint` is a
  runtime-dir marker, health = `which codex` + dir-exists) →
  `run_codex_exec_delivery` → `run_codex_exec_process`, which spawns
  `codex exec --json <envelope>` for a fresh conversation or
  `codex exec resume --json <id> <envelope>` when the member carries a prior
  `provider_thread_id`. Memory carries across deliveries via the on-disk rollout.
  Transport stays dead simple (stdin=null, stdout NDJSON parsed by
  `parse_codex_ndjson`, stderr captured, `try_wait` timeout loop, kill on
  expiry). No reader thread, no id correlation, no `ServerRequest` responders, no
  socket lifecycle. `thread.started` yields the resumable session id recorded
  into `ExecDeliverySessionRecord`.
- **Claude: resident path added (this PR), opt-in.** Same lifecycle *shape* as
  Codex (`claude -p --output-format stream-json` + `--resume <session_id>`) —
  one parser family, one mental model, two providers — but held open across turns
  behind `HARNESS_CLAUDE_RESIDENT=1`. Default Claude behavior is unchanged.
- **If the app-server fallback is ever needed**, gate it behind an explicit
  delivery-mode flag exactly as ADR 0018 prescribes; implement it as a
  `stdio://` (or `unix://`) child with a single dedicated reader thread demuxing
  by JSON-RPC id + routing turn events by `threadId` and auto-answering
  `ServerRequest` approvals under `approvalPolicy: never` + a permissive sandbox.
  Never reintroduce the `ws://` path.

### Relationship to ADR 0018

No supersession. ADR 0018 already (a) makes headless exec-stream the primary
substrate, (b) retains persistent app-server only as an optional,
explicitly-flagged fallback for members that genuinely require live mid-turn
approval, and (c) records the WS-over-UDS retirement in `4772af2`. Picking
`exec-server` or `app-server`-as-default would have required revising 0018;
picking **neither leaves its decision intact**.

Recommended clarifying addendum to 0018 (status stays Accepted), so the question
is not re-litigated:

> Re-evaluated 2026-06 against codex-cli 0.135.0 probes. `exec-server` is a
> non-functional stub in 0.135.0 (only `initialize` works; all conversation
> methods return `-32601`) and is not a viable substrate; revisit when the stub
> is implemented. `app-server` remains viable and is the correct realization of
> the 0018 fallback (use `stdio://` or `unix://`, never the deleted `ws://`
> path), but is **not** promoted to default: the harness is a sync runtime,
> app-server requires an async bidirectional reader (streamed turn output +
> server-initiated approval/elicitation requests) — the exact fragile transport
> class deleted in `4772af2` — and its persistence value is already provided by
> `codex exec resume <id>` reading the same rollout JSONL that `thread/resume`
> reads. Mid-turn approval/steering remains the only differentiator and remains
> de-scoped per the original risk section. Decision direction unchanged.

Optionally, 0018's Consequences "Retire (later WP)" bullet may be updated to past
tense, since that retirement has shipped (`4772af2`).

## Validation

Documentation-level decision. The behavior-changing companion (Claude resident
path) is validated by the `resident` module tests:

```bash
cargo test -p harness-cli resident
```

See [../resident-claude.md](../resident-claude.md) for the resident principle and
the `HARNESS_CLAUDE_RESIDENT` flag.
