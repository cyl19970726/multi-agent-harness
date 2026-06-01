# Research: Multica Architecture

A faithful writeup of Multica's multi-agent runtime. Source paths cite a local
read-only checkout (`.research-cache/multica`, HEAD `2bb2d13e`); paths are kept
verbatim (`server/internal/daemon/daemon.go:1842`, etc.) so claims are
checkable.

**Diagrams.** Detailed ASCII deployment, lifecycle, end-to-end data-flow,
concurrency/slot, and resume diagrams are in the companion
[multica-architecture-diagrams.md](multica-architecture-diagrams.md).

## Answer first: how does Multica get persistence and concurrency?

**Postgres is the single source of truth; agents are per-task ephemeral CLI
subprocesses; continuity is session-resume, not a long-lived process.** There is
no container-per-agent, no daemonized agent, and no tmux/pty. Concurrency comes
from **goroutines inside one edge daemon process**, each spawning a short-lived
vendor CLI per task. Cross-turn / cross-restart continuity is reconstructed from
a persisted `(session_id, work_dir)` per task and replayed through the vendor
CLI's own `--resume` (`daemon.go:2727-2729`, `claude.go:521-523`). The server
holds no compute: it is a coordinator/state-store; the daemon is the executor.

## Component map

| Component | Stack | Process | Path |
| --- | --- | --- | --- |
| Web frontend | Next.js 16 App Router | Vercel/Node | `apps/web/` |
| Desktop | Electron (electron-vite) | local | `apps/desktop/` |
| Mobile | Expo / React Native | device | `apps/mobile/` |
| API server | Go (Chi, sqlc, gorilla/websocket) | server daemon | `server/cmd/server/` |
| Agent daemon + CLI | Go (one binary `multica`) | **on user's machine** | `server/cmd/multica`, `server/internal/daemon/` |
| Database | PostgreSQL 17 + pgvector | — | `server/migrations/` |
| Optional Redis | realtime relay + liveness + empty-claim cache | — | `server/cmd/server/main.go:177-230` |

```text
 Next.js / Electron / Expo  ──HTTP+WS──►  Go API server (Chi)  ──sqlc──►  Postgres(+pgvector)
   (React Query / Zustand)              │  - events.Bus (in-proc pub/sub)       ▲
                                        │  - realtime.Hub (browser WS)          │ optional Redis
                                        │  - daemonws.Hub (daemon wakeups)      │ (relay/liveness/
                                        │  - runtimeSweeper / autopilotScheduler│  empty-claim)
                                        ▼
                             ╔══════════════════════════╗  one per user machine
                             ║  multica agent DAEMON     ║
                             ║  polls/claims tasks,      ║──exec──► claude / codex / copilot /
                             ║  spawns agent CLIs        ║          gemini / cursor-agent / … (12)
                             ╚══════════════════════════╝
```

The product framing: "Linear, but agents are first-class teammates"
(`README.md:29-64`). Agents get profiles, appear on the board, are assigned
issues, comment, change status, and report blockers like human colleagues. The
key inversion vs. a typical SaaS: **the server runs no agents**.

## Multi-agent runtime model

The daemon is a multiplexer (`daemon.go`):

- `Daemon.Run` (`daemon.go:581-648`) launches background goroutines:
  `workspaceSyncLoop`, `taskWakeupLoop`, `heartbeatLoop`, `gcLoop`,
  `autoUpdateLoop`, `tokenRenewalLoop`, `serveHealth`, and the main `pollLoop`.
- On start it auto-detects installed CLIs on PATH (claude, codex, copilot,
  opencode, openclaw, hermes, gemini, pi, cursor-agent, kimi, kiro-cli, agy) and
  registers **one "runtime" per (CLI × workspace)** (`registerRuntimesForWorkspace`
  `daemon.go:716-765`). A *runtime* = a compute environment advertising which
  CLIs it can run.

Concurrency (`pollLoop` `daemon.go:1842-1920`):

