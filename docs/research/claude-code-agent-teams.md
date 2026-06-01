# Research: Claude Code Agent Teams

A faithful writeup of how Claude Code runs persistent teammates. Source paths
cite a local read-only checkout of the Claude Code source
(`/Users/hhh0x/claude-code-source/…`); paths are kept verbatim so claims are
checkable against that tree.

## Answer first: tmux/long-lived process, session-resume, or in-process?

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

Claude Code models background work as typed tasks in app state. Six matter here;
only one (`in_process_teammate`) is the team-member substrate.

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

## How teammates are launched

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
The benefit is a single uniform mailbox API that process-based teammates (if
ever spawned) could share.

## Team creation and what persists

Team identity is a file: `~/.claude/teams/{team}/team.json`
(`TeamCreateTool.ts:156-175`), holding `name`, `leadAgentId`, `leadSessionId`,
and `members[]` (each with `agentId`, `name`, `agentType`, `model`,
`tmuxPaneId` — **empty for in-process** — `cwd`, `subscriptions`). Written on
create (`:177`), read on spawn (`:66`), and registered for session cleanup
(`:180`) — but **not deleted on session exit** (source references issue
\#32730); files persist until an explicit `TeamDelete`.

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

## Critical design decisions (as found in source)

1. **Single-session model** — teammates share the leader's event loop, FD cache,
   MCP connections, and API auth; they cannot span `claude code` invocations.
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
