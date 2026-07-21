# Provider Runtime Contract

This implementation reference defines the provider-neutral runtime substrate
shared by Host execution, Agent Team members, Dynamic Workflow steps and future
Standing Agent operation. Provider-specific files under `docs/integration/`
explain how a concrete provider implements the substrate.

Provider records are execution infrastructure. They do not own company
identity, organization authority, WorkItem responsibility, Mission/Wave
acceptance or business results. The owning executor and product systems keep
those truths.

## Vision Link

The product needs provider turns that can be launched, correlated, observed,
resumed and closed. A provider turn is useful only after the Harness can relate
it to the executor or Host that requested it and preserve its observable output
without inventing lifecycle control.

Final acceptance for this mechanism:

```text
select Mission/Wave executor or direct WorkItem action
  -> start or resume AgentRuntime
  -> deliver bounded request / executor-native assignment
  -> record delivery and provider session
  -> reduce provider events into harness state
  -> return outcome, artifacts, checks and optional attribution
  -> close or recover runtime
```

## Key Questions

| Question | Runtime answer |
| --- | --- |
| What requested execution? | Mission/Wave executor, Host action or linked WorkItem execution reference. |
| Who or what is acting? | A run-scoped member, Host, optional Standing Agent link, human/service actor or external provider identity. |
| What is running? | `AgentRuntime` process/session/control endpoint and health. |
| What did the provider do? | `ProviderSession` plus `AgentEvent` stream. |
| How does it receive work? | Delivery maps the requesting executor's assignment or Host request to provider input. |
| What happens when busy? | Harness-owned queue policy decides enqueue, interrupt, reject, or fail. |
| How is context built? | Harness packages bounded execution context, artifact refs, skill refs and permissions per delivery. |
| How are providers swapped? | Providers implement the same interfaces and cannot own harness state. |

## A-ROM Objects

| Object | Owns | Refuses |
| --- | --- | --- |
| `AgentMember` | compatibility/runtime configuration for an addressable agent; may be explicitly linked to a Standing Agent or MemberRun | automatic company identity or organization authority |
| `AgentRuntime` | lifecycle, pid/socket/control endpoint, protocol and delivery health | WorkItem, assignment or acceptance ownership |
| `MessageDelivery` | delivery request to provider correlation and terminal delivery state | assignment ownership outside the selected executor |
| `ProviderSession` | one provider interaction and reproducible request/output refs | canonical WorkItem or Wave state |
| `AgentEvent` | normalized provider/runtime/hook events | raw provider-specific semantics |
| `ProviderChildThread` | provider-native subagent or child thread visibility | durable harness member identity by default |
| `PermissionProfile` | allowed tools, approval policy, sandbox, live/destructive boundaries | prompt-only safety |
| `WorkspaceRef` | cwd, worktree, branch, environment, owned paths | implicit global workspace |

## Provider Interfaces

```text
AgentProvider
  create_runtime(actor_config, workspace, permissions)
  close_runtime(runtime)
  health(runtime)
  deliver(request, context)
  interrupt(runtime, reason)
  read_events(runtime, cursor)

Delivery
  package_context(request, execution_refs, artifact_refs, skill_refs, permissions)
  send(provider_request)
  correlate_response(response_or_event)
  record_delivery(status, provider_session)

EventReducer
  provider_event -> AgentEvent
  AgentEvent -> runtime health and executor-specific projections

WorkspaceProvider
  prepare_workspace(execution)
  attach_branch_or_pr(execution)
  inspect_changed_paths(execution)
  cleanup_or_archive(execution)
```

Codex, Claude Code, Kimi, OpenClaw, a Permission Agent, or a future cloud
provider should implement these boundaries without changing Mission/Wave,
executor-native records, WorkItem, Approval or organization semantics.

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
include only the bounded context needed for that turn: objective, acceptance
criteria, relevant executor-native assignments/messages, artifact refs, skill
refs, owned paths, workspace refs, permission profile and necessary Company OS
links.

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

The delivered provider input must carry a stable Harness envelope containing
the requesting Mission/Wave/run or WorkItem reference, sender, recipient,
delivery attempt and content as applicable. Provider-specific transcript text
is not a substitute for this correlation envelope.

## Provider-Specific Docs

Use this split:

```text
docs/agent-integration-model.md  # how to integrate a new agent (three pillars + launch spec)
docs/agent-runtime.md        # provider-neutral runtime substrate and interfaces
docs/integration/README.md   # integration rules and template
docs/integration/codex.md    # Codex implementation
docs/integration/claude.md   # Claude implementation
docs/integration/kimi.md     # Kimi implementation
docs/integration/<name>.md   # future provider implementation
```

The [Agent Integration Model](agent-integration-model.md) is the canonical
"to integrate a new provider you define X, Y, Z" doc; this file is the runtime
substrate it builds on. Do not let the first provider implementation define the
generic runtime or product authority.

## Invariants

1. Executor-native and Company OS stores are canonical; provider transcript is
   an execution reference, never product authority.
2. Hooks and provider notifications are event inputs, not assignment ownership.
3. A runtime can fail while the member identity remains recoverable.
4. Provider-native subagents are visible child threads, not harness members
   unless explicitly promoted.
5. Dashboard reads normalized harness state, not raw provider state directly.
6. Delivery claims happen before provider side effects.
7. Closed, closing, and retired members fail normal delivery.

## Real-Time Event Streaming (SSE)

The harness serves real-time events via Server-Sent Events (SSE) at the `/v1/events` endpoint. This allows clients to maintain a live view of harness state without polling.

### Endpoint: `GET /v1/events`

**Purpose**: Stream provider-neutral harness events (agent events, messages, provider sessions, workflow runs/steps, live provider turn events) to connected clients as they are recorded. The stream is project-scoped: `?project=<id>` selects the project; frames from other projects never leak into a client's stream.

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
- **`workflow_run`** / **`workflow_step`**: A `WorkflowRun` / `WorkflowStep` record was appended or updated (dynamic workflow runtime).
- **`provider_turn_event`** / **`provider_turn_event_normalized`**: A raw provider turn-event line teed live to `provider_turn_events.jsonl` during delivery, and its normalized `HarnessTurnEvent` expansion.

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

The watcher thread monitors each project's jsonl store files (`agent_events.jsonl`, `messages.jsonl`, `provider_sessions.jsonl`, `workflow_runs.jsonl`, `workflow_steps.jsonl`, `provider_turn_events.jsonl`) for appends. On detection (~150ms poll), new records are parsed and broadcast via a crossbeam channel fan-out to the clients subscribed to that project. The project registry is re-scanned on every poll, so a project registered after `serve` starts gets a live event channel without a restart. Each client connection receives events independently.

### How A Member Looks Live

The end-to-end model of how these events, the four-layer `runtime_health`
probe, and the `ProviderSession` lifecycle compose into an `AgentMember`'s
real-time state — and how that state reaches the Agent Dashboard — is the
canonical contract in
[member-runtime-observability.md](member-runtime-observability.md).
