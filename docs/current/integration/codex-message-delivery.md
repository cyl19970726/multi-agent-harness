# Codex Agent Team Work And Canonical Message Delivery

```text
status: implemented identity-first Message fabric and separate WorkDelivery plane
```

This document defines how a Codex Agent Team Member receives durable Harness
coordination. It extends [Codex Integration](codex.md) and the provider-neutral
runtime contract.

## Boundary

Harness owns immutable Message identity, subscriptions, correlation and
per-recipient delivery facts. Codex app-server owns the native thread, turns,
chat, tools and execution history. Codex does not poll Harness storage.

```text
Message
  -> authorized MessageSubscription expansion
  -> one CanonicalMessageDelivery per recipient AgentIdentity
  -> exact current AgentSession resolved at claim time
  -> NodeDaemon-fenced provider dispatch
  -> app-server turn/start or turn/steer
  -> provider receipt / recipient acknowledgement
  -> updated CanonicalMessageDelivery

Work -> WorkDelivery -> exact Work revision delivery (separate plane)
RuntimeCommand -> exact provider effect (separate plane)
```

New Codex Team members always use `codex_app_server`. `codex_exec` delivery is
bounded Dynamic Workflow or historical behavior and is not a Team fallback.

## Current Durable Shapes

| Object | Meaning |
| --- | --- |
| `Message` | one immutable, source-authored communication envelope |
| `MessageSubscription` | durable routing policy for one AgentIdentity |
| `CanonicalMessageDelivery` | independent delivery truth for one Message and recipient AgentIdentity |
| `WorkDelivery` | one assigned or changed Work revision; never chat |
| `RuntimeCommand` | one authorized provider effect; never chat or Work ownership |

Message kinds are `message`, `reply`, `request_decision`,
`provider_interaction_request`, and `provider_interaction_response`. Human
readable intents such as `PLAN:`, `QUESTION:`, `BLOCKER:`, `REVIEW:` and
`DECISION:` remain Markdown inside ordinary Messages.

Provider-native questions that pause the current turn are
`provider_interaction_request` Messages. Answers are causation-linked
`provider_interaction_response` Messages with the same correlation id.
Permission is frozen at AgentSession start and never becomes a mail workflow.
Steer, Interrupt, Close, Reopen and other provider effects use RuntimeCommand;
no Message kind carries runtime authority.

## Identity, Addressing And Initial State

- Every Message freezes its source Execution Space, Node, NodeDaemon authority
  generation, authenticated sender Actor, optional sender AgentIdentity and
  AgentSession, address kind, target, recipients, content fingerprint and
  idempotency key.
- Authorized subscriptions expand one Message into one
  `CanonicalMessageDelivery` per recipient AgentIdentity. A Team address never
  becomes one shared mutable delivery.
- Delivery begins `queued` or, while crossing a Node boundary, `routed`. The
  target NodeDaemon alone may claim it and freezes the exact current
  AgentSession id and generation at that point.
- Direct, Team, topic and authorized-broadcast addresses obey their selected
  subscription and membership policy. Team-scoped conversation may link a
  TeamRun, but AgentIdentity remains the recipient authority.
- Work owner/version and WorkEvents prove responsibility. WorkDelivery carries
  the exact Work id/version that enters the Member's safe-boundary context.
- Message may link a Work and preserve correlation/reply lineage. It never
  assigns, claims, submits or accepts Work.

Codex submits results through a Work operation with evidence references. The
adapter never converts provider final text into an automatic Work submission or
duplicate Message. A submission is fenced while a newer WorkDelivery or linked
response-required Message remains actionable.

## Projection And Idempotency

Message is immutable. Delivery is an independent versioned per-recipient
projection. Readers select the latest delivery version for each delivery id and
never infer one recipient's state from another:

```text
delivery(agent-a, message-1) queued
delivery(agent-a, message-1) provider_received
delivery(agent-b, message-1) queued

projection: agent-a received; agent-b still queued
```

A repeated authoring request with the same idempotency key and content
fingerprint returns the same Message. Reusing the key with different semantics
is rejected. A stale earlier delivery version is never dispatched again. This
same projection drives CLI inbox, Dashboard counts and delivery warnings.

## Claim, Receipt And Acknowledgement

Provider side effects may start a turn, so delivery is fenced before injection:

```text
latest queued CanonicalMessageDelivery
  -> verify Message, subscription, AgentIdentity and target Node authority
  -> resolve and freeze one current AgentSession generation
  -> claim under current NodeDaemon / Team Supervisor authority
  -> submit envelope to the same-process app-server adapter
  -> record provider_received or failed
  -> record acknowledged only when that exact recipient consumes it
```

