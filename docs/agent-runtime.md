# Provider Runtime Contract

This implementation reference defines the provider-neutral runtime substrate
shared by Host execution, Agent Team members, Dynamic Workflow steps and future
Standing Agent operation. Provider-specific files under `docs/integration/`
explain how a concrete provider implements the substrate.

The provider-neutral rule for continuous, multi-cycle Member execution lives
in [Member Continuation Model](member-continuation-model.md). Runtime lifecycle,
mail delivery, and continuation are related but separate contracts.

Provider records are execution infrastructure. They do not own company
identity, organization authority, WorkItem responsibility, Mission/Wave
acceptance or business results. The owning executor and product systems keep
those truths.

## Vision Link

The product needs provider turns that can be launched, correlated, observed,
resumed and closed. A provider turn is useful only after the Harness can relate
it to the executor or Host that requested it. Harness references the
provider-native session without copying its transcript or activity stream and
without inventing lifecycle control.

Final acceptance for this mechanism:

```text
select Mission-linked execution, Host-plan context, or direct WorkItem action
  -> start or resume AgentRuntime
  -> deliver bounded request / executor-native assignment
  -> bind provider-native session
  -> project native activity on demand
  -> promote explicit outcome, artifacts, checks and optional attribution
  -> close or recover runtime
```

## Key Questions

| Question | Runtime answer |
| --- | --- |
| What requested execution? | Mission-linked run, Host action, Dynamic Workflow invocation, or linked WorkItem execution reference. |
| Who or what is acting? | A run-scoped member, Host, optional Standing Agent link, human/service actor or external provider identity. |
| What is running? | `AgentRuntime` process/session/control endpoint and health. |
| What did the provider do? | Provider-native session via `NativeSessionRef`; ephemeral adapter projection for UI. |
| How does a member receive work? | A correlated Assignment and member Inbox are projected into provider turns by `MessageDelivery`. |
| Who starts the next provider cycle? | The Member's selected `execution_driver`: Harness (`host_driven`) or one reviewed native continuation controller (`provider_driven`). |
| Who decides the work is accepted? | The Host, using the Assignment completion policy and evidence; provider completion is only an execution signal. |
| What happens when busy? | Harness-owned queue policy decides enqueue, interrupt, reject, or fail. |
| How is context built? | Harness packages bounded execution context, artifact refs, skill refs and permissions per delivery. |
| How are providers swapped? | Providers implement the same interfaces and cannot own harness state. |

## A-ROM Objects

| Object | Owns | Refuses |
| --- | --- | --- |
| `AgentMember` | compatibility/runtime configuration for an addressable agent; may be explicitly linked to a Standing Agent or MemberRun | automatic company identity, organization authority, or provider transcript as identity |
| `AgentRuntime` | lifecycle, pid/socket/control endpoint, protocol and delivery health | WorkItem, assignment, or acceptance ownership |
| `MessageDelivery` | delivery request to provider correlation and terminal delivery state | assignment ownership outside the selected executor |
| `TeamSupervisorLease` | single cross-process owner generation for TeamRun controls and delivery claims | provider transcript or proof that an uncertain claim was consumed |
| `NativeSessionRef` | mode-aware provider session identity, availability, version, and resume capability | transcript or event copy |
| `NativeContinuationProjection` | ephemeral observation of the selected provider's continuation condition, state, cycle and terminal reason | durable Goal identity, Assignment ownership, or Host acceptance |
| `AgentEvent` | explicit Harness-owned lifecycle, control, and summary facts | provider transcript, tool stream, or turn history |
| `ProviderChildThread` | provider-native subagent or child thread visibility | durable harness member identity by default |
| `PermissionProfile` | allowed tools, approval policy, sandbox, live/destructive boundaries | prompt-only safety |
| `WorkspaceRef` | cwd, worktree, branch, environment, owned paths | implicit global workspace |

## Agent Team Collaboration Boundary

An Agent Team member is an accountable, multi-turn actor with a stable
`MemberRun`, Workspace, mailbox address, Assignment correlation, and
provider-native session. Its provider-native subagents are child execution
threads, not additional Harness members. The parent member retains permission,
evidence, and acceptance responsibility.

Harness owns ordinary coordination through `TeamMessage`. The preferred new
write model is deliberately small: `assignment`, ordinary `message`, and
`handoff`; `control` is reserved for real steer/interrupt/resume protocols.
Question, answer, progress, blocker, plan, review, and peer coordination are
ordinary message intents, not lifecycle objects. Historical specialized kinds
remain readable but are read-only on new public writes. Members may send
ordinary messages to the Host or direct peer
messages to active members in the same TeamRun. Member-to-Host messages
are delivered when appended because the control plane already received them.
Messages addressed to a member remain queued until the current Supervisor
claims them and the adapter returns a provider-native acceptance receipt for
the selected MemberRun and native session. The adapter must poll or
subscribe independently of provider turn completion; busy modes that cannot
inject safely keep mail visibly queued for the next turn.

