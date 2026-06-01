# Runtime & Persistence Decision

The deliverable study: a 3-way comparison of how Claude Code teams, Multica, and
our harness execute and persist agents, followed by an honest judgement of the
owner's proposal ("tmux + claude for real persistence; implement one-shot Claude
exec AND Codex exec in one pass") and a concrete recommended model for **our**
harness. Ties back to
[0018](../decisions/0018-exec-stream-primary-substrate.md) and
[agent-integration-model.md](../agent-integration-model.md). Source evidence:
[claude-code-agent-teams.md](claude-code-agent-teams.md),
[multica-architecture.md](multica-architecture.md), and the harness source cited
inline.

## 3-way comparison

| Dimension | Claude Code teams | Multica | Our harness (post-0018) |
| --- | --- | --- | --- |
| **Substrate** | In-process async, one Node.js event loop; `AsyncLocalStorage` per teammate | Edge daemon goroutine spawns one vendor CLI subprocess per task | One cold subprocess per claimed delivery (`codex exec --json`, `claude -p stream-json`) |
| **Persistence mechanism** | In-memory app state; team file + task list + mailbox on disk | Postgres = source of truth; `(session_id, work_dir)` rows; isolated workdirs | Append-only JSONL + global `flock` + per-row `fsync`; latest-wins projection |
| **One-shot vs persistent** | **Persistent** within a session (resident idle-poll loop) | **One-shot** subprocess per task | **One-shot** subprocess per delivery |
| **Multi-turn / resume** | None for teammates (no session id); `LocalAgentTask` resumes via `--resume` | Yes — vendor CLI `--resume <session_id>`, workdir reuse, poisoned-session guard | **Declared but unused**: `session_id`/`provider_thread_id` persisted, never read back into a `--resume`/`--session` arg |
| **Coordination** | File mailbox (`inboxes/{agent}.json`), 500ms poll; in-proc permission bridge | DB claim + `events.Bus` + `daemonws` WS wakeups; comments/@mentions/squads | Harness mailbox; pull-based gateway tick; 150ms SSE file-tail |
| **Autonomy** | Leader-driven; teammates idle until messaged | Server cron `runAutopilotScheduler` manufactures issues; event re-trigger | Gated shallow single-round tick (observe→plan→decide→deliver); foreground sleep-loop |
| **Cost / complexity** | High in-session (whole team in one heap; ~292 agents → ~36.8GB); zero durability | High infra (Postgres + Redis + daemon + sweeper) but cheap per-agent; strong hardening | Lowest: no DB, no daemon, no socket, no pty; restart-safe by construction |

Source anchors: Claude Code `inProcessRunner.ts:883-1552`,
`teammateMailbox.ts:134-192`, `spawnInProcess.ts:120-122`; Multica
`daemon.go:1842-2032`, `:2727-2729`, `claude.go:489-527`, `task.go:839-943`;
harness `main.rs:7226-7276` / `:7713-7798`, `harness-store/src/lib.rs:138-181` /
`:261-280` / `:324-333`, `main.rs:3804-3849`, `sse.rs:80-226`,
`main.rs:1239-1314`.

### Reading the table

The three systems sit at distinct points:

- **Claude Code = maximum in-session statefulness, zero durability.** Great for a
  live human-driven session; useless across restarts; memory-bound.
- **Multica = ephemeral compute + durable DB + resume.** The subprocess is
  disposable; the *conversation* survives in `(session_id, work_dir)` and is
  replayed via the vendor CLI's `--resume`. No process is kept alive.
- **Our harness = ephemeral compute + durable JSONL, but resume not yet wired.**
  We already match Multica's substrate shape; we just do not consume the session
  ids we persist.

The convergent lesson across both external systems: **the durable thing is the
session record, not the process.** Even Claude Code's only restart-surviving
task type (`LocalAgentTask`) gets there via transcript + `--resume`, not a live
process. Nobody keeps a long-lived agent process for durability.

## Evaluating the owner's proposal

Proposal: *"tmux + claude for real persistence; implement one-shot Claude exec
AND Codex exec in one pass."* Taken apart:

### (a) Is one-shot exec the right default? — Yes.