- A buffered slot semaphore `newTaskSlotSemaphore(MaxConcurrentTasks)` (default
  20, `daemon.go:2037-2043`) caps daemon-wide concurrency; the slot index is
  exposed to the agent as `MULTICA_TASK_SLOT` for shared-GPU indexing
  (`daemon.go:2629-2653`).
- One `runRuntimePoller` goroutine **per runtime** (`daemon.go:1945-2032`)
  acquires a slot **before** claiming (`daemon.go:1957-1971`) — deliberately, to
  avoid pushing tasks into server-side `dispatched` and racing the 300s
  dispatch-timeout sweeper (rationale `daemon.go:1922-1944`).
- On claim it spawns a `handleTask` goroutine and loops immediately, so one
  runtime can run several tasks in parallel up to the global cap
  (`daemon.go:2021-2031`).

Session lifecycle (`handleTask` `daemon.go:2099-2238`): `StartTask`
(dispatched→running) → `ReportProgress`; `watchTaskCancellation` polls the
server every 5s, and a `cancelled` status or a 404 (`shouldInterruptAgent`
`daemon.go:2061-2066`) cancels the run ctx → kills the subprocess; `runTask`
(`daemon.go:2451-3025`) prepares an isolated workdir, builds the prompt, spawns
the CLI, drains its stream, reports the result; outcomes map to
`completed` / `blocked` / `cancelled` (`daemon.go:2888-3024`).

Sweep/expiry is **server-side**, not in the daemon
(`server/cmd/server/runtime_sweeper.go`, every 30s):

| Condition | Action |
| --- | --- |
| Runtime no heartbeat > 150s | mark offline (cross-checked vs Redis liveness, `runtime_sweeper.go:91-203`) |
| Offline runtime, no agents > 7 days | delete |
| Task `dispatched` > 300s or `running` > 9000s (2.5h) | fail (`FailStaleTasks` `:242-257`) |
| Task `queued` > 2h | expire in 500-row batches (`:259-280`) |

Heartbeats are per-runtime jittered HTTP goroutines (`runRuntimeHeartbeat`
`daemon.go:1299-…`, default 15s), suppressed when WS acks are fresh
(`daemon.go:534-568`).

## Persistence model

**DB rows, not processes.** Tasks live in `agent_task_queue` (sqlc
`db.AgentTaskQueue`) with a state machine
`queued → dispatched → running → completed|failed|blocked|cancelled` plus
`waiting_local_directory` (`service/task.go`: `CreateAgentTask`, `StartTask`
`:969`, `CompleteTask` `:1020`, `FailTask` `:1198`).

Continuity = **session-resume, not process longevity**:

- Each completed task stores `session_id` + `work_dir`. The next task on the
  same `(agent_id, issue_id)` is handed `PriorSessionID` + `PriorWorkDir` at
  claim; `runTask` reuses the workdir (`execenv.Reuse`, `daemon.go:2550-2559`)
  and passes `ResumeSessionID` so the CLI continues the conversation
  (`daemon.go:2727-2729`; claude `--resume` `claude.go:521-523`).
- If resume fails (no session established), the daemon retries with a fresh
  session (`daemon.go:2844-2856`); `resolveSessionID` (`claude.go:567-572`)
  prevents persisting a bogus id.
- **"Poisoned" session protection**: outputs that would re-break the issue on
  every resume (iteration-limit fallback, API 400 invalid_request) get a
  `failure_reason` so `GetLastTaskSession` excludes them from the resume lookup
  (`daemon.go:2905-2925`, `3009-3023`). A corrupt-context loop cannot trap an
  issue forever.
- **Isolated dirs** under `MULTICA_WORKSPACES_ROOT` (`~/multica_workspaces`),
  GC'd by `gcLoop`: full cleanup of done/cancelled dirs after TTL, orphan
  cleanup (missing `.gc_meta.json`), and artifact-only cleanup
  (`node_modules`/`.next`/`.turbo`) that **preserves source/`.git`/output/logs**
  so a workdir stays resumable (`CLI_AND_DAEMON.md:185-193`; meta written
  `daemon.go:2218-2237`).