New writes carry typed actor provenance. An unbound MCP connection may author
only as the Host, an Operator, or a Service; it cannot select `member_run` or
`agent_member` by id. Member-originated messages come from that Member's bound
Provider runtime.

The member Inbox is a latest-row projection over messages addressed to that
MemberRun. Its default view contains actionable queued/delivered coordination;
the historical view contains the complete same-run coordination lineage. It
does not read or copy provider-native chat.

`PendingInteraction` is reserved for a provider turn actually paused on a
question or approval. It is not a replacement for ordinary peer or Host chat,
including planning discussion. Steer, interrupt, and resume must reflect the real selected execution
mode: unsupported live Steer fails. The caller may separately choose an
ordinary queued next-round Message; Harness never silently converts it or emits
a fake current-turn ACK.

## Provider Interfaces

This is the provider-neutral interface model, not a claim that every operation
is one public Rust trait today. Implemented V1 native reads return a bounded
projection with `truncated`; cursor pagination remains an extension documented
in [integration/native-session-storage.md](integration/native-session-storage.md).

```text
AgentProvider
  create_runtime(actor_config, workspace, permissions)
  close_runtime(runtime)
  health(runtime)
  deliver(request, context)
  interrupt(runtime, reason)
  bind_native_session(launch_receipt)
  read_native_session(session_ref) -> bounded projection
  resume_native_session(session_ref, input)

ContinuationController
  capabilities(mode, version)
  start_or_replace(session_ref, condition)
  inspect(session_ref) -> NativeContinuationProjection
  inject_or_queue(session_ref, message)
  interrupt_current_cycle(session_ref, reason)
  clear(session_ref)

Delivery
  package_context(request, execution_refs, artifact_refs, skill_refs, permissions)
  send(provider_request)
  correlate_response(response_or_event)
  record_delivery(status, native_session_ref)

NativeActivityProjector
  provider-native record -> ephemeral sanitized projection
  provider interaction boundary -> PendingInteraction / control acknowledgement
  explicit promotion -> handoff / outcome / artifact or check ref

WorkspaceProvider
  prepare_workspace(execution)
  attach_branch_or_pr(execution)
  inspect_changed_paths(execution)
  cleanup_or_archive(execution)
```

Codex, Claude Code, Kimi, OpenClaw, a Permission Agent, or a future cloud
provider should implement these boundaries without changing Mission/Wave,
executor-native records, TeamMessage, PendingInteraction, outcome, artifact,
WorkItem, Approval, gate, or organization semantics.

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

The selected execution driver owns cycle creation. In `host_driven` mode,
eligible mail causes Harness to start exactly one next provider cycle. In
`provider_driven` mode, Harness may queue or inject mail according to the
reviewed native protocol, but it must not independently start a competing
top-level cycle. The lease is scoped to one MemberRun, native session, and
writable Workspace; it is not a claim that a Member can perform only one turn.

Team `max_concurrency` applies to active execution leases, not Member
supervisors. An idle Member retains its native session, mailbox and Host control
handle without occupying a provider-turn permit.

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
docs/member-continuation-model.md
                             # execution-driver, completion and native continuation contract
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

1. Harness store is canonical for coordination; the provider-native session is
   canonical for per-agent transcript, activity, turn lifecycle, and resume.
2. Hooks and provider notifications are event inputs, not assignment ownership.
3. A runtime can fail while the member identity remains recoverable.
4. Provider-native subagents are visible child threads, not harness members
   unless explicitly promoted.
5. Dashboard joins normalized Harness coordination with provider-adapter native
   session projections; browser code does not read private provider files
   directly and Harness does not mirror them.
6. Delivery claims happen before provider side effects.
7. Closed, closing, and retired members fail normal delivery.

## Real-Time Event Streaming (SSE)

The harness serves real-time events via Server-Sent Events (SSE) at the `/v1/events` endpoint. This allows clients to maintain a live view of harness state without polling.

### Endpoint: `GET /v1/events`

**Purpose**: Stream Harness coordination/lifecycle changes plus transient native
activity projections to connected clients. The stream is project-scoped:
`?project=<id>` selects the project; frames from other projects never leak.

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
- **`workflow_run`** / **`workflow_step`**: A `WorkflowRun` / `WorkflowStep` record was appended or updated (dynamic workflow runtime).
- **`native_activity`**: Ephemeral provider-native projection when the selected
  adapter/mode supports live publication; reconnect re-reads native state.

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

The watcher monitors Harness-owned project JSONL files. Provider adapters
publish ephemeral native projections and support on-demand reconstruction from
`NativeSessionRef`.

### How A Member Looks Live

The end-to-end model of how these events, the four-layer `runtime_health`
probe, `MessageDelivery`, and the native session binding compose into an `AgentMember`'s
real-time state — and how that state reaches the Agent Dashboard — is the
canonical contract in
[member-runtime-observability.md](member-runtime-observability.md).