It is already proven for both providers in current source: `codex exec --json`
(`main.rs:7226-7276`) and `claude -p --output-format stream-json --verbose`
(`main.rs:7713-7798`), each with null stdin and a kill-on-timeout reap, both
normalized into the same neutral `AgentEvent`/`ProviderSession` rows and served
over SSE. It is the cheapest substrate (no DB, no daemon, no socket, no pty —
confirmed: grep for `tmux`/`pty`/`app-server`/`--listen`/`WebSocket`/`jsonrpc`
returns nothing in non-test `crates/`), it is restart-safe by construction
(crash mid-turn just leaves a reclaimable lease, `main.rs:3851-3886`), and it is
exactly what 0018 ratified as primary. Multica independently lands on the same
per-task-subprocess substrate at scale. **Keep one-shot exec-stream as the
default. Implementing both providers' exec in one pass is the right move** and
is the single biggest 0018 value unlock (it makes the Claude branch real and
0011 true).

### (b) When is a PERSISTENT runtime actually needed?

Persistence buys exactly four things; only some justify a long-lived
process/socket, and most are better served by session-resume:

| Need | Does one-shot exec suffice? | Cheapest sufficient mechanism |
| --- | --- | --- |
| **Multi-turn back-and-forth** (Lead ↔ member over several deliveries) | No — each exec is a cold turn | **Session-resume** (`--resume`/`--session`); no live process |
| **Long mid-task context** (large accumulated state across turns) | No — cold turn re-pays context | **Session-resume** (provider keeps the thread); workdir reuse for files |
| **Expensive cold-start** (model warmup, MCP/tool init per turn) | Tolerable for now; costly at high turn rate | Optional **warm pool** of pre-spawned execs (0018 already allows "a small warm pool") |
| **Interactive mid-turn approval / steering** (approve a tool call *during* a turn) | **No** — one-shot exec must pre-resolve tool policy | **Persistent bidirectional channel** (app-server / SDK streaming) — the genuine case |

So the only need that *truly* requires a resident bidirectional process is
**live mid-turn approval/steering**, which 0018 already isolates as the
explicitly-flagged `app-server` fallback (`0018:76-78,102-112`). The other three
needs — which are what "real persistence" usually means in practice — are met by
**session-resume**, which keeps the cheap one-shot substrate and adds no daemon.

### (c) On "tmux + claude" specifically — do not adopt it as the persistence mechanism.

tmux/pty gives you a long-lived *interactive REPL* in a pane. But:

- It does not give durable persistence — a tmux pane dies with the host/session,
  exactly like Claude Code's in-process teammates; it adds a process to babysit
  without a restart story.
- Claude Code itself, despite modeling a `tmuxPaneId` field, runs teammates
  **in-process, not in tmux** (`team.json` members carry an empty `tmuxPaneId`;
  see [claude-code-agent-teams.md](claude-code-agent-teams.md)).
- It forces parsing the interactive TUI instead of the documented headless
  stream — the opposite of 0018's "use the documented exec/stream mode" and of
  Multica's approach.
- The real multi-turn win the owner wants is **conversation continuity**, and
  Claude delivers that headlessly via `--resume <session_id>` + the `result`
  event's `session_id` — no pane required.