- **Orphan recovery on restart**: after re-registering, the daemon calls
  `RecoverOrphans` so the server learns about in-flight tasks the dead process
  held, and issues do not hang at `in_progress` (`daemon.go:481-487`,
  `1206-1214`).

**Autopilot** (the autonomous loop) is **not** an in-agent `while`. A server
goroutine `runAutopilotScheduler` (`server/cmd/server/autopilot_scheduler.go`)
ticks every 30s: `ClaimDueScheduleTriggers` (cron via `ComputeNextRun`) →
`DispatchAutopilot` → `dispatchCreateIssue` manufactures an issue and assigns it
to an agent/squad, which then flows through the normal enqueue path
(`autopilot.go:58-266`). It recovers triggers whose `next_run_at` was lost to a
crash (`autopilot_scheduler.go:34-69`). The per-issue agent run is the unit of
work; re-triggering happens via comments/mentions/child-issue-done events.

## Provider integration

**Backends are driven by `exec`-ing the vendor CLI and parsing its streaming
JSON stdout — not SDK/HTTP calls from Go.** Unified interface in
`server/pkg/agent/agent.go`:

- `Backend.Execute(ctx, prompt, opts) (*Session, error)` returns
  `Session{ Messages <-chan Message, Result <-chan Result }` (`agent.go:16-56`).
  `agent.New(agentType, cfg)` is a factory over 12 backends
  (`agent.go:111-144`): claude, codex, copilot, opencode, openclaw, hermes,
  gemini, pi, cursor, kimi, kiro, antigravity. Comment: "mirrors the happy-cli
  AgentBackend pattern, translated to idiomatic Go" (`agent.go:1-5`).
- Invocation styles differ per CLI (`agent.go:157-170`): claude `stream-json`,
  `codex app-server`, `copilot (json)`, `cursor-agent (stream-json)`, `gemini
  (stream-json)`, hermes/kimi/kiro `acp` (ACP protocol), `opencode run (json)`,
  `pi (json mode)`, `agy -p (print mode)`.

Claude adapter (`server/pkg/agent/claude.go:23-245`):

- `exec.CommandContext("claude", "-p", "--output-format","stream-json",
  "--input-format","stream-json", "--verbose", "--strict-mcp-config",
  "--permission-mode","bypassPermissions", "--disallowedTools","AskUserQuestion",
  …)` (`buildClaudeArgs` `claude.go:489-527`).
- Prompt written to **stdin** as a stream-json frame in its own goroutine to
  avoid a stdout/stdin deadlock (`claude.go:105-118`, `writeClaudeInput`
  `:529-558`).
- A `bufio.Scanner` (10MB buffer) reads stdout line-by-line; assistant / user /
  system / result / log events become unified `Message`s with per-model
  `TokenUsage` (`claude.go:144-242`, `handleAssistant` `:247-287`).
- **Streaming, not one-shot in spirit**: messages flow live;
  `handleControlRequest` (`claude.go:310-346`) auto-approves every tool-use
  ("autonomous/daemon mode"). Protocol-critical flags are blocked from user
  `custom_args` (`claudeBlockedArgs` `:474-487`).
- Per-task auth: each run gets a **task-scoped token** (`MULTICA_TOKEN =
  task.AuthToken`, bound to (agent,task), `daemon.go:2641-2654`) — lateral-
  movement protection. Model selection is two-tier (agent.model →
  `MULTICA_<PROVIDER>_MODEL` → CLI default; never a hardcoded Go default,
  `daemon.go:2740-2755`).

The daemon's `executeAndDrain` (`daemon.go:3029-…`) drains `session.Messages`,
batches text/thinking/tool events every 500ms, forwards them to the server via
`ReportTaskMessages` (powering the live timeline), and runs an **idle watchdog**
that cancels the subprocess if it goes silent while no tool is in-flight
(`daemon.go:3056-3074`).

## Coordination & communication

DB-centric and event-driven; **not a peer-to-peer agent mesh.**

