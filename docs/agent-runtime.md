# Agent Runtime

This document defines the provider-neutral Agent Runtime Object Model
(A-ROM). Provider-specific files under `docs/integration/` explain how a
concrete provider implements this contract.

## Vision Link

The product needs persistent agent members that can be created, messaged,
observed, reviewed, and closed. A provider turn is useful only after the harness
can relate it to a member, task, message, evidence, proposal, and decision.

Final acceptance for this mechanism:

```text
create AgentMember
  -> start AgentRuntime
  -> send Message(kind=task)
  -> record delivery and provider session
  -> reduce provider events into harness state
  -> receive report/evidence/proposal
  -> close or recover runtime
```

## Key Questions

| Question | Runtime answer |
| --- | --- |
| Who is the agent? | `AgentMember` durable identity, role, skills, permissions, team, and workspace policy. |
| What is running? | `AgentRuntime` process/session/control endpoint and health. |
| What did the provider do? | `ProviderSession` plus `AgentEvent` stream. |
| How does a member receive work? | `MessageDelivery` maps harness messages to provider turns or native inputs. |
| What happens when busy? | Harness-owned queue policy decides enqueue, interrupt, reject, or fail. |
| How is context built? | Harness packages bounded task context, evidence refs, skill refs, and permissions per delivery. |
| How are providers swapped? | Providers implement the same interfaces and cannot own harness state. |

## A-ROM Objects

| Object | Owns | Refuses |
| --- | --- | --- |
| `AgentMember` | identity, role, prompt refs, skill refs, permission profile, team, current projections | provider transcript as identity |
| `AgentRuntime` | lifecycle, pid/socket/control endpoint, protocol and delivery health | task ownership or decisions |
| `MessageDelivery` | message to provider request correlation and terminal delivery state | hidden chat assignment |
| `ProviderSession` | one provider interaction and reproducible request/output refs | canonical task state |
| `AgentEvent` | normalized provider/runtime/hook events | raw provider-specific semantics |
| `ProviderChildThread` | provider-native subagent or child thread visibility | durable harness member identity by default |
| `PermissionProfile` | allowed tools, approval policy, sandbox, live/destructive boundaries | prompt-only safety |
| `WorkspaceRef` | cwd, worktree, branch, environment, owned paths | implicit global workspace |

## Provider Interfaces

```text
AgentProvider
  create_runtime(member, workspace, permissions)
  close_runtime(runtime)
  health(runtime)
  deliver(message, context)
  interrupt(runtime, reason)
  read_events(runtime, cursor)

MessageDelivery
  package_context(message, task, evidence_refs, skill_refs, permissions)
  send(provider_request)
  correlate_response(response_or_event)
  record_delivery(status, provider_session)

EventReducer
  provider_event -> AgentEvent
  AgentEvent -> member/task/message/proposal/evidence projections

WorkspaceProvider
  prepare_workspace(task)
  attach_branch_or_pr(task)
  inspect_changed_paths(task)
  cleanup_or_archive(task)
```

Codex, Claude Code, OpenClaw, a Permission Agent, or a future cloud provider
should implement these boundaries without changing `Goal`, `Task`, `Message`,
`Evidence`, `Proposal`, or `Decision` semantics.

## Queue And Context Policy

The harness owns delivery policy:

| Member state | Message policy |
| --- | --- |
| `idle` | deliver next eligible message |
| `running` | enqueue normal messages; allow explicit interrupt only by policy |
| `waiting_for_input` | deliver clarification or decision messages |
| `waiting_for_approval` | deliver approval decision or keep queued |
| `blocked` | queue or reassign, depending on Leader decision |
| `closed` / `error` | fail delivery and create evidence/blocker |

Provider context is ephemeral. Harness state is durable. Each delivery should
include only the bounded context needed for that turn: task objective,
acceptance criteria, relevant messages, evidence refs, skill refs, owned paths,
workspace refs, and permission profile.

