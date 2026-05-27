# Claude Code Agent Teams Case

This reference captures engineering lessons from the Claude Code Agent Teams
source audit. Use it when designing a multi-agent mailbox, delivery gateway,
runtime loop, or dashboard evidence model.

Audit source:

- Repository: `https://github.com/cyl19970726/claude-code-sourcemap.git`
- Audited commit: `a8a678cb6244e6770e1e421767ff0987a1d95549`
- Local audit clone used during this project: `/tmp/claude-code-sourcemap`

## Core Findings

Claude Code Agent Teams does not rely on hooks to inject messages into agents.
It uses an explicit message tool and mailbox path.

1. It is not based on hooks pushing messages into agent context.
2. Agent Teams communication uses the `SendMessage` tool.
3. `SendMessage` writes to teammate mailbox files:
   `.claude/teams/{team}/inboxes/{agent}.json`.
4. Mailbox writes use file locking.
5. The receiving runtime or poller reads unread mailbox messages.
6. Unread messages are wrapped as XML teammate messages and submitted as the
   next user prompt or queued for later submission.
7. If the agent is busy, messages are queued and submitted when the agent
   becomes idle.
8. In-process teammates poll mailbox after each turn, set the new message as
   `currentPrompt`, and run `runAgent` again.
9. Hooks observe lifecycle, such as `SubagentStart` and `SubagentStop`; they are
   not the message bus.

## Source Points

The important source points from the audit:

- `SendMessageTool` is the official tool entry:
  `restored-src/src/tools/SendMessageTool/SendMessageTool.ts:520`
- Mailbox file model and locked writes:
  `restored-src/src/utils/teammateMailbox.ts:1`
- Initial teammate prompt after spawn is also written to mailbox:
  `restored-src/src/tools/shared/spawnMultiAgent.ts:511`
- Inbox poller wraps unread messages in `<teammate-message>` and submits them:
  `restored-src/src/hooks/useInboxPoller.ts:810`
- In-process teammate polls mailbox and turns messages into the next prompt:
  `restored-src/src/utils/swarm/inProcessRunner.ts:755`
- The teammate system prompt says plain text is not visible to teammates and
  requires `SendMessage`:
  `restored-src/src/utils/swarm/teammatePromptAddendum.ts:8`
- Running local agents can queue pending messages and drain them as
  `queued_command` attachments:
  `restored-src/src/tasks/LocalAgentTask/LocalAgentTask.tsx:162`
  and `restored-src/src/utils/attachments.ts:1085`

## Delivery Shape

The design can be summarized as:

```text
Agent uses SendMessage tool
  -> write recipient mailbox with lock
  -> recipient runtime/poller reads unread messages
  -> if idle: submit as next user prompt
  -> if busy: queue in local app state
  -> when idle: submit queued message
  -> mark mailbox message read only after submit or durable queue
```

For in-process teammates:

```text
runAgent(prompt)
  -> finish turn
  -> send idle notification
  -> poll mailbox
  -> select shutdown first, leader messages next, then FIFO
  -> mark selected message read
  -> set currentPrompt
  -> runAgent(currentPrompt)
```

## Engineering Principles

### Message Is A Product Surface

Agent communication is exposed as a tool, not hidden in prompt convention.

Reusable principle:

```text
If a message affects task state, permission, handoff, or review, make it a
first-class action with a durable record.
```

### Mailbox Is Separate From Runtime

The mailbox is durable enough for coordination; the runtime is the consumer.
This separation makes busy, idle, crash, and resume behavior explicit.

Reusable principle:

```text
Do not make provider transcript the only mailbox. The runtime may die or be
busy, but the message must remain visible and recoverable.
```

### Hooks Observe, They Do Not Deliver

Lifecycle hooks can record starts, stops, tool use, or final reports. They are
not responsible for deciding which message is next or whether a delivery is
valid.

Reusable principle:

```text
Hooks may reduce provider activity into events; they should not be the
canonical message bus unless the whole system is designed and tested that way.
```

### Busy Policy Is A First-Class Contract

Claude Code distinguishes idle submission from busy queuing. This avoids
surprising interruption of active work.

Reusable principle:

```text
Normal messages should queue while an agent is busy. Interrupts must be an
explicit policy with visible evidence.
```

### The Agent Must Use The Communication Tool

The teammate prompt explicitly states that plain assistant text is not visible
to others and that the agent must use `SendMessage`.

Reusable principle:

```text
An agent's final answer is not automatically a team report. Reports, blockers,
handoffs, and questions need explicit message or workflow objects.
```

### Read Marking Happens After Safe Handoff

The inbox poller marks mailbox messages as read only after they have been
submitted or reliably queued. That prevents permanent message loss when the
receiver is busy.

Reusable principle:

```text
Do not acknowledge a message before it is either injected into a turn or stored
in a recoverable local queue.
```

## Adaptation To Harness

For Multi-Agent Harness, the clean translation is:

```text
Harness owns mailbox.
Provider Gateway delivers turns.
Hooks and plugins observe or package the workflow.
Dashboard proves the delivery and review chain from harness store.
```

Recommended mapping:

| Claude Code case | Harness adaptation |
| --- | --- |
| `SendMessage` tool | `message send` / Dashboard safe action / future provider tool |
| mailbox JSON file | canonical `Message` object in harness store |
| unread/read | delivery status plus claim/lease |
| inbox poller | Provider Gateway dispatcher |
| XML teammate wrapper | harness message envelope in provider turn input |
| busy queue | member queue policy |
| idle notification | runtime/member status event |
| SubagentStart/Stop hooks | lifecycle `AgentEvent` inputs |

## Design Warnings

Do not copy these details blindly:

- File mailbox works for Claude Code's local team model, but a harness may need
  an append-only store, database, or queue.
- `read=true` is not enough for crash-safe delivery when provider side effects
  include remote turn creation. Add claim/lease or delivery attempts.
- XML wrapping is a useful envelope pattern, not a mandatory format.
- Team roster rules may differ. Claude Code prevents nested teammates because
  its roster is flat; another system can support hierarchy if the graph and
  dashboard prove it.

## Questions To Ask After Reading

- What is the durable mailbox in this system?
- What tool or API writes messages?
- What component consumes messages into agent turns?
- When is a message considered acknowledged?
- What happens if the runtime crashes after claim but before turn injection?
- What happens if the runtime crashes after turn injection but before report?
- What is the busy policy?
- What state does the dashboard show without reading provider transcript?
- Which hooks are lifecycle observation only?
- Which actions require permission request/response messages?