- **Shared task store + claim**: `service/task.go` is the scheduler. Enqueue
  paths: assignment, @mention, squad-leader, child-issue-done, chat,
  quick-create, autopilot (`EnqueueTaskForIssue` `:392`, `…ForMention` `:459`,
  `…ForSquadLeader` `:469`). Each task pins `runtime_id = agent.runtime_id`; the
  daemon's `ClaimTaskForRuntime` (`:839-943`) atomically claims via Postgres
  (`ClaimTask` row-locks, reclaims stale dispatched, uses a Redis "empty-claim"
  cache to skip empty scans).
- **Two event systems** (`server/internal/events/bus.go` + two hubs in
  `main.go:164-167`): `events.Bus` in-proc pub/sub feeds `realtime.Hub` (fans WS
  to browsers — WS events invalidate React Query, never write stores); a
  separate `daemonws.Hub` pushes **task-available wakeups to daemons** so they
  claim instantly instead of waiting on the poll. `notifyTaskAvailable`
  (`task.go:1766-1782`) bumps the empty-claim version *then* sends the wakeup
  (ordering matters) via `RelayNotifier`, which also publishes through Redis so
  any API node can deliver (`daemonws/notifier.go:23-49`).
- **Agent ↔ platform protocol = the `multica` CLI.** The spawned agent has the
  CLI on PATH plus a token; the runtime brief instructs it to drive the platform
  with `multica issue get/comment add/status/metadata set`, `multica repo
  checkout`, etc. (`server/internal/daemon/prompt.go:33-34,157-158`). Status
  transitions are made **by the agent calling the CLI**, not by the daemon.
  Cross-agent comms is literally posting comments and @mentions, which the
  server turns into new tasks.
- **Squads = stable routing via a leader agent.** Work assigned to a squad
  resolves to its leader; the server prepends a hard-coded Squad Operating
  Protocol (`server/internal/handler/squad_briefing.go:19-89`). The leader reads
  the issue, picks the best member, **delegates by @mention** using the exact
  `mention://<type>/<UUID>` markdown form from the rendered roster (`:104-218`),
  records an evaluation, then **stops and exits** — re-triggered when a delegated
  member replies (`comment.go:1165`). A mailbox/blackboard model (issue =
  blackboard, comments = mailbox, leader = router), not direct RPC.

## What we can learn

Ideas worth borrowing into our harness:

- **Session-resume as the persistence primitive** — store `(session_id,
  work_dir)`, replay via CLI `--resume`, instead of keeping processes alive.
  Cheap, restart-safe, and yields free orphan recovery.
- **"Poisoned session" classification** — tag failures that would re-break on
  resume and exclude them from the resume lookup (`daemon.go:2905-2925`,
  `3009-3023`) so a corrupt-context loop cannot trap a goal forever.
- **Slot-before-claim semaphore** + a separate server-side dispatch timeout —
  prevents a silent backlog/timeout race (`daemon.go:1922-1944`).
- **WS wakeup + empty-claim version cache** — instant dispatch with a poll
  fallback, version invalidated *before* the wakeup to close the enqueue/claim
  race (`task.go:1759-1782`).
- **Squad-leader-as-router** via a fixed protocol + rendered roster — prompt-only
  delegation (terse @mention, record-evaluation, stop-and-wait-for-retrigger)
  that scales routing without a DAG engine (`squad_briefing.go`).
- **Idle watchdog that distinguishes "silent" from "legit long tool call"** by
  tracking in-flight tool count (`daemon.go:3056-3074`) — avoids killing agents
  mid `npm install`/`docker build`.
- **GC that preserves `source/.git/output/logs` but reaps
  `node_modules/.next/.turbo`** so resumable workdirs survive while disk stays
  bounded (`CLI_AND_DAEMON.md:185-193`).

The structural takeaway: Multica reaches the same exec-stream + session-resume
substrate as our ADR-0018 direction, but adds a DB source of truth, an
edge-daemon executor, and heavy operational hardening (sweeper, slot semaphore,
poisoned-session, idle watchdog). The decision doc
([runtime-persistence-decision.md](runtime-persistence-decision.md)) judges which
of these we should adopt and in what order.
