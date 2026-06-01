# Claude Code Agent Teams — Architecture & Data-Flow Diagrams

> **Correction note (2026-06-01).** These diagrams depict the **in-process
> teammate mode only**. They state `tmuxPaneId:""` and "NO subprocess, NO pty,
> NO tmux backend" (diagram (a) line ~38, legends ~68-75, ~220-232) — that is
> true *only* for the in-process branch. Claude Code v2.1.88 also has a real
> **tmux / iTerm2 split-pane mode** that spawns a **separate `claude` process per
> pane** with a real `tmuxPaneId` (e.g. `%3`). For the definitive
> mode/process-model answer and the selection logic, see the
> [Correction / Modes](claude-code-agent-teams.md#correction--modes-definitive--supersedes-the-original-in-process-only-answer)
> section of the prose doc. The diagrams below are accurate for the in-process
> mode and are retained for that purpose.

Diagram companion to [claude-code-agent-teams.md](claude-code-agent-teams.md).
Prose lives there; this file is ASCII diagrams + legends + source citations
only. All paths cite the read-only Claude Code source checkout
(`/Users/hhh0x/claude-code-source/src/…`); paths are verbatim so each claim is
checkable.

Diagrams:

- [(a) Component diagram](#a-component-diagram--task-types-team-registry-tools-mailbox)
- [(b) Teammate lifecycle state machine](#b-teammate-lifecycle-state-machine)
- [(c) Inter-agent message sequence](#c-inter-agent-message-sequence-sendmessage--mailbox--poll)
- [(d) Concurrency model](#d-concurrency-model--one-node-process-n-async-loops)
- [(e) Persistent vs ephemeral across restart](#e-persistent-vs-ephemeral-across-host-restart)

---

## (a) Component diagram — task types, team registry, tools, mailbox

```text
                     ┌───────────────────────── ONE node.js PROCESS (single event loop) ───────────────────────────┐
                     │                                                                                               │
                     │   ┌───────────────────┐  creates/owns   ┌──────────────────────────────────────────────┐    │
                     │   │  LEAD SESSION      │────────────────►│  AppState  (in-memory, ephemeral)            │    │
                     │   │ (LocalMainSession  │                 │                                              │    │
                     │   │  Task)             │   reads/writes  │  appState.tasks[taskId]:                     │    │
                     │   │  leadAgentId       │◄───────────────►│   ├─ InProcessTeammateTask  ◄── team member  │    │
                     │   └───────┬───────────┘                  │   ├─ LocalAgentTask         (one-shot+resume)│    │
                     │           │ invokes tools                │   ├─ RemoteAgentTask        (server-side)    │    │
                     │           │                              │   ├─ LocalShellTask         (pty child proc) │    │
                     │   ┌───────▼───────────┐  spawns          │   ├─ LocalMainSessionTask   (the lead)       │    │
                     │   │ TeamCreateTool    │─────────────────►│   └─ DreamTask              (memory consol.) │    │
                     │   │ TeamDeleteTool    │  init registry   │                                              │    │
                     │   └───────┬───────────┘                  │  appState.teamContext:                       │    │
                     │           │                              │   teamName, teamFilePath, leadAgentId,       │    │
                     │           │ spawnInProcessTeammate()     │   teammates{agentId→{name,color,             │    │
                     │           ▼                              │     tmuxPaneId:"" , cwd, spawnedAt}}         │    │
                     │   ┌─────────────────────────────┐ owns   └──────────────────────────────────────────────┘    │
                     │   │ InProcessTeammateTask        │            ▲ registers task state                          │
                     │   │  identity, status,           │────────────┘                                               │
                     │   │  isIdle, abortController,     │  AsyncLocalStorage: per-teammate identity                  │
                     │   │  messages[] (cap 50),         │  (agentName/teamName/color) injected by                    │
                     │   │  pendingUserMessages[]        │  runWithTeammateContext()                                  │
                     │   │  ── resident loop:            │                                                            │
                     │   │     runInProcessTeammate()    │  messages-via-file  ┌─────────────────────────────┐        │
                     │   │     waitForNextPromptOrS...() │◄───────────────────►│ SendMessage tool            │        │
                     │   └───────────┬─────────────────┘   poll/read,write    └──────────────┬──────────────┘        │
                     │               │ idle notif → mailbox                                   │ writeToMailbox()      │
                     └───────────────┼────────────────────────── process boundary ───────────┼───────────────────────┘
                                     │ persists-to                                            │ persists-to
                                     ▼                                                        ▼
        ┌────────────────────────────────────── DISK  ~/.claude/ (persistent) ──────────────────────────────────────┐
        │  teams/{team}/config.json   teams/{team}/inboxes/{agentName}.json   tasks/{taskListId}/tasks.json          │
        │  (TeamFile: members[],      (TeammateMessage[]: from,text,read,     (shared task list: id,subject,         │
        │   leadAgentId,leadSessionId) color,summary  ── the FILE MAILBOX)     status,owner,blockedBy)               │
        └─────────────────────────────────────────────────────────────────────────────────────────────────────────┘
                                     ▲ syncs (separate system, NOT teammate persistence)
                     ┌───────────────┴───────────────┐
                     │ teamMemorySync                 │──HTTP upsert──► server API (org-wide knowledge, per repo_hash)
                     │ ~/.claude/team-memory/{hash}/  │
                     └────────────────────────────────┘
```

**Legend.** Solid boxes = modules; the outer frame = the single OS process /
event loop. Edge labels: *creates/owns* (lifetime), *spawns* (instantiates),
*messages-via-file* (no in-memory channel — disk mailbox), *persists-to* (disk
write), *syncs* (separate server upload). `tmuxPaneId:""` is always empty for
in-process teammates — there is no pty/tmux backend. teamMemorySync is an
org-knowledge uploader, explicitly **not** teammate persistence.

**Sources.** Task registry `src/tasks/InProcessTeammateTask/types.ts:22-76`;
spawn `src/utils/swarm/spawnInProcess.ts:104-216`; team context init
`src/tools/TeamCreateTool/TeamCreateTool.ts:193-211`; team file schema +
`tmuxPaneId:""` `src/utils/swarm/teamHelpers.ts:64-90,:81`; mailbox
`src/utils/swarm/teammateMailbox.ts:56-66,:134-192`; teamMemorySync
`src/services/teamMemorySync/index.ts:1-25`.

---

## (b) Teammate lifecycle state machine

```text
                 spawnInProcessTeammate()
                 status='running', isIdle=false
                 void runInProcessTeammate() (fire-and-forget)
                          │
                          ▼
              ┌───────────────────────┐   runAgent() yields ContentBlockParam
              │       RUNNING          │   (text / tool_use / tool_result),
              │   (executing one turn) │   appendTeammateMessage() → messages[] (cap 50)
              └───────────┬───────────┘
                          │ turn loop finishes (no abort)
                          │ updateTaskState(isIdle=true)
                          │ sendIdleNotification() → writeToMailbox(LEAD, idleMsg)
                          ▼
              ┌───────────────────────┐
              │        IDLE            │  status STILL 'running', isIdle=true
              │  waitForNextPromptOr   │  500ms poll cycle:
              │  Shutdown() polling    │    1 pendingUserMessages[] (UI)
              │                        │    2 mailbox: shutdown_req > LEAD msg > FIFO
              │                        │    3 shared task list: tryClaimNextTask()
              └───┬───────────┬────────┘
   new_message    │           │   shutdown_request (NOT auto-approved)
   isIdle=false   │           │   currentPrompt = shutdownMsg → model decides
   loop back to   │           │
   RUNNING ───────┘           ▼
                   ┌──────────────────────────┐  model approves → shouldExit=true
                   │ model decides on shutdown │──── declines ──► back to RUNNING
                   └───────────┬──────────────┘
                               │ approves
        abort signal           ▼
   killInProcessTeammate()  ┌──────────────┐        graceful loop exit
   abortController.abort()  │  shouldExit  │        (resident loop never self-exits
   ──────────────────────►  │   = true     │         except via shutdown/abort)
                            └──────┬───────┘
                                   ▼
        ┌──────────────┐                       ┌────────────────┐
        │   KILLED      │  abort path:          │   COMPLETED    │  clean exit:
        │ status=killed │  remove member from   │ status=        │  emitTaskTerminatedSdk()
        │ AbortController│ team file config.json│  'completed'   │
        └──────────────┘                       └────────────────┘
```

**Legend.** Three terminal-ish states: RUNNING ↔ IDLE cycle within a session;
KILLED via abort; COMPLETED via graceful loop exit. `requestTeammateShutdown()`
only sets a UI flag (`shutdownRequested=true`); actual termination is either
the model approving a shutdown_request or `killInProcessTeammate()` firing the
abort. The resident loop polls every 500ms while IDLE and never self-exits.

**Sources.** Resident loop `src/utils/swarm/inProcessRunner.ts:883-1552`; poll
`:689-868`; kill/shutdown `src/tasks/InProcessTeammateTask/InProcessTeammateTask.tsx:24-44`;
status field + message cap `src/tasks/InProcessTeammateTask/types.ts:22-76,:101`.

---

## (c) Inter-agent message sequence (SendMessage → mailbox → poll)

```text
 LEAD (or A)        SendMessage tool      DISK mailbox file              Teammate B resident loop
    │                     │          ~/.claude/teams/{team}/inboxes/           │ (in waitForNextPrompt
    │                     │                /{sanitize(B)}.json                 │  OrShutdown, IDLE)
    │  (1) SendMessage    │                     │                             │
    │  {to:B,message,     │                     │                             │
    │   summary}          │                     │                             │
    │────────────────────►│                     │                             │
    │                     │ (2) writeToMailbox(B, {from:A,text,ts,color,       │
    │                     │      summary}, team)                              │
    │                     │     ── acquire {inbox}.lock (proper-lockfile,      │
    │                     │        5–100ms backoff) ──                        │
    │                     │────────────────────►│ read[] → push{...,read:false}│
    │                     │                     │ writeFile(JSON) ; release    │
    │                     │◄────────────────────│ lock                         │
    │                     │                     │                             │
    │                     │                     │   (3) 500ms poll tick:       │
    │                     │                     │       readMailbox(B,team)     │
    │                     │                     │◄────────────────────────────│
    │                     │                     │   scan unread; priority:     │
    │                     │                     │   shutdown_req > from LEAD >  │
    │                     │                     │   first-unread FIFO          │
    │                     │                     │   (4) markMessageAsReadBy     │
    │                     │                     │       Index() (lock+write)    │
    │                     │                     │◄────────────────────────────│
    │                     │                     │   (5) return {type:           │
    │                     │                     │   'new_message',from:A,text}  │
    │                     │                     │      → currentPrompt          │
    │                     │                     │   (6) classify text:          │
    │                     │                     │   isShutdownRequest? →model    │
    │                     │                     │   isPermissionResponse? →cb    │
    │                     │                     │   isStructuredProtocolMsg? │   │
    │                     │                     │   else → <teammate-message>XML│
    │                     │                     │   (7) next runAgent() turn    │
    │                     │                     │       consumes as user input  │
    │  (8) reply: B sends back via SendMessage to A — same write→poll→read path │
    │◄═══════════════════════════════════════════════════════════════════════ │
    │  LEAD's own waitForNextPromptOrShutdown() poll reads B's reply (steps 2–7)│
```

**Legend.** Numbered arrows = ordered steps. There is **no push/notify**: B
discovers A's message only by polling its own inbox file every ~500ms; delivery
latency = poll interval + lock backoff. Every write/read is guarded by a
per-inbox `proper-lockfile` for safe concurrent access. The reply (8) is the
exact same mechanism in reverse — the lead is itself polling its inbox.

**Sources.** writeToMailbox `src/utils/swarm/teammateMailbox.ts:134-192`;
getInboxPath `:56-66`; readMailbox `:84-108`; poll + priority + markRead
`src/utils/swarm/inProcessRunner.ts:689-868,:201-271`; message classification
`:457-466`; isStructuredProtocolMessage `teammateMailbox.ts:1073-1095`.

---

## (d) Concurrency model — one node process, N async loops

```text
┌──────────────────────── ONE node.js PROCESS · ONE event loop (no threads/subprocess) ────────────────────────┐
│                                                                                                               │
│   microtask/async scheduling (no Promise.all global scheduler; each loop is an independent async task)        │
│                                                                                                               │
│   ┌─────────────┐   ┌──────────────────┐   ┌──────────────────┐   ┌──────────────────┐                       │
│   │ LEAD turn /  │   │ teammate loop #1 │   │ teammate loop #2 │   │ teammate loop #N │   ◄ EPHEMERAL          │
│   │ UI interact  │   │ runInProcess...  │   │ runInProcess...  │   │ runInProcess...  │     (in-memory only)   │
│   │              │   │ setInterval 500ms│   │ setInterval 500ms│   │ setInterval 500ms│                       │
│   │ ALS: lead    │   │ ALS ctx #1       │   │ ALS ctx #2       │   │ ALS ctx #N       │                       │
│   └──────┬───────┘   └────────┬─────────┘   └────────┬─────────┘   └────────┬─────────┘                       │
│          │ each has its OWN AbortController (independent — leader Ctrl+C does NOT cascade)                     │
│          │ FileStateCache CLONED per-teammate (no cross-contamination)                                        │
│          ▼                    ▼                      ▼                      ▼                                  │
│   ┌──────────────────────────────────────────────────────────────────────────────┐  ◄ EPHEMERAL shared      │
│   │ AppState (shared in-memory): tasks{}, teamContext, tool/permission context     │    (lost on exit)        │
│   │ MCP client connections (shared) · API auth (inherited from lead session)       │                          │
│   └──────────────────────────────────────────────────────────────────────────────┘                          │
│          │ poll/read/write (the ONLY cross-loop channel is the disk mailbox)                                  │
└──────────┼────────────────────────────────────────────────────────────────────────────────────────────────┘
           ▼
   ┌────────────────────────────────────────────────────────────────┐  ◄ PERSISTENT (on-disk)
   │ ~/.claude/teams/{team}/inboxes/*.json  ·  config.json           │
   │ ~/.claude/tasks/{taskListId}/tasks.json                         │
   └────────────────────────────────────────────────────────────────┘

   NOTE: tmuxPaneId:"" for all members — NO subprocess, NO pty, NO tmux backend.
```

**Legend.** All loops are cooperative async tasks on one event loop; the lead
can interact with UI while teammates poll. Cross-loop communication is *only*
via the on-disk mailbox (top boxes = ephemeral in-memory; bottom box =
persistent disk). Each teammate owns an independent `AbortController` and a
cloned `FileStateCache`; AppState, MCP connections and API auth are shared.

**Sources.** Fire-and-forget spawn + independent abort
`src/utils/swarm/spawnInProcess.ts:120-122,:175-178`; AsyncLocalStorage
`src/utils/swarm/inProcessRunner.ts:87,:1046-1048`; setInterval polling
`:386-433`; `tmuxPaneId:""` `src/utils/swarm/teamHelpers.ts:81`.

---

## (e) Persistent vs ephemeral across host restart

```text
   BEFORE restart (process alive)                      AFTER `claude code` restart (new process)
 ┌─────────────────────────────────────┐            ┌─────────────────────────────────────────┐
 │ IN-MEMORY (EPHEMERAL) ── all lost ►  │            │ IN-MEMORY ── reconstructed from nothing:  │
 │  • teammate async loops (alive)      │   crash/   │  • NO teammate loops (all gone)           │
 │  • AbortController signals           │   exit     │  • NO AsyncLocalStorage contexts          │
 │  • AsyncLocalStorage ctx (id/color)  │ ─────────► │  • NO abortControllers / progress         │
 │  • AppState.tasks{} status/isIdle    │            │  • AppState empty                         │
 │  • messages[] (cap 50 UI mirror)     │            │  • messages[] gone (never persisted)      │
 │  • pendingUserMessages[]             │            │                                           │
 ├─────────────────────────────────────┤            ├───────────────────────────────────────────┤
 │ ON-DISK (PERSISTENT) ── survives ──► │            │ ON-DISK ── still present, re-readable:    │
 │  • teams/{team}/config.json          │  unchanged │  • config.json (members[] — STALE; NOT    │
 │    (members[],lead*)  NOT auto-del   │ ─────────► │    auto-deleted, issue #32730)            │
 │  • inboxes/{agent}.json (read flags) │            │  • inboxes/*.json (unread msgs survive)   │
 │  • tasks/{taskListId}/tasks.json     │            │  • tasks.json (task defs survive)         │
 └─────────────────────────────────────┘            └───────────────────────────────────────────┘

   GAP: no session-id mapping, no transcript, no AsyncLocalStorage checkpoint for in-process
        teammates → they CANNOT be resumed. (Contrast: LocalAgentTask persists transcript.jsonl
        + uses --resume, the only Claude Code task type that survives a restart.)
```

**Legend.** Left = live process; right = state after a restart. Top band =
ephemeral (in-memory, gone on exit); bottom band = persistent (disk files that
survive but are not enough to revive a teammate). The disk team file lingers
(no auto-cleanup, issue #32730) yet there is no mechanism to rehydrate a
teammate's loop or identity — so in-process teammates have **no cross-session
resume**, unlike `LocalAgentTask`.

**Sources.** Ephemeral vs serialized fields + cap
`src/tasks/InProcessTeammateTask/types.ts:22-76,:101`; team file path / no
auto-delete `src/utils/swarm/teamHelpers.ts:122-124`; mailbox path
`src/utils/swarm/teammateMailbox.ts:56-66`; persistence table + resume gap
`docs/research/claude-code-agent-teams.md` (LocalAgentTask transcript/`--resume`).
