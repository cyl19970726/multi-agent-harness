# Multica — Architecture & Data-Flow Diagrams

Diagram companion to [multica-architecture.md](multica-architecture.md). Prose
lives there; this file is ASCII diagrams + legends + source citations only. All
paths cite the read-only checkout (`.research-cache/multica`, HEAD `2bb2d13e`);
paths are verbatim (`server/internal/daemon/daemon.go:1842`, etc.) so each claim
is checkable.

Diagrams:

- [(a) Deployment / component](#a-deployment--component-diagram)
- [(b) Agent task lifecycle state machine](#b-agent-task-lifecycle-state-machine)
- [(c) End-to-end data flow](#c-end-to-end-data-flow-sequence)
- [(d) Concurrency / slot model](#d-concurrency--slot-model)
- [(e) Resume data flow](#e-resume-data-flow-turn-n--turn-n1)

---

## (a) Deployment / component diagram

```text
 ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
 │ Web (Next16)│ │ Desktop     │ │ Mobile      │     CLIENT TIER
 │ apps/web/   │ │ (Electron)  │ │ (Expo/RN)   │
 └──────┬──────┘ └──────┬──────┘ └──────┬──────┘
        │  HTTP (REST)  │               │
        └───────┬───────┴───────────────┘  WS realtime: GET /ws  (realtime.HandleWebSocket)
                ▼
 ┌──────────────────────────────────────────────────────────────────────────┐
 │ API SERVER  (Go: Chi router + sqlc + gorilla/websocket)  :8080             │  server/cmd/server/main.go
 │  cmd/server/router.go                                                      │
 │  ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────┐ │
 │  │ realtime.Hub         │  │ daemonws.Hub         │  │ events.New() bus │ │  two distinct hubs
 │  │ (browser WS rooms)   │  │ (daemon wakeups)     │  │ (in-proc pub/sub)│ │  main.go:164-167
 │  └──────────────────────┘  └──────────────────────┘  └──────────────────┘ │
 │  background goroutines (main.go:316-320):                                  │
 │   runRuntimeSweeper · heartbeatScheduler · runAutopilotScheduler ·         │
 │   runAutopilotFailureMonitor · runDBStatsLogger · hub.Run()                │
 └───┬──────────────────────────────────────────────┬─────────────────┬──────┘
     │ sqlc generated queries (DB)                   │ WS wakeup       │ optional
     ▼ server/pkg/db/generated/                      │ GET /api/        │ Redis relay
 ┌──────────────────────────────┐                    │ daemon/ws        │ (realtime.New
 │ Postgres 17 + pgvector        │                    │ router.go:292    │  ShardedStream
 │ SOLE source of truth          │                    │                  │  Relay, multi-node)
 │ server/migrations/            │                    │                  ▼
 └──────────────────────────────┘                    │          ┌──────────────┐
                                                      │          │ Redis (opt.) │ liveness +
                                                      │          │              │ empty-claim
                                                      │          └──────────────┘ cache
 ┌────────────────────────────────────────────────────┼───────────────────────────────┐
 │ multica DAEMON  (same Go binary, `daemon` subcmd)   │  server/internal/daemon/       │  on user's machine
 │  goroutines (Daemon.Run): workspaceSyncLoop ·       │                                │
 │   taskWakeupLoop ◄──── holds WS to /api/daemon/ws ──┘  HTTP for all lifecycle calls: │
 │   heartbeatLoop · gcLoop · autoUpdateLoop ·            claim/start/complete/fail/    │
 │   tokenRenewalLoop · serveHealth · pollLoop           progress/session/recover      │
 │                                                       (client.go → API server)       │
 │  ┌──────────────────────────────────────────────────────────────────────────────┐  │
 │  │ per-task goroutine (handleTask) → runTask → agent.Backend.Execute()            │  │
 │  └───────────────────────────────────────┬──────────────────────────────────────┘  │
 └──────────────────────────────────────────┼─────────────────────────────────────────┘
                                             │ os/exec (process exec), parse stream JSON stdout
                                             ▼
            ┌──────────────────────────────────────────────────────────────────────┐
            │ PROVIDER CLI SUBPROCESSES (12 backends behind agent.Backend)           │
            │  claude · codex · copilot · opencode · openclaw · hermes · gemini ·    │
            │  pi · cursor · kimi · kiro · antigravity   (agent.go:117-141)          │
            │   └─ spawned with task-scoped env: MULTICA_TOKEN, MULTICA_SERVER_URL,  │
            │      MULTICA_WORKSPACE_ID, MULTICA_TASK_SLOT (daemon.go:2646-2653)     │
            │   └─ the `multica` CLI on PATH calls API server back over HTTP ────────┼──► API SERVER
            └──────────────────────────────────────────────────────────────────────┘
```

**Legend.** Boxes = processes/datastores; edges labeled by protocol —
**HTTP** (REST), **WS** (websocket), **DB** (sqlc), **exec** (os/exec
subprocess). The CLI and daemon are one Go binary; the daemon runs on the
user's machine and is the only executor (server holds no agent compute). Redis
is optional: absent ⇒ single-node in-memory hub. Spawned provider CLIs phone
home to the API server via the `multica` CLI using a task-scoped token.

**Sources.** Entries `server/cmd/server/main.go`,
`server/cmd/multica/main.go:73`; WS routes `server/cmd/server/router.go:242,:292`;
two hubs `main.go:164-167`; server goroutines `main.go:316-320`; daemon WS URL
`server/internal/daemon/wakeup.go:301-325`; env injection `daemon.go:2646-2653`;
Redis `main.go:197-244,:215-225`; 12 backends
`server/pkg/agent/agent.go:117-141`.

---

## (b) Agent task lifecycle state machine

```text
  table agent_task_queue · status CHECK (migration 109 = 7 states; 001 had 6)
  server/migrations/001_init.up.sql:127-140 + 109_agent_task_waiting_local_directory.up.sql

       enqueue (service/task.go: EnqueueTaskForIssue/…ForMention/…ForSquadLeader/
                EnqueueQuickCreateTask/EnqueueChatTask/autopilot) · pins runtime_id
                                  │
                                  ▼
                            ┌──────────┐  ExpireStaleQueuedTasks (sweeper, queuedTTL=7200s)
                            │  QUEUED  │──────────────► failure_reason='queued_expired'
                            └────┬─────┘                            │
       daemon: slot acquired     │ ClaimAgentTask                   │
       BEFORE claim (sem)        │ UPDATE status='dispatched'       │
       daemon.go:1957-1971       │ FOR UPDATE SKIP LOCKED           │
                                 ▼ agent.sql.go:517-545             │
                          ┌─────────────┐                           │
                          │ DISPATCHED  │── dispatchTimeout=300s ──►│
                          └──┬───────┬──┘   FailStaleTasks          │
        daemon StartTask     │       │ claimed local_directory      │
        TaskService.StartTask│       │ path mutex busy:             │
        task.go:969          │       │ MarkTaskWaitingLocalDirectory│
                             ▼       ▼ task.go:993                  │
                      ┌──────────┐ ┌───────────────────────┐        │
                      │ RUNNING  │ │ WAITING_LOCAL_DIRECTORY│       │
                      └──┬────┬──┘ └───────────┬───────────┘        │
   running              │    │ got path lock   │ (sweeper EXCLUDES  │
   Timeout=9000s        │    │◄────────────────┘  this state)       │
   FailStaleTasks       │    │                                      │
   (timeout) ───────────┼────┤                                      ▼
                        │    │ CompleteTask (task.go:1020)   ┌────────────┐
  FailAgentTask         │    └──────────────────────────────►│ COMPLETED  │ stores
  WHERE status IN       │                                    └────────────┘ session_id+work_dir
  (dispatched,running,  │
   waiting_local_dir)   ▼
  failure_reason=       ┌──────────┐  poisoned classify (poisoned.go):
  agent_error (default) │  FAILED  │  iteration_limit · agent_fallback_message ·
                        └────┬─────┘  api_invalid_request · codex_semantic_inactivity
                             │            (these EXCLUDED from resume lookup §e)
        MaybeRetryFailedTask │ (task.go:1352) if attempt<max_attempts:
        spawns CHILD task ───┘ parent_task_id back-pointer → new QUEUED row

  ── orthogonal terminal ──────────────────────────────────────────────────────────
   any non-terminal ──CancelAgentTask──► CANCELLED  (WHERE status IN queued/dispatched/
                                                      running/waiting_local_directory)
   runtime offline ──FailTasksForOfflineRuntimes──► FAILED  (sweeper, after MarkRuntimes
                                                      OfflineByIDs; staleThreshold=150s)
   daemon restart  ──RecoverOrphanedTasksForRuntime──► FAILED failure_reason=runtime_recovery
                                                      (dispatched/running/waiting → failed)
```

**Legend.** Boxes = the 7 DB statuses. Edge labels name the trigger + the exact
mutation site. Three actors drive transitions: the **daemon** (claim/start/
complete/fail), the **runtime_sweeper** (timeouts, queued-expiry, offline-runtime
fail, orphan recovery), and **provider exit** (success→complete, error→fail with
a classified `failure_reason`). "Poisoned" is not a status — it is a set of
`failure_reason` values that make a FAILED task ineligible for resume.

**Sources.** Status enum `migrations/001_init.up.sql:127-140`,
`migrations/109_…up.sql`; claim `agent.sql.go:517-545`; start/complete/wait
`task.go:969,:993,:1020`; sweeper thresholds `server/cmd/server/runtime_sweeper.go`
(staleThreshold=150, dispatchTimeout=300, runningTimeout=9000, queuedTTL=7200,
offlineTTL=604800; FailTasksForOfflineRuntimes `:150`); poisoned
`server/internal/daemon/poisoned.go`; retry `task.go:1352,:1534` +
`migrations/055_task_lease_and_retry.up.sql`; orphan `agent.sql`
`RecoverOrphanedTasksForRuntime`; slot-before-claim `daemon.go:1922-1944`.

---

## (c) End-to-end data flow (sequence)

```text
 Client/@mention/autopilot   API SERVER (svc + bus)   Postgres   daemonws.Hub   DAEMON   Provider CLI   Web(WS)
        │                          │                     │            │           │           │           │
 (1)  enqueue ───────────────────► │ INSERT agent_task_queue          │           │           │           │
        │                          │ status='queued',────►│ row        │           │           │           │
        │                          │ runtime_id pinned    │            │           │           │           │
 (2)  NotifyTaskEnqueued           │ notifyTaskAvailable: │            │           │           │           │
        │                          │  bump empty-claim VER FIRST,      │           │           │           │
        │                          │  then Wakeup.Notify ─────────────►│ frame     │           │           │
        │                          │  (RelayNotifier: local + Redis)   │ daemon:   │           │           │
        │                          │                      │            │ task_     │           │           │
 (3)    │                          │                      │            │ available ───────────►│ taskWakeupLoop
        │                          │                      │            │           │ signals taskWakeups
        │                          │                      │            │           │ pollLoop fans to
        │                          │                      │            │           │ runRuntimePoller
 (4)    │                          │                      │            │  slot acquire (sem) THEN
        │                          │ POST /api/daemon/runtimes/{rid}/tasks/claim ◄─│ ClaimTask
        │                          │ ClaimTaskForRuntime (task.go:839-943):        │           │           │
        │                          │  a ReclaimStaleDispatched (FOR UPDATE SKIP LOCKED)          │           │
        │                          │  b Redis empty-claim IsEmpty short-circuit    │           │           │
 (5)    │                          │  c sample version                             │           │           │
        │                          │  d ListQueuedClaimCandidatesByRuntime         │           │           │
        │                          │  e ClaimAgentTask: UPDATE status='dispatched' │           │           │
        │                          │    WHERE id=(SELECT … FOR UPDATE SKIP LOCKED   │           │           │
        │                          │    LIMIT 1)  ── single atomic UPDATE ─────────►│ dispatched│           │
 (6)    │                          │ return Task{PriorSessionID,PriorWorkDir}       │ (from     │           │
        │                          │  (resolved via GetLastTaskSession) ───────────►│ GetLastTaskSession)   │
 (7)    │                          │ POST /api/daemon/tasks/{id}/start ◄────────────│ StartTask │           │
        │                          │ status running ──────►│ running    │           │ watchTaskCancellation
        │                          │                      │            │           │ polls /status every 5s
 (8)    │                          │                      │            │           │ runTask: execenv.Reuse/
        │                          │                      │            │           │ Prepare; agent.New ──────► exec.CommandContext
        │                          │                      │            │           │           │ ("claude" -p
        │                          │                      │            │           │           │ stream-json …
        │                          │                      │            │           │           │ [--resume sid])
        │                          │                      │            │           │ prompt → stdin frame
 (9)    │                          │                      │            │           │◄── stdout: system(early sid)/
        │                          │ POST .../session (PinTaskSession) ◄────────────│ result(final sid+usage)
        │                          │ UpdateAgentTaskSession─►│ sid pinned│           │           │           │
 (10)   │                          │ POST .../messages (batch ~500ms) ◄─────────────│ executeAndDrain
        │                          │ POST .../progress ◄────────────────────────────│ ReportProgress
 (11)   │                          │ POST .../complete (sid,work_dir,branch) ◄───────│ CompleteTask
        │                          │ CompleteTask ────────►│ completed  │           │ (or FailTask + reason)
 (12)   │                          │ publish on events.Bus (in-proc sync pub/sub):   │           │           │
        │                          │  registerSubscriber → registerActivity(writes   │           │           │
        │                          │  activity_log) → registerNotification           │           │           │
        │                          │  registerListeners bridges bus→realtime.Broadcaster ──────────────────► browser WS rooms
        │                          │  (daemonws.Hub is the SECOND, separate event system: task-available frames)
```

**Legend.** Numbered arrows = ordered steps; the DB (Postgres) and the
subprocess (Provider CLI) are drawn as explicit columns. The claim (step 5) is
a **single atomic `UPDATE … FOR UPDATE SKIP LOCKED`** — not an advisory lock,
not a multi-statement transaction. Slot is acquired (step 4) **before** the
claim to avoid stranding `dispatched` rows. Two event systems fan out: the
in-proc `events.Bus` → browser WS, and the separate `daemonws.Hub` wakeups →
daemons.

**Sources.** enqueue `service/task.go`; wakeup ordering `task.go:1754-1782`,
`daemonws/notifier.go:23-49`; daemon wakeup `wakeup.go:258-292`, pollLoop
`daemon.go:1842`; slot+claim `daemon.go:1957-1971,:1986`, client `client.go:156-164`;
ClaimTaskForRuntime `task.go:839-943`, atomic claim `agent.sql.go:517-545`,
candidates index `migrations/067`; start `daemon.go:2099,:2061`; spawn
`daemon.go:2451,:2711,:2788-2798`, claude args `claude.go:489-527,:62,:105-118`;
stream parse `claude.go:141,:160-164,:165`; report `executeAndDrain`,
complete `task.go:1020`; event systems `main.go:255-257`, `listeners.go`,
`activity_listeners.go`, `notification_listeners.go`.

---

## (d) Concurrency / slot model

```text
 ┌──────────────────────────────── multica DAEMON (one process) ───────────────────────────────────┐
 │                                                                                                   │
 │   pollLoop (daemon.go:1842-1920)  ── one goroutine PER registered runtime ──                     │
 │   ┌───────────────────┐  ┌───────────────────┐  ┌───────────────────┐                            │
 │   │ runRuntimePoller A │  │ runRuntimePoller B │  │ runRuntimePoller C │  (slow 30s claim on one  │
 │   └─────────┬─────────┘  └─────────┬─────────┘  └─────────┬─────────┘   can't stall others MUL-1744)
 │             │ acquire slot          │                     │                                       │
 │             ▼                       ▼                     ▼                                       │
 │   ┌──────────────────────────────────────────────────────────────────────────────────────────┐  │
 │   │ taskSlotSemaphore = buffered channel of int indices [0,n)  (newTaskSlotSemaphore,          │  │
 │   │  daemon.go:2037-2043)   receive=acquire · send-back=release · caps DAEMON-WIDE concurrency │  │
 │   │  default MaxConcurrentTasks≈20 · slot index → MULTICA_TASK_SLOT env (daemon.go:2653)       │  │
 │   │   [slot0][slot1][slot2] … [slotN-1]                                                        │  │
 │   └───────┬────────────┬───────────────┬─────────────────────────────────────────────────────┘  │
 │           │ slot0       │ slot1          │ slotN-1                                                 │
 │           ▼             ▼                ▼                                                         │
 │   ┌────────────┐ ┌────────────┐ ┌────────────┐   one goroutine PER claimed task                  │
 │   │ handleTask │ │ handleTask │ │ handleTask │   (daemon.go:2023-2029); poller loops immediately  │
 │   │  → runTask │ │  → runTask │ │  → runTask │   to claim more                                    │
 │   └─────┬──────┘ └─────┬──────┘ └─────┬──────┘                                                    │
 │         ▼              ▼              ▼                                                            │
 │   ┌──────────────────────────────────────────────────────────────────────────────────────────┐  │
 │   │ agent.Backend interface: Execute(ctx, prompt, opts) (*Session, error)  (agent.go:16-21)     │  │
 │   │ Session{ Messages <-chan Message, Result <-chan Result }  · factory agent.New (agent.go:111)│  │
 │   │ ┌────────┬────────┬────────┬──────────┬──────────┬────────┬────────┬─────┬───────┬─────┬────┐│  │
 │   │ │ claude │ codex  │copilot │ opencode │ openclaw │ hermes │ gemini │ pi  │cursor │kimi │kiro││  │ + antigravity
 │   │ │stream- │app-    │ json   │ run json │   acp    │  acp   │stream- │json │stream-│ acp │ acp││  │ (agy -p)
 │   │ │ json   │server  │        │          │          │        │ json   │mode │ json  │     │    ││  │
 │   │ └────────┴────────┴────────┴──────────┴──────────┴────────┴────────┴─────┴───────┴─────┴────┘│  │
 │   │  claudeBackend.Execute (claude.go:23-245): 3 goroutines (stdin writer, stdout scanner, wait) │  │
 │   │  handleControlRequest auto-approves every tool-use (claude.go:310-346)                       │  │
 │   └──────────────────────────────────────────────────────────────────────────────────────────┘  │
 └────────────────────────────────────────────────────────────────────────────────────────────────┘
              ▲ (server side, separate process) runtime_sweeper every 30s reclaims/fails/expires/GCs
              └── server/cmd/server/runtime_sweeper.go  (see diagram b for thresholds)
```

**Legend.** Two concurrency axes: goroutine-**per-runtime** poller (isolation
between runtimes) and goroutine-**per-task** handler (parallel execution). The
single limiter is the slot semaphore — a buffered channel of int indices,
capping concurrency daemon-wide; the slot index is exposed as
`MULTICA_TASK_SLOT`. All 12 providers sit behind one `agent.Backend.Execute`
returning `Session` channels; the server-side `runtime_sweeper` runs out-of-band.

**Sources.** pollLoop / per-runtime `daemon.go:1842-1920`; per-task goroutine
`daemon.go:2023-2029`; semaphore `daemon.go:2037-2043`, slot env `:2653`;
interface + Session `server/pkg/agent/agent.go:16-21,:50-56`; factory + roster
`agent.go:111-144,:117-141`; launch styles `agent.go:157-170`; claude backend
`claude.go:23-245,:310-346,:474-487`; sweeper `server/cmd/server/runtime_sweeper.go`.

---

## (e) Resume data flow (turn N → turn N+1)

```text
  ── TURN N (writes the resume key) ───────────────────────────────────────────────
  DAEMON runTask                       Provider CLI (claude)        API SERVER → Postgres
     │ execenv.Prepare (fresh workdir      │                            │
     │ under MULTICA_WORKSPACES_ROOT)       │                            │
     │ exec.CommandContext claude -p … ────►│                            │
     │                                      │ stdout system → early sid  │
     │ PinTaskSession ─────────────────────────────────────────────────►│ UpdateAgentTaskSession
     │                                      │ stdout result → final sid  │ COALESCE-merge,
     │ CompleteTask(sid, work_dir, branch) ────────────────────────────►│ WHERE status IN
     │                                      │                            │ (dispatched,running)
     │                                      │                            ▼
     │                                      │            agent_task_queue row:
     │                                      │            session_id = "sess-abc"   (migrations/020)
     │                                      │            work_dir   = "/…/ws-7"
     │                                      │            status     = completed (or failed-non-poisoned)

  ── TURN N+1 (reuses the resume key) ──────────────────────────────────────────────
  next enqueue for SAME (agent_id, issue_id) → QUEUED → claim
     │                                                          │
     │ ClaimTaskForRuntime resolves resume via GetLastTaskSession (agent.sql.go:1340-1404):
     │   SELECT session_id, work_dir, runtime_id
     │   WHERE agent_id=$1 AND issue_id=$2
     │     AND (status='completed'
     │          OR (status='failed'
     │              AND failure_reason NOT IN {iteration_limit, agent_fallback_message,
     │                  api_invalid_request, codex_semantic_inactivity}  ◄── poisoned filter
     │              AND error NOT ILIKE '%400%'+'%invalid_request_error%'))
     │     AND session_id IS NOT NULL
     │   ORDER BY COALESCE(completed_at,started_at,dispatched_at,created_at) DESC LIMIT 1
     │
     ▼ Task carries PriorSessionID="sess-abc", PriorWorkDir="/…/ws-7"
  runTask:
     │  execenv.Reuse(ReuseParams{WorkDir: PriorWorkDir})  ◄── REUSE workdir, not Prepare
     │  (daemon.go:2550-2559)                                   (GC kept source/.git/output/logs)
     │  execOpts.ResumeSessionID = PriorSessionID
     │       └─ claude turns it into  --resume sess-abc  (buildClaudeArgs claude.go)
     ▼
  if result.Status=="failed" && PriorSessionID!="" && result.SessionID=="":
     └─ retry ONCE with a FRESH session (daemon.go:2842-2856)  ── resume produced nothing
```

**Legend.** The resume key is the `(session_id, work_dir)` pair stored on the
`agent_task_queue` row. Turn N pins it mid-flight (`PinTaskSession`, so a crash
still leaves a usable pointer) and finalizes it on complete. Turn N+1's claim
resolves it via `GetLastTaskSession` — **failed tasks are eligible** (a crash
may have established a real session) **except poisoned reasons**. The workdir is
reused (`execenv.Reuse`) and the session id becomes `--resume`; if resume yields
no session, the daemon retries once fresh.

**Sources.** session/work_dir columns `server/migrations/020_task_session.up.sql`;
resume SQL `GetLastTaskSession` `agent.sql` (`agent.sql.go:1340-1404`); poisoned
set `server/internal/daemon/poisoned.go`; mid-flight pin `UpdateAgentTaskSession`;
workdir reuse `daemon.go:2550-2559`; `--resume` `claude.go` buildClaudeArgs;
fresh-session retry `daemon.go:2842-2856`; runtime model
`migrations/004_agent_runtime_loop.up.sql`.