Delivery queues must be built from the latest projection of mutable objects.
For an append-only store, this means selecting the latest row per `Message.id`
before checking `delivery_status=queued`. Raw historical rows are audit data,
not deliverable work.

Delivery correctness also requires a claim/lease before provider side effects.
Starting a runtime, creating a provider thread, or sending provider input can
change external state. A provider implementation must not perform those effects
until it has atomically claimed the latest queued message or recorded an
equivalent recoverable lease. The claim must be visible to later dispatchers
and to the Dashboard.

Closed, closing, or retired members cannot be revived by delivery. A provider
may expose an explicit reopen operation later, but normal message delivery and
runtime start must fail visibly for those states.

The delivered provider input must carry a stable harness envelope containing at
least message id, kind, task id, sender, recipient, channel, delivery attempt,
and content. Provider-specific transcript text is not a substitute for this
correlation envelope.

## Provider-Specific Docs

Use this split:

```text
docs/agent-integration-model.md  # how to integrate a new agent (three pillars + launch spec)
docs/agent-runtime.md        # provider-neutral A-ROM and interfaces
docs/integration/README.md   # integration rules and template
docs/integration/codex.md    # Codex implementation
docs/integration/<name>.md   # future provider implementation
```

The [Agent Integration Model](agent-integration-model.md) is the canonical
"to integrate a new agent you define X, Y, Z" doc; this file is the runtime
object model it builds on. Do not let the first provider implementation define
the generic runtime.

## Invariants

1. Harness store is canonical; provider transcript is evidence.
2. Hooks and provider notifications are event inputs, not the message bus.
3. A runtime can fail while the member identity remains recoverable.
4. Provider-native subagents are visible child threads, not harness members
   unless explicitly promoted.
5. Dashboard reads normalized harness state, not raw provider state directly.
6. Delivery claims happen before provider side effects.
7. Closed, closing, and retired members fail normal delivery.

## Real-Time Event Streaming (SSE)

The harness serves real-time events via Server-Sent Events (SSE) at the `/v1/events` endpoint. This allows clients to maintain a live view of harness state without polling.

### Endpoint: `GET /v1/events`

**Purpose**: Stream provider-neutral harness events (agent events, messages, provider sessions) to connected clients as they are recorded.

**Response Headers**:
```
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
Access-Control-Allow-Origin: *
```

### Event Kinds

The endpoint emits the following event types:

- **`snapshot`**: Initial state sent on connection (contains `generated_at` timestamp). Clients use this to initialize their state during reconnect.
- **`agent_event`**: A new `AgentEvent` was recorded (provider/runtime/hook event).
- **`message`**: A new `Message` was created or its `delivery_status` changed.
- **`provider_session`**: A new `ProviderSession` was recorded or its `status` changed.

### Event Frame Format

Each event is transmitted as:
```
event: <event_kind>
data: <JSON object>

```

Example (agent_event):
```
event: agent_event
data: {"id":"evt-001","agent_member_id":"mem-001","provider":"claude","event_type":"message_queued",...}

```

### Keepalive

The connection sends a keepalive comment every ~15 seconds (when no events are being transmitted) to prevent proxy/client idle timeouts:

```
: keepalive

```

### Client Behavior

1. On connection: receive `snapshot` event to initialize state.
2. Stream in events as they arrive (typical latency <1s from append).
3. On reconnect: fetch `/v1/snapshot` to resync, then reconnect to `/v1/events`.
4. Handle client disconnect gracefully (connection drop, drop receiver).

### Implementation

The watcher thread monitors jsonl store files (`agent_events.jsonl`, `messages.jsonl`, `provider_sessions.jsonl`) for appends. On detection (~500ms poll), new records are parsed and broadcast to all connected SSE clients via a crossbeam channel fan-out. Each client connection receives events independently.

### How A Member Looks Live

The end-to-end model of how these events, the four-layer `runtime_health`
probe, and the `ProviderSession` lifecycle compose into an `AgentMember`'s
real-time state — and how that state reaches the Agent Dashboard — is the
canonical contract in
[member-runtime-observability.md](member-runtime-observability.md).