`provider_received` means the adapter accepted the envelope for the frozen
AgentSession and native thread. `acknowledged` means that recipient consumed the
envelope. Neither means the model agreed, executed Work, answered a question or
obtained approval. Semantic response is a correlated Message, Work transition,
Host review action, or real RuntimeCommand result.

If the adapter fails before receipt, delivery remains queued or visibly failed.
A claimed delivery with uncertain provider effect requires explicit
reconciliation; it is never blindly replayed. Exactly-once semantic execution
is not inferred from a transport receipt.

## Busy Member Policy

| Member/runtime state | Ordinary Message |
| --- | --- |
| live and idle | deliver next eligible delivery as a new turn |
| current turn running | retain queued until the next eligible round |
| waiting on a provider question | answer with an exact correlated `provider_interaction_response` |
| interrupted but runtime open | allow a later ordinary turn |
| explicitly closed | reject normal delivery |
| native session unavailable/incompatible | show blocker; do not fabricate resume |

Ordinary Messages never interrupt a busy turn. Steer is a separate
RuntimeCommand and uses `turn/steer` only when the snapshotted mode supports it.
Interrupt and Close are likewise provider controls. They are not implied by a
turn, Mission Log append, TeamRun completion or Mission completion.

## Delivered Envelope And Native Continuity

Each delivered turn contains the smallest stable coordination envelope:

```text
project_id
mission_id (derived from AgentTeam)
agent_team_id / execution_node_id
team_run_id / member_run_id
work_id / work_version / work_delivery_id
authenticated sender AgentIdentity/Session and recipient AgentIdentity
team roster and roles
owned paths / worktree / permission boundary
Work context and completion criteria
optional linked Message Markdown + correlation/causation lineage
```

The envelope provides identity and responsibility, not a copy of earlier
provider chat. One live Codex MemberRun binds one native Codex thread. Later
ordinary Messages use new turns on that same thread. Resume after process loss
uses the recorded native thread id and verified `thread/resume`.

Harness does not rebuild continuity by concatenating Messages. Provider-native
subagents remain inside the Member's own thread tree and do not receive Harness
mailbox identities unless the Host explicitly creates a durable AgentIdentity,
Team membership and AgentSession for them.

## Read Surfaces And Authority

CLI, MCP, HTTP and Dashboard inboxes project the same canonical Message and
CanonicalMessageDelivery records. Default Inbox returns actionable current
mail; history views include terminal delivery lineage. Provider-native history
is resolved on demand from its native locator and is never copied into inbox.

The physical app-server handle is process-local. The machine NodeDaemon lease
and current Team Supervisor generation authorize claim and dispatch. The owner
fences both immediately before the provider operation. After a crash, a claimed
delivery remains uncertain until an operator reconciles a provider receipt or
explicitly requeues it under current authority.

Host delivery follows the same identity and ownership rule. A Codex `Stop`
boundary may consume actionable Host mail once. An already-idle Desktop task
cannot be asynchronously woken by thread id alone; it receives mail at its next
prompt or resume. See [ADR 0040](../../decisions/0040-native-host-inbox-delivery.md).

## Legacy Boundary

`TeamMessage`, `TeamMessageProjection`, `team_messages.jsonl`, delivery-policy
`manual_ack`, and legacy ACK commands are ADR 0056 compatibility history. They
may be read or exported only to reconstruct old runs. Current authoring, inbox,
provider dispatch, Dashboard projections and acceptance must not consult,
mutate, dual-write or fall back to them.

## Acceptance

1. Mail validates authenticated sender identity, authorized address/subscription
   scope and optional TeamRun/Work link.
2. Each recipient gets one independent CanonicalMessageDelivery.
3. The target NodeDaemon resolves and freezes exactly one current AgentSession
   generation before provider dispatch.
4. Provider receipt, recipient acknowledgement, semantic reply, Work state and
   RuntimeCommand result remain distinct.
5. Closed, stale-generation or incompatible sessions reject delivery without
   a provider side effect.
6. CLI, MCP, HTTP and Dashboard reconstruct the same canonical mailbox state
   without reading the legacy TeamMessage ledger.
7. Provider interaction request/response lineage is exact and authenticated.
8. Provider final text never creates a duplicate Work submission or Message.
9. Native transcript, tools, commands, files, reasoning and subagent transcript
   remain outside Harness storage.