Verdict: the *instinct* (we need real persistence for multi-turn) is right; the
*mechanism* (tmux) is the wrong tool. Use **session-resume** for multi-turn, and
reserve a **persistent bidirectional runtime** only for live mid-turn approval —
and when that day comes, use the documented `app-server` / SDK streaming path
(0018's fallback), not a tmux-driven REPL.

## Recommendation for our harness

**Keep exec-stream one-shot as the default substrate; add an *optional*
persistent-runtime mode whose first and primary form is session-resume, not a
live process.** Three tiers, opt-in per member, behind a delivery-mode flag, all
mapping onto the same neutral `ProviderSession`/`AgentEvent` rows:

1. **Tier 0 — one-shot exec (default, today).** Unchanged from 0018. Cold turn
   per delivery; lease/claim for crash safety.
2. **Tier 1 — session-resume (the "persistent" mode we should actually build).**
   Read back the `session_id` we *already persist* and pass it as the provider's
   resume arg on the next delivery to the same member/goal. Still one subprocess
   per turn — durable conversation, no resident process, restart-safe, and it
   gives free orphan recovery (Multica's exact pattern).
3. **Tier 2 — persistent bidirectional process (narrow, flagged).** Only for
   live mid-turn approval/steering: the `app-server`/SDK streaming path that
   0018 already designates as the fallback. Not the default, not tmux.

This is uniform across providers and matches what each actually supports:

| Provider | Tier 1 resume | Tier 2 persistent | What we already capture |
| --- | --- | --- | --- |
| **Claude** | `claude -p --resume <session_id>` (id from the `result` event) | Claude Agent SDK streaming (bidirectional) | `session_id` via `extract_session_id_from_claude_events` (`main.rs:7778`/`7804`) |
| **Codex** | `codex exec --session <thread_id>` / resumed thread | `codex app-server` (the retired-but-retained socket path) | `provider_thread_id`/`provider_turn_id` (`main.rs:7396-7397`) |

The gap is purely consumption: we persist `provider_thread_id` /
`provider_session_ref` on `ProviderSession`
(`harness-core/src/lib.rs:565-577`) but **no delivery reads them back** into a
`--resume`/`--session` arg (grep for `--resume` in `main.rs` returns nothing).
Tier 1 closes that gap with the data we already have.

### Sequenced WP plan

| WP | Title | Scope | Size |
| --- | --- | --- | --- |
| **WP-A** | Both-provider one-shot exec to projection parity | Finish/confirm the Claude `-p stream-json` branch alongside Codex `exec --json`, normalize both into the same `AgentEvent`/`ProviderSession` rows behind the delivery-mode flag (0018's primary). One pass, both providers, as the owner asked. | M |
| **WP-B** | Tier-1 session-resume (read-back) | On the next delivery to the same `(member, goal)`, read the persisted `session_id`/`provider_thread_id` and pass `claude --resume` / `codex --session`. Add the poisoned-session guard (exclude failure-tagged sessions from resume lookup, per Multica `daemon.go:2905-2925`). | M |
| **WP-C** | Workdir reuse + GC for resumed turns | Reuse the per-member workdir across resumed turns; GC that reaps `node_modules/.next/.turbo` but preserves `source/.git/output/logs` (Multica `CLI_AND_DAEMON.md:185-193`). Optional small warm pool if cold-start latency bites. | S–M |
| **WP-D** | Retire the Codex app-server WS-over-UDS path | Per 0018's "Retire (later WP)": remove WS framing, socket lifecycle, `--listen` spawn + socket-poll, dead test framer — once WP-A/B reach parity. | M |
| **WP-E** | Tier-2 persistent bidirectional (optional, deferred) | Only if live mid-turn approval is required: revive `app-server`/SDK streaming as the explicitly-flagged Tier-2 mode. Honest capability declaration so the Dashboard does not fake mid-turn interrupt on exec members (`0018:99-100`). | L (defer) |

Recommended order: **A → B → C → D**, with **E deferred** until a concrete
mid-turn-approval requirement appears. WP-A delivers the owner's "one-shot Claude
exec AND Codex exec in one pass"; WP-B delivers the "real persistence" they want
— via resume, not tmux.

### Provider persistence support (ground truth)

- **Claude**: `claude -p --output-format stream-json --verbose` is one-shot;
  multi-turn continuity is `--resume <session_id>` where the id arrives in the
  terminal `result` event (we already extract it). The Claude Agent SDK is the
  same loop with typed events and can hold a bidirectional stream for Tier 2.
- **Codex**: `codex exec --json` is one-shot NDJSON, resumable via `--session`
  (thread id we already capture as `provider_thread_id`). `codex app-server`
  (WebSocket-over-UDS + JSON-RPC) is the persistent bidirectional path — the one
  0018 supersedes-in-part 0008 to demote to flagged fallback, and the path
  WP-D retires.

## Tie-back

This recommendation **reinforces 0018**: exec-stream stays primary; Tier-2
persistent is exactly 0018's flagged mid-turn-approval fallback; WP-D is 0018's
"Retire (later WP)" item. It fills the one thing 0018 left implicit — that the
*persistent* answer for multi-turn should be **session-resume (Tier 1)**, not a
long-lived process — and it maps cleanly onto the neutral launch spec and the
three pillars in
[agent-integration-model.md](../agent-integration-model.md). No ADR change is
required; if Tier 1 is adopted as policy it should be recorded as a short ADR
that cites this study and 0018.
