# Codex Message Delivery

This document explains how a persistent Codex `AgentMember` receives harness
messages. It is the operational detail behind [codex.md](codex.md) and must
stay aligned with the provider-neutral runtime contract in
[../agent-runtime.md](../agent-runtime.md).

## Core Answer

Codex does not poll the harness store by itself.

The harness owns the mailbox. A provider gateway or dispatcher reads durable
`Message` objects from the harness store and pushes eligible messages to the
member by spawning its headless exec-stream (`codex exec --json`); the
app-server `turn/start` path is the retained fallback contract (ADR 0018).

```text
Leader / AgentMember / API
  -> append Message(to_agent_id=<member>, delivery_status=queued)
  -> Provider Gateway selects latest queued messages for that member
  -> record or probe the member's exec-stream runtime
  -> spawn `codex exec --json` with the harness message envelope
     (`codex exec resume --json <provider_thread_id> ...` when known)
  -> provider-native events reduced in memory
  -> NativeSessionRef + explicit outcome/evidence candidates
  -> Message delivery update + provider-report Message
  -> AgentMember runtime/status projections
  -> Agent Dashboard read model
```

This is the critical design point. If delivery depends on Codex noticing files
or hooks polling the store, messages can be missed and the multi-agent system
becomes unreliable. Hooks and plugins help observation and packaging; they are
not the canonical message bus.

## Current Status

This document is the target delivery contract. The implementation has the first
MVP delivery loop, but it is not yet a supervised production runtime.

Implemented slices:

- `agent start --id <member>` records an exec-stream `AgentRuntime` for the
  member (no persistent process; each delivery spawns `codex exec --json`).
- `agent send --to-agent <member>` can append queued harness `Message` rows.
- `agent deliver --agent <member>` can act as a manual gateway fixture for
  exec-stream delivery.
- Delivery queue selection has a latest-message projection test so stale
  historical `queued` rows are not selected after a newer terminal row exists.
- Delivery claim now happens under the store write lock: latest queued message
  selection, unresolved-delivery blocking, delivery-attempt creation,
  and `Message.delivery_status=acknowledged` are recorded together.
- Normal `agent send`, `agent deliver`, and runtime start reject closed,
  closing, or retired members.
- The Codex turn input now uses a stable harness envelope with sender,
  recipient, channel, task, content, and delivery attempt fields.
- Agent Dashboard warnings expose pending claims, blocking provider sessions,
  failed delivery, queued task messages, and pending delivery to closed members.
- `agent gateway` can run a local provider-gateway loop or a single
  `--once` tick. The tick uses the same delivery path as `agent deliver` and
  can optionally start runtimes.
- Safe pre-provider claim recovery exists: a claimed message whose provider
  session has no provider request or turn id can be requeued by operator action
  or by the gateway claim TTL.
- The local HTTP API exposes safe control-plane actions for send message,
  deliver, retry delivery, reconcile session, request review, close member, and
  gateway tick.
- Agent Dashboard has first safe-action controls for member send/deliver/close,
  provider-session retry/fail, and task review request. These actions call the
  same API/CLI value paths rather than mutating the store directly.
- Codex source-audit and Claude Code Agent Teams case notes have been captured
  as provider-design references.

Not yet complete:

- The gateway is still an in-process CLI/API loop. It is not yet a supervised
  production daemon with durable scheduling, metrics, backoff policy, and
  deployment packaging.
- Automatic retry is intentionally narrow. It only requeues safe pre-provider
  claims where there is no provider request id or turn id. Accepted provider
  turns still require reconciliation or explicit forced operator action.
- Live Codex acceptance must still be run before claiming real persistent
  Codex AgentMember delivery. The quick MVP gate proves the object protocol,
  static dashboard, hook bridge, adapter surface, and dry-run paths only.

Do not claim persistent Codex AgentMember delivery is fully accepted until the
remaining recovery, safe-action, and daemon/backend gaps are implemented and
covered by tests.

## Current CLI Slice

The current CLI implementation is the first gateway slice:

```text
agent start --id <member>
  -> records an exec-stream AgentRuntime for that AgentMember
  -> control endpoint codex-exec-runtime://... (no persistent pid/socket)

agent send --to-agent <member>
  -> appends Message(kind=task|message, delivery_status=queued)

agent deliver --agent <member> [--start-runtime]
  -> reads latest queued messages for that member
  -> blocks if a previous provider session is unresolved
  -> spawns codex exec --json (null stdin)
  -> reuses provider_thread_id via codex exec resume when known
  -> reduces the native event stream in memory
  -> records NativeSessionRef and Message.delivery

agent gateway [--once] [--start-runtime] [--dry-run] [--claim-ttl-ms <ms>]
  -> expires safe pre-provider claims by policy
  -> groups latest queued messages by AgentMember
  -> calls the same delivery path as agent deliver
  -> keeps later messages queued while a member is busy or unresolved
```

This proves the object protocol and Codex app-server integration path. It does
not by itself prove production supervision, accepted-turn reconciliation, or
live provider behavior.

The production path should be a long-running Provider Gateway daemon or backend
service that automatically watches the queue and delivers to idle members.
Manual `agent deliver` should remain an operator tool and CI fixture.

## Message Selection Invariant

The harness store is append-only for mutable objects. Therefore the dispatcher
must select messages from the latest row per `Message.id`, not from raw append
order.

```text
raw rows:
  message-1 queued
  message-1 acknowledged

deliverable projection:
  message-1 acknowledged
```

Only the latest row with `delivery_status=queued` is deliverable. A stale
earlier `queued` row must never be delivered again.

This invariant is shared by:

- delivery queue selection;
- Agent Dashboard message projection;
- warning generation;
- review gates that reason about whether work was assigned and reported.

## Claim And Lease Invariant

A dispatcher must claim a deliverable message before provider side effects.
Provider side effects include runtime start, provider thread creation, and the
provider spawn (`codex exec --json`; fallback contract: `turn/start`).

The minimal safe order is:

```text
latest queued Message selected
  -> atomic claim or lease recorded
  -> MessageDelivery recorded as running or terminal
  -> runtime/protocol side effects
  -> terminal Message update: delivered, failed, stale, canceled
```

This matters because `turn/start` can succeed while the harness process crashes
before it records delivery. Without a claim/lease, a later dispatcher cannot
distinguish "never delivered" from "possibly delivered but not reconciled."

Acceptance-critical requirements:

- claim is atomic with the latest-message status check;
- claim is visible to Dashboard and later dispatchers;
- unresolved accepted/running/stale sessions block later normal delivery to the
  same member;
- retry requires explicit reconciliation or a safe retry policy;
- closed, closing, and retired members fail delivery before runtime start.

## Busy Member Policy

The default V1 policy is conservative:

| Member/runtime state | Normal message behavior |
| --- | --- |
| no runtime | deliver only if the caller allows runtime start |
| idle runtime | deliver next eligible queued message |
| running provider session | keep later messages queued |
| stale provider session | require reconciliation or explicit operator action |
| failed runtime | fail delivery or restart by policy |
| closed member | reject or fail delivery |

If a message arrives while Codex is already executing a turn, it remains queued
in the harness store. The gateway should not interrupt the current Codex turn
unless a policy explicitly marks the new message as interrupting. This keeps
task execution, review, and Dashboard state explainable.

## Provider Thread Mapping

One harness `AgentMember` maps to one primary Codex provider thread in V1.

```text
AgentMember.id
  -> AgentRuntime(control_endpoint)
  -> provider_thread_id
  -> many provider turns
  -> many MessageDelivery records
```

The first delivery's `thread.started` event supplies the provider thread id.
Later deliveries reuse `member.provider_thread_id` via `codex exec resume`
(fallback contract: `thread/start` + thread reuse). The thread is provider execution
context, not harness identity. If the thread is lost, the member can be
recovered by starting or binding a new provider thread while preserving harness
messages, tasks, evidence, and decisions.

Codex native subagents are a different layer. They are provider child threads
and should be ingested as `ProviderChildThread` or `AgentEvent` data. They do
not automatically become harness `AgentMember` identities.

## JSON-RPC Delivery State Machine

The primary exec-stream path is: spawn `codex exec --json` (null stdin), read
NDJSON until the terminal `turn.completed` / `thread.idle` event or process
exit; a timeout becomes running or stale, not silent success.

The app-server fallback path (retained contract, ADR 0018) is:

```text
connect WebSocket-over-Unix-socket
  -> initialize
  -> initialized notification
  -> thread/start or use known thread id
  -> turn/start
  -> read response and notifications
  -> terminal event or timeout
  -> reconcile if accepted but not terminal
```

Terminal signal priority:

1. `turn/completed` notification.
2. `thread/status/changed` to idle plus `thread/read` reconciliation.
3. Stop hook report candidate.
4. Timeout converted to `running` or `stale`, not silent success.

Accepted but unresolved turns block later delivery to the same member. This is
intentional because pushing another task into a member with an unknown current
turn would make assignment, report, and evidence ordering ambiguous.

## Message Envelope

Each delivered turn must include a harness envelope before task content:

```text
Harness message id: <message.id>
kind: <task|message|report|...>
task: <task.id or none>
from_agent_id: <sender>
to_agent_id: <agent member>
channel: <channel or none>
delivery_attempt: <attempt id or lease id>
content:
<message.content>
```

The envelope lets Codex hooks, transcripts, provider events, and Dashboard
warnings correlate provider behavior back to the canonical harness message.
Provider chat without this envelope is evidence at best; it is not a delivered
harness assignment.

The envelope should be stable enough for hooks, transcripts, and reconciler
logic to parse. Debug enum formatting is not a contract.

## Hooks And Plugins

Hooks are observation and reconciliation inputs:

- `SessionStart` can record runtime start context.
- `UserPromptSubmit` can verify the harness envelope.
- `PostToolUse` can produce evidence candidates.
- `SubagentStart` and `SubagentStop` can expose Codex native subagents.
- `Stop` can provide a final report candidate.

Plugins package skills, hooks, MCP tools, and install metadata. They are the
right productization path once the contract is stable. They still must not own
the canonical mailbox or decide whether a `Message` was delivered.

## Dashboard Proof

The Agent Dashboard should prove delivery without reading raw provider logs:

```text
member card
  -> runtime health: process/socket/protocol/delivery
  -> current task and provider thread
  -> inbox/outbox latest messages
  -> provider sessions and terminal source
  -> events timeline
  -> child provider threads
  -> warnings for queued, stale, failed, or missing report states
```

The Dashboard read model must use the same latest-message projection as the
dispatcher. Otherwise the UI can claim a message is queued after delivery has
already acknowledged or failed it.

## Target V1.1 Work

The next production-quality step is to harden the provider gateway loop into a
supervised service:

```text
Gateway daemon / backend
  -> watch latest queued messages
  -> group by target AgentMember
  -> apply member state and permission policy
  -> start/probe runtime as needed
  -> deliver one message at a time per member
  -> stream notifications into harness store
  -> reconcile stale turns
  -> expose safe Dashboard actions
```

Safe Dashboard actions should call the same API:

- create member;
- send message; implemented for existing members;
- retry delivery; implemented for safe pre-provider claims and explicit forced
  operator action;
- request review; implemented as a task status transition plus review message;
- interrupt by explicit policy;
- close member; implemented for existing members;
- reconcile stale session; implemented for terminal operator reconciliation.

## Implementation Order

Use this order so code follows the contract instead of backfilling the docs:

1. Store and CLI: atomic latest-message claim/lease before provider side
   effects. Implemented for the file store and CLI gateway slice.
2. Runtime guard: reject delivery and runtime restart for closed, closing, and
   retired members. Implemented for normal `agent send`, `agent deliver`, and
   runtime start.
3. Envelope: stable parseable message envelope with sender, recipient, channel,
   task, and delivery attempt. Implemented for Codex `turn/start` input.
4. Provider session reconciliation: accepted, running, terminal, stale, failed,
   and canceled states visible from the store. Implemented for operator
   reconciliation and safe pre-provider retry; accepted provider turns still
   need live reconciliation policy.
5. Dashboard read model: warnings for queued, claimed, running, stale, failed,
   closed-member delivery, and missing report. Advisory warnings and first safe
   actions are implemented.
6. Provider Gateway daemon/backend loop: replace manual delivery as the normal
   runtime path. Implemented as an in-process CLI/API loop; still needs
   production supervision.
7. Managed hooks/plugin packaging: improve live status and report extraction
   without making hooks the message bus.

## Failure Modes To Test

Every Codex provider acceptance suite should include:

- latest-message projection prevents stale queued redelivery;
- no delivery when an unresolved provider session exists;
- message sent to a missing or closed member fails visibly;
- runtime start failure becomes evidence, not silent queue loss;
- turn accepted but not completed becomes running or stale;
- hook/report reconciliation can finish a previously running session;
- Dashboard and dispatcher agree on message status;
- native Codex subagent events do not masquerade as harness members.
