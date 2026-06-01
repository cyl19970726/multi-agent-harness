# Research: Claude Code Agent Teams

A faithful writeup of how Claude Code runs persistent teammates. Source paths
cite a local read-only checkout of the Claude Code source
(`/Users/hhh0x/claude-code-source/…`); paths are kept verbatim so claims are
checkable against that tree.

**Diagrams.** Detailed ASCII architecture, lifecycle, message-sequence,
concurrency, and persistence diagrams are in the companion
[claude-code-agent-teams-diagrams.md](claude-code-agent-teams-diagrams.md).

## Correction / Modes (definitive — supersedes the original "in-process only" answer)

> **Corrected 2026-06-01 against (a) the official docs and (b) a re-scan of the
> local source at version `2.1.88`.** An earlier conclusion in this doc said
> Claude Code teammates are *only* in-process and tmux is unused/vestigial. That
> was an artifact of reading **one of three spawn branches** in a headless /
> non-tmux session. It is corrected below, not silently deleted: the old
> "Answer first" section is retained verbatim under
> [Original (corrected) answer](#original-corrected-answer-in-process-was-only-one-of-three-branches)
> and flagged.

**Does Claude Code use tmux for agent teams? YES — optionally, as one of two
display modes.** Tmux (or iTerm2) is **not required**, but it *is* a first-class,
fully implemented mode, not a leftover field. Agent teams have **two display
modes** (official docs term), backed by **three spawn code paths** in the source:

| Mode (docs) | Spawn path (source) | Process model | `tmuxPaneId` | TUI? | Cross-session `/resume`? |
| --- | --- | --- | --- | --- | --- |
| **Split panes** (tmux) | `handleSpawnSplitPane` → `tmux split-window` | **Separate `claude` OS process per pane** | real pane id (e.g. `%3`) | Yes — one pane per teammate, click to interact | Docs silent (separate processes survive a leader `/resume` differently than in-process; not confirmed) |
| **Split panes** (tmux new-window) | `handleSpawnSeparateWindow` → `tmux new-window` | **Separate `claude` OS process per window** | real pane id | Yes — separate window | as above |
| **Split panes** (iTerm2) | `ITermBackend` (AppleScript / `it2` CLI) | **Separate `claude` OS process per pane** | real pane id | Yes | as above |
| **In-process** | `handleSpawnInProcess` | Async loop in the leader's **one** Node.js process | literal `'in-process'` / `'leader'` | No — single terminal, cycle with Shift+Down | **No** — `/resume` & `/rewind` do not restore in-process teammates |

**What selects the mode** (`isInProcessEnabled()`,
`src/utils/swarm/backends/registry.ts:351-389`, in selection priority):

1. **Headless / `-p` / SDK** → always in-process (`getIsNonInteractiveSession()`
   returns true at `registry.ts:354`). *This is the gate that produced the
   original wrong conclusion.*
2. **Explicit mode** from `--teammate-mode <auto|tmux|in-process>`
   (`src/main.tsx:3857`, applied via `setCliTeammateModeOverride` at
   `main.tsx:1216`, frozen into a session snapshot by
   `teammateModeSnapshot.ts`) or `settings.json` `teammateMode`:
   `'in-process'` → in-process; `'tmux'` → pane backend (errors with install
   instructions if no backend, `spawnMultiAgent.ts:1054-1056`).
3. **`'auto'` (default)** → `enabled = !insideTmux && !inITerm2`
   (`registry.ts:380-382`): if the leader is **inside tmux OR iTerm2**, use the
   **pane backend** (separate `claude` processes); otherwise in-process.
   `insideTmux` = `!!process.env.TMUX` captured at module load
   (`detection.ts:36-40`). In `'auto'` only, if a pane backend turns out
   unavailable (e.g. iTerm2 without `it2`), it silently falls back to in-process
   (`spawnMultiAgent.ts:1059-1068`, `markInProcessFallback()`).

Backend detection priority (`detectAndGetBackend`, `registry.ts:136-200`):
(1) inside tmux → `TmuxBackend`; (2) iTerm2 + `it2` CLI → `ITermBackend`;
(3) iTerm2 without `it2` → tmux fallback; (4) tmux installed (outside tmux) →
external tmux swarm session (`tmux -L <swarm-socket> new-session`,
`TmuxBackend.ts:475`); (5) else throw install instructions.

**How a tmux teammate is actually spawned (separate process, not async loop):**
`handleSpawnSplitPane` runs `tmux split-window … -P -F '#{pane_id}'`
(`TmuxBackend.ts:571-617`) to get a real pane id, then sends
`cd … && env … <binaryPath> --agent-id … --agent-name … --team-name … --parent-session-id …`
to that pane via `tmux send-keys -t <paneId> <cmd> Enter`
(`spawnMultiAgent.ts:440-444`; `TmuxBackend.ts:157`). `binaryPath =
getTeammateCommand()` = `process.execPath` (native build) or `process.argv[1]`
(`spawnMultiAgent.ts:193-198`) — i.e. the `claude` executable itself. The
teammate CLI flags are registered at `main.tsx:3851-3857`. The leader and the
separate-process teammate communicate over the **same file mailbox**
(`writeToMailbox`, `spawnMultiAgent.ts:513`) used by in-process teammates — which
is exactly why the original doc's "single uniform mailbox API that process-based
teammates could share" turns out to be the real production path, not hypothetical.

**Task-type subtlety that hid this:** the out-of-process tmux teammate is *also*
registered under the `in_process_teammate` task type
(`registerOutOfProcessTeammateTask`, `spawnMultiAgent.ts:760,798`); on abort it
calls `getBackendByType(...).killPane(paneId, …)` (`spawnMultiAgent.ts:828-829`).
So the task-type *name* is not a reliable signal of the process model — the real
separate-process spawn lives in the tmux/iTerm2 code paths, not in a distinct task
class. The `tmuxPaneId: ''` empty placeholder the original scan cited is only the
team-lead / pre-spawn member record in `TeamCreateTool.ts:170,206`, **not** the
teammate spawn path.

**Sources.** Official:
[Orchestrate teams of Claude Code sessions](https://code.claude.com/docs/en/agent-teams)
(two display modes; split panes "requires tmux or iTerm2"; default mode "auto";
override via `teammateMode` / `claude --teammate-mode in-process`; "no session
resumption with in-process teammates"). Local source v2.1.88
(`/Users/hhh0x/claude-code-source/package.json:3`):
`src/tools/shared/spawnMultiAgent.ts:1040` (the three-branch selector),
`:193-198`, `:388`, `:440-444`, `:466,504,533`, `:587-604`, `:647-656`,
`:760,798`, `:828-829`, `:1059-1068`;
`src/utils/swarm/backends/registry.ts:136-200,351-389`;
`src/utils/swarm/backends/TmuxBackend.ts:157,475,529,551,571-617`;
`src/utils/swarm/backends/detection.ts:36-40`;
`src/utils/swarm/backends/teammateModeSnapshot.ts`;
`src/main.tsx:1216,3851-3857`; `src/tools/TeamCreateTool/TeamCreateTool.ts:170,206`.

**Persistence per mode (definitive):**
- **In-process** — persistent only *within* one session (resident idle-poll
  loop); **no** cross-session resume; official docs confirm `/resume` & `/rewind`
  do **not** restore in-process teammates. Everything below in this doc about the
  in-process runtime remains accurate.
- **tmux / iTerm2 split-pane** — a separate `claude` process in its own pane/
  window. The team config stores real pane ids per teammate, and the docs warn
  not to hand-edit it because it "holds runtime state such as session IDs and
  tmux pane IDs." Whether those processes are re-attached on a leader `/resume`
  is **not stated** by the docs and not implemented as auto-restore in this
  source; the only *explicit* resume limitation called out is for in-process.

**What this changes for the rest of this doc:** the sections below describe the
**in-process mode** correctly and in detail — treat them as scoped to that mode.
The tmux/iTerm2 split-pane mode (separate `claude` processes, real pane ids) is
**not** covered by them; this Correction section is canonical for the
mode/process-model question. The companion
[claude-code-agent-teams-diagrams.md](claude-code-agent-teams-diagrams.md)
diagrams are likewise in-process-only and carry a matching correction note.

### Original (corrected) answer — "in-process was only one of three branches"

> **CORRECTED.** The paragraph below was the original "Answer first" and is wrong
> as a *universal* claim about Claude Code agent teams. It is accurate **only for
> the in-process spawn branch** (headless/`-p`, or `'auto'` mode outside tmux/
> iTerm2). It missed `handleSpawnSplitPane` / `handleSpawnSeparateWindow` /
> `ITermBackend`, which spawn a real separate `claude` process inside a real tmux
> pane / iTerm2 pane. See the Correction / Modes section above. Kept verbatim for
> the record:

**In-process async, single-session persistent.** Claude Code teammates are not
backed by tmux panes, child processes, a daemon, or cross-session
`--resume`. They run as long-lived async tasks **inside the leader's single
Node.js process**, multiplexed by `AsyncLocalStorage` for per-teammate identity
(`src/utils/swarm/teammateContext.ts:41-64`). A teammate is persistent only
*within one session*: its `runInProcessTeammate()` loop stays alive and polls a
file mailbox while idle (`src/utils/swarm/inProcessRunner.ts:883-1552`), but on
`claude code` restart every teammate dies — the `AbortController` lifecycle and
the in-memory `AsyncLocalStorage` context cannot be checkpointed or resumed.
Coordination is file-based (disk mailboxes + a shared task list, ~500ms poll),
which is what lets a hypothetical tmux/iTerm teammate coexist with an in-process
one on the same disk API. So: persistence-during-a-session via a resident async
loop; **no** cross-session resume; **no** subprocess/pty substrate for the
teammates themselves.

## Task-type taxonomy

Claude Code models background work as typed tasks in app state. Six matter here.
**Note (corrected):** the `in_process_teammate` task type is the substrate for
in-process teammates **and is also reused to track tmux/iTerm2 split-pane
teammates** whose actual `claude` process runs out-of-process
(`registerOutOfProcessTeammateTask`, `src/tools/shared/spawnMultiAgent.ts:760,798`);
see the Correction / Modes section. The table below describes the **in-process**
substrate.

| Task type | Substrate | Process model | Persistent vs one-shot | Cross-session resume | Source |
| --- | --- | --- | --- | --- | --- |
| `InProcessTeammateTask` | In-process async | Same Node.js event loop | Persistent (within session) | No | `src/tasks/InProcessTeammateTask/types.ts:22-76` |
| `LocalAgentTask` | One-shot subprocess | Queued / awaited per turn | One-shot | Yes (`--resume`) | `src/tasks/LocalAgentTask/LocalAgentTask.tsx:116-151` |
| `RemoteAgentTask` | Out-of-process / orchestrator | Polled via teleport API | Persistent (server-side) | Yes (server) | `src/tasks/RemoteAgentTask/RemoteAgentTask.tsx:22-59` |
| `LocalShellTask` | Child process (pty) | `child_process.spawn()` | One-shot | No | `src/tasks/LocalShellTask/guards.ts:11-32` |
| `LocalMainSessionTask` | In-process async | Same as leader | Persistent (within session) | No | `src/tasks/LocalMainSessionTask.ts:54-479` |
| `DreamTask` | In-process async (forked) | Async memory-consolidation agent | Persistent (within session) | No | `src/tasks/DreamTask/DreamTask.ts:25-41` |

Key reading of the table:

- **Teammates are `InProcessTeammateTask`.** The one persistent-within-session,
  no-resume, no-subprocess type. State (`identity`, `abortController`,
  `status`, `isIdle`, capped `messages`, `pendingUserMessages`) lives only in
  app state / memory; there is no on-disk runtime state for it.
- **`LocalAgentTask` is the one with real resume** (transcript +
  sidechain JSONL, disk bootstrap on `--resume`) — but it is a **stateless
  one-shot agent**, explicitly *not* used for team members.
- **`LocalShellTask` is the only pty/child-process substrate**, and it is for
  shell commands, not agents.
- `RemoteAgentTask` pushes persistence off-box (server-side, polled with
  per-type completion checkers that survive `--resume`).
- `LocalMainSession` (Ctrl+B-backgrounded main query) and `Dream` (memory
  consolidation) are additional in-process async tasks, single-instance, no
  resume.

## How teammates are launched (in-process mode)

This section is the **in-process** spawn path. The **tmux/iTerm2 split-pane**
path is different — it spawns a separate `claude` OS process via
`tmux split-window`/`new-window` + `tmux send-keys <claude-binary> --agent-id …`
(`spawnMultiAgent.ts:440-444,587-604,647-656`; `TmuxBackend.ts:157,571-617`);
see the Correction / Modes section.

In-process spawn is a fire-and-forget async loop, not a process fork
(`src/utils/swarm/spawnInProcess.ts:104-216`,
`src/utils/swarm/InProcessBackend.ts:72-143`):

```text
InProcessBackend.spawn(config)
  └─ spawnInProcessTeammate(config, context)
       ├─ agentId = formatAgentId(name, teamName)      // "researcher@my-team"
       ├─ taskId  = generateTaskId('in_process_teammate')
       ├─ abortController = createAbortController()      // INDEPENDENT of parent
       ├─ teammateContext via createTeammateContext()    // → AsyncLocalStorage
       ├─ register InProcessTeammateTaskState in AppState
       └─ register process-exit cleanup (abortController.abort())
  └─ startInProcessTeammate({ identity, taskId, prompt, teammateContext,
       toolUseContext: { ...context, messages: [] },     // clears parent msgs
       abortController, model, systemPrompt, permissions, … })
       └─ void runInProcessTeammate(config).catch(log)    // fire-and-forget
```

The `AbortController` is deliberately **independent of the leader's query**
(`spawnInProcess.ts:120-122`): Ctrl+C in the main interaction must not kill
teammates — they outlive the foreground turn.

### The resident teammate loop

`runInProcessTeammate()` is the long-lived part
(`inProcessRunner.ts:1048-1417`). Pseudocode:

```text
runWithTeammateContext(ctx, () => runWithAgentContext(agentCtx, async () => {
  while (!aborted && !shouldExit) {
    status = 'running'; isIdle = false
    for await (msg of runAgent({...})) {       // one turn
      if (lifecycleAbort) break                // kill teammate
      if (workAbort) { workWasAborted=true; break }  // Escape: stop turn, stay alive
      update progress + capped task.messages (max 50) + unbounded allMessages
    }
    isIdle = true
    if (!wasAlreadyIdle) sendIdleNotification(...)        // → leader mailbox
    waitResult = await waitForNextPromptOrShutdown(...)   // BLOCKING poll loop
    switch (waitResult.type) {
      shutdown_request | new_message → currentPrompt = ...; break
      aborted → shouldExit = true; break
    }
  }
})); status='completed'; emitTaskTerminatedSdk(...)
```

While idle the teammate is not gone — it sits in
`waitForNextPromptOrShutdown()` polling the mailbox and the shared task list
every 500ms (`inProcessRunner.ts:689-868`). Killing is a signal, not a process
kill: `killInProcessTeammate()` calls `task.abortController?.abort()`, marks the
task `killed`, removes the member from the team file, and emits an SDK
`task_terminated` bookend (`spawnInProcess.ts:227-328`).

## Inter-agent communication: the file mailbox

Teammates talk through **disk files under
`~/.claude/teams/{team}/inboxes/{agent}.json`**, not in-memory channels
(`src/utils/swarm/teammateMailbox.ts`). Message shape (`teammateMailbox.ts:43-50`):

```text
TeammateMessage = { from, text, timestamp, read, color?, summary? }
```

- **Write** `writeToMailbox()` (`teammateMailbox.ts:134-192`) — locked with
  `proper-lockfile` (5–100ms backoff, `{inbox}.lock`), pretty JSON for
  readability.
- **Read** `readMailbox()` (`:84-108`) returns the array (or `[]` on ENOENT);
  `readUnreadMessages()` (`:115-125`) filters `read:false`;
  `markMessageAsReadByIndex()` (`:201-271`) does an atomic read-mark-write under
  the lock.
- **Teammate → leader**: `sendMessageToLeader()` always writes to
  `TEAM_LEAD_NAME`'s inbox (`inProcessRunner.ts:547-563`).
- **Poll loop priority**: shutdown requests > leader messages > FIFO peer
  messages > unclaimed shared tasks, at a 500ms interval
  (`inProcessRunner.ts:689-868`).
- **Permission delegation** is dual-path: primary is the leader's in-process
  `ToolUseConfirm` queue bridge; fallback is the same file mailbox carrying
  permission request/response XML when the bridge is unavailable.

The cost of this design is real: ~500ms poll latency and disk I/O per message.
The benefit is a single uniform mailbox API. **Corrected:** process-based
teammates are not hypothetical — tmux/iTerm2 split-pane teammates are separate
`claude` processes that use **this same file mailbox** to talk to the leader
(`spawnMultiAgent.ts:513,727`), which is why one disk API serves both modes.

## Team creation and what persists

Team identity is a file: `~/.claude/teams/{team}/team.json`
(`TeamCreateTool.ts:156-175`), holding `name`, `leadAgentId`, `leadSessionId`,
and `members[]` (each with `agentId`, `name`, `agentType`, `model`,
`tmuxPaneId`, `cwd`, `subscriptions`). **Corrected:** `tmuxPaneId` is `''` only
for the team-lead / pre-spawn placeholder record (`TeamCreateTool.ts:170,206`)
and the literal `'in-process'`/`'leader'` for in-process teammates
(`spawnMultiAgent.ts:957-1026`); for a **tmux/iTerm2 split-pane teammate it holds
a real pane id** (e.g. `%3`) from `tmux split-window … -P -F '#{pane_id}'`
(`spawnMultiAgent.ts:466,504,533,679,718,747`; `TmuxBackend.ts:571-617`). It is
not always empty. Written on create (`:177`), read on spawn (`:66`), and
registered for session cleanup (`:180`) — but **not deleted on session exit**
(source references issue \#32730); files persist until an explicit `TeamDelete`.

### Persisted across sessions vs not

| Persisted on disk across sessions | Lost on restart |
| --- | --- |
| Team file `team.json` (members, ids, config) | In-process teammate runtime state (all in `AsyncLocalStorage` / memory) |
| Shared task list `~/.claude/tasks/{id}/tasks.json` | `task.messages` app-state mirror (capped 50; full history only in live `allMessages`) |
| Mailbox files `inboxes/{agent}.json` (unread flag preserved) | Permission mode, idle state, pending messages (app-state only) |
| `LocalAgentTask` transcripts `~/.claude/agents/{id}/transcript.jsonl` | No session-id mapping exists for in-process teammates → no `--resume` |

So even though the team file, task queue, and mailboxes survive a restart, the
**running teammates do not**. A resumed leader session can re-read the team file
and the task list, but the prior teammates' live contexts are gone; nothing
restores them.

### Team memory sync is a different system

`src/services/teamMemorySync/index.ts:1-92` syncs `~/.claude/team-memory/{repo_hash}/`
to a server with upsert semantics (deltas by content hash; deletions do not
propagate). This is an **org-wide shared knowledge base**, scoped per repo — it
is explicitly **not** teammate persistence or session resumption.

## Critical design decisions (as found in source — in-process mode)

> **Scope (corrected):** items 1, 4, 5 below characterize the **in-process** mode.
> In **tmux/iTerm2 split-pane** mode teammates are *separate* `claude` processes
> with their own event loops and heaps (so the single-heap memory ceiling in
> item 5 does not apply), launched via `tmux send-keys <claude-binary>`; see the
> Correction / Modes section.

1. **Single-session model (in-process)** — in-process teammates share the
   leader's event loop, FD cache, MCP connections, and API auth; they cannot span
   `claude code` invocations. (tmux/iTerm2 teammates are separate processes and do
   not share the heap, but are still not auto-resumed across a leader restart.)
2. **File mailbox, not in-memory channels** — uniform disk API for correctness
   under concurrent access; cost is poll latency + disk I/O.
3. **No resume for in-process teammates** — resume would need per-teammate
   session files plus checkpoint/restore of `AsyncLocalStorage` (infeasible).
4. **Independent `AbortController`** — teammates outlive the foreground turn;
   Ctrl+C does not cascade to them (`spawnInProcess.ts:120-122`).
5. **Memory-bound scaling** — all teammates in one heap; the source notes a
   crash near 292 agents / ~36.8GB in a burst.

## Implication for our harness

Claude Code's teammates are the **opposite extreme** from our exec-stream
one-shot model: maximum in-session statefulness (resident loop, shared heap)
with zero cross-session durability and a non-trivial memory ceiling. Notably,
the **one Claude Code task type that does survive restarts** (`LocalAgentTask`)
gets there exactly the way our store could — transcript/sidechain JSONL plus
`--resume` — not by keeping a process alive. That validates session-resume as
the durable primitive, and is picked up in
[runtime-persistence-decision.md](runtime-persistence-decision.md).
