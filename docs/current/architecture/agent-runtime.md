# Node Runtime and Message Fabric

```text
status: accepted implementation contract
authority: AF-ADR-011
canonical_for: Agent identity/session separation, NodeDaemon runtime ownership,
  messaging, provider dispatch, runtime control, recovery, and provider parity
```

Provider runtime is machine-local infrastructure. It does not own AgentMember
identity, Team membership, Work responsibility, acceptance, or provider-native
transcripts. One machine-scoped NodeDaemon owns every local AgentSession,
provider process/thread, provider delivery, and runtime-control effect across
the machine's registered Execution Spaces.

Lease renewal is independent of Execution Space discovery. The NodeDaemon
heartbeats only the exact machine authorities it already owns while the
discovery loop scans registered Spaces; it never acquires or steals authority
through that heartbeat path. A slow or unhealthy Space therefore cannot let an
otherwise live daemon's lease expire underneath an attached AgentSession. A
heartbeat failure stops the daemon and uses the normal drain/release path;
lease expiry alone is never treated as a provider-drain receipt.

## Canonical separation

```text
AgentMember
  ├─ TeamMembership (participation in one flat Team)
  └─ AgentSession (machine-local provider runtime)

Work revision
  └─ WorkExecutionBinding
       └─ exact TeamMembership + AgentMember + AgentSession generation

Message
  └─ MessageSubscription
       └─ CanonicalMessageDelivery per recipient

NodeDaemon
  └─ RuntimeCommand
       └─ ProviderInvocation / provider effect
```

The objects are intentionally independent:

| Object | Owns | Never owns |
| --- | --- | --- |
| `AgentMember` | the sole durable addressable agent identity and organization status | provider process, Team membership, Work, transcript |
| `AgentSession` | one provider session on one Node and exact NodeDaemon generation, hanging off its AgentMember | Team identity, Work acceptance |
| `TeamMembership` | one AgentMember's active participation in one flat Team | identity, provider lifecycle |
| `WorkExecutionBinding` | one exact Work revision bound to one membership and AgentSession generation | authored conversation |
| `Message` | immutable identity-authored, source-NodeDaemon-attested conversation | Work ownership or runtime control |
| `MessageSubscription` | authorized recipient policy and delivery mode | a second Message or browser-chosen recipient truth |
| `CanonicalMessageDelivery` | per-recipient queue, claim, provider receipt and ACK/cursor state | provider transcript |
| `RuntimeCommand` | crash-recoverable prepare/settle journal for provider/process effects | conversation or Work state |
| `ProviderInvocation` | target-NodeDaemon-built provider input derived from a claimed delivery | public mutation authority |

Current DEV-31 schema work adds bounded runtime-control facts to
`AgentSession.control_state`: runtime residency, runtime activity,
execution-driver class, driver generation/ref, handoff state, native
continuation projection/activation, composition fingerprint, capability
fingerprint, and last reconciliation time. These fields are control fences and
projections only; they do not mirror native turns, tool calls, commands,
files, transcript, or provider reasoning.

The `AgentIdentity` name is a deprecated same-ID read-only compatibility
projection of `AgentMember`: legacy readers resolve the same row, and nothing
may be bound to it as a second identity root.

## Team Host runtime

[ADR 0057](../../decisions/0057-host-is-an-agent-member.md) closes the former
Host/Member execution split. The Host is the exact `AgentMember` named by
`AgentTeam.host_agent_id` and its one active `TeamMembership(role=host)`.
Every current TeamRun resolves one Host MemberRun.

In the default `managed` mode, that MemberRun uses the same AgentSession,
NodeDaemon, RuntimeCommand, provider admission, TeamRuntimeAdapter, Message
claim/receipt/ACK, and Close/Reopen lifecycle as every other managed
participant. Host authority is role policy, not a provider feature and not a
second runtime species. Its coordination AgentSession is `ReadOnly` whenever
the provider can prove that ceiling. Kimi ACP cannot, so a managed Kimi Host
retains an honestly declared `FullAccess` ceiling only when
`provider_cwd_hint` selects an explicit workspace distinct from the Team
execution root; otherwise admission fails before AgentSession materialization.
A Host that owns coding Work always needs a separate, independently leased
workspace.
Managed Host status delivery additionally carries the
exact recipient MemberRun, AgentSession/runtime generation, and NodeDaemon
generation fence before a provider receipt can settle it.

`external_interactive` is an explicit user-driven exception. It keeps the same
Host AgentMember and business authority but has only a detached MemberRun and
durable pull-based inbox. Harness performs no provider admission or turn and
creates no AgentSession, RuntimeCommand, or native-session record for it; it
cannot claim timely wake, provider receipt, or ACK. Historical `external` values decode to
this mode without fabricating managed evidence, and there is no silent fallback
between modes.

Work events, Messages, and runtime/recovery attentions remain independent
canonical planes. The daemon may batch them into the next Host cycle, but
delivery does not authorize Work mutation and provider completion does not
mean Host acceptance. Ordinary progress is batched; decisions, blocked Work,
submission, direct Messages, and recovery facts can wake an idle managed Host.
Host-authored status updates do not recursively wake that same Host.

`TeamRun` and `MemberRun` remain internal diagnostics and history
projections. They are not provider runtime authority and never scope durable
identity or Work responsibility. No CLI, HTTP, MCP,
Dashboard, adapter, or mutable Store seam may dispatch, resume, interrupt, or
stop a provider through them.

## Authority flow

### Same-node messaging

1. The authenticated source AgentSession sends an authoring RuntimeCommand to
   its current NodeDaemon.
2. The source NodeDaemon freezes sender identity/session, immutable content,
   sequence, Team/Work relation, recipients, and content fingerprint.
3. Canonical subscriptions produce one delivery per authorized recipient.
4. The target NodeDaemon claims the delivery for the exact current recipient
   AgentSession generation.
5. Only after the durable claim does it build a `ProviderInvocation` and touch
   the provider.
6. Provider receipt and recipient ACK/cursor are separate durable facts.

The source and target may be the same NodeDaemon. That does not allow a second
Message, sequence, or delivery authority.

### Cross-node messaging

The source NodeDaemon remains the only Message author. The Control Plane owns
only a route journal for the immutable Message id and source fingerprint. The
target NodeDaemon owns recipient delivery and provider state. Routing may
retry, but it may not rewrite content, allocate a second source sequence, or
fold target delivery into Control Plane truth.

### Work delivery

Work and Message are separate planes. `CanonicalWorkDelivery` carries an exact
Work id/revision to an active `WorkExecutionBinding`. Claiming Work verifies the
current membership, AgentMember, AgentSession generation, Node placement,
NodeDaemon lease, and Work owner under canonical Store authority. Work result,
progress, finding, failure, revise, submit, gate, and acceptance remain Work
operations.

### Runtime control

Start, resume, turn, queued input, interrupt, and stop all use the same durable
RuntimeCommand protocol:

```text
authenticate and resolve authority
  -> bind exact Node + NodeDaemon generation
  -> bind exact AgentSession generation and permission ceiling
  -> validate full command fingerprint and idempotency key
  -> persist Accepted / effect=Unknown
  -> touch process/provider
  -> persist Applied, Failed/NotApplied, or RecoveryRequired/Unknown
```

DEV-31 tightens this into an exact binding fence for every provider/process
effect: the prepared command records the target AgentSession id, runtime
generation, execution-driver generation/ref, NativeSessionRef, permission
envelope, composition fingerprint, capability fingerprint, preconditions, and
postconditions. A command whose binding cannot be proven is rejected before the
provider boundary. A provider ACK proves only transport acceptance; terminal,
quiesce, release, and semantic postconditions require adapter observation
evidence and are tracked separately from provider effect certainty.

Exact replay returns the original durable result and never repeats the effect.
The same key with a changed provider, mode, payload, permission, Node, Space,
Session, or generation fails with an idempotency conflict. Authorization and
generation rejection have zero canonical ledger, provider, process, Message,
and delivery side effects.

The public runtime-command route accepts only closed semantic intents. It does
not accept caller-selected capabilities, permission envelopes, provider
profiles, or complete AgentSession payloads. For session control, the server
resolves exact self or the exact machine Operator/NodeDaemon. Team Host
authority is Team-scoped coordination authority and never controls the global
machine Session. StartSession derives the local Node and active project
registration independently of TeamMembership and enforces the frozen
AgentMember permission ceiling under the same Store lock before any session,
command, process, or provider side effect. Team join/leave does not create,
resume, or close a Session. Likewise, Team `close-member` closes only that
MemberRun generation and cancels its current provider turn; it leaves the
machine-owned AgentSession available and never releases or rewrites Work
bindings from this or another Team.

## Effect certainty and recovery

| Observation | Durable result | Retry rule |
| --- | --- | --- |
| failure proven before provider/process boundary | `Failed / NotApplied` | a new command may be issued intentionally |
| provider/process effect observed complete | `Applied / Applied` | exact replay returns completion |
| socket loss, timeout, callback race, or torn state after the boundary | `RecoveryRequired / Unknown` | no automatic repeat; reconcile first |
| stale NodeDaemon or AgentSession generation | typed fenced error | zero side effects |

`effect_certainty` and semantic `postcondition_status` are distinct. For
example, a native interrupt frame may be sent (`Applied`) while terminal
settlement is still `Unknown` until the adapter observes the provider's settled
boundary. RuntimeCommand is the only durable provider-effect journal; DEV-31
does not introduce a second event ledger, PermissionRequest object, or
PermissionDecision object.

Canonical operation rows are recovered atomically. A torn prepared or settled
tail must yield the last complete operation, never an unreadable ledger or a
fabricated completion. A successor NodeDaemon or AgentSession cannot settle an
older generation's command. Every active WorkExecutionBinding that references
a Session must be explicitly released, rebound, or quiesced before StopSession;
rejection changes neither the Session, binding, command journal, nor provider
process.

`RecoveryRequired / Unknown` is visible in the exact Node Operator RoleView.
Resolution is a critical confirmed action bound to command version, Node,
NodeDaemon and AgentSession generation, authority, and idempotency fingerprint.
The Operator may record evidence that the effect was applied, was not applied,
or remains unknown; resolution never blindly repeats the native effect.

## Provider conformance

Codex, Claude, Kimi, and Pi expose separate, closed capability tuples:

- requested permission must fit both the AgentMember ceiling and provider
  adapter capability, and the ceiling must be verifiably enforced — the
  adapter names its `security_enforcement_locus` in the provider profile
  (provider-native policy, adapter tool allowlist, adapter auto-approval, OS
  sandbox, network/credential boundary, or honestly `none_verified`);
- safe current-turn injection requires both adapter support and an observed
  safe point;
- unsupported or unprovable permission, queue, interrupt, resume, or stop
  behavior fails closed;
- every interrupt/close path crosses one closed executable provider-adapter
  seam: it freezes the native plan, durably prepares RuntimeCommand, performs
  the actual native control, waits for terminal acknowledgement, then settles;
  exact replay cannot repeat the native effect, and ambiguous dispatch becomes
  `RecoveryRequired`;
- one table-driven faithful-shim harness applies that lifecycle to Codex,
  Claude, Kimi, and Pi, including permission mapping, safe-injection downgrade,
  terminal acknowledgement, replay, and recovery. It is contract evidence,
  not a claim that an unavailable provider passed a live run;
- the persistent Team member loop is provider-neutral: the monotonic round
  progression lives in `firm-runtime-supervisor` over an application port,
  while `firm-runtime-contract` owns the provider-facing lifecycle language.
  Wake → claim → ExecutionCycle → settle is shared, and each provider package
  compiles the semantic intents
  (open/resume, start cycle, inject current cycle, queue at native boundary,
  interrupt, continuation inspection/control, narrow Team Close, strong
  quiesce, and release) into provider primitives with an executable per-intent
  capability report. Pi, Codex app-server, Claude Agent SDK, and Kimi ACP all
  enters through this shared loop; `firm-provider-{codex,claude,kimi,pi}` own
  native transport, observation, permission mapping, and exact-version
  receipts, while application composition owns Work/Message/RuntimeCommand
  preparation and settlement;
- Team `CloseRuntime` and strong runtime replacement are deliberately separate
  intents. `CloseRuntime` terminates and reaps the Harness-owned provider
  handle, freezes the Member mailbox, and retains the native session id for an
  explicit higher-generation Reopen. Strong `quiesce`/`release` additionally
  require every adapter to prove continuation inhibition, current-cycle
  terminal state, native queue settlement, writable-child drain, idle
  observation, and durable native flush. A provider that cannot observe one of
  those postconditions remains degraded and fails closed; a process exit or
  session-close ACK never fills in missing evidence;
- DeepSeek is not a managed production provider in this contract. Any current
  DeepSeek harness work is treated as a faithful conformance shim/table-driven
  test surface until a reviewed native adapter, exact version, capability
  evidence, and live acceptance exist;
- Codex has a proven NodeDaemon-owned app-server start/resume/stop path;
  standalone cancel remains disabled until a native turn is bound;
- Claude, Kimi, and Pi remain disabled for standalone AgentSession lifecycle
  until their NodeDaemon-owned handles are executable; their existing Team
  transports do not imply global Session conformance;
- provider binary/version availability is probed explicitly; installation alone
  does not grant a capability, and an unavailable or unprovable provider is
  disabled rather than reported as conformance PASS;
- Team RoleViews project exact-version compatibility and executable capability
  admission as separate facts. An idle Member counts as Ready only when its
  provider tuple is current and the core `open_or_resume`, `start_cycle`, and
  `observe` bindings are all `verified/active`; protocol support or a passing
  deterministic shim without exact live evidence cannot make that row Ready;
- the browser cannot declare provider compatibility, permission, current turn,
  or effect success;
- provider transcripts, tool calls, commands, files, reasoning, and child
  threads remain in native provider storage unless explicitly promoted as a
  result/evidence reference.

Provider adapters consume only canonical claimed `CanonicalMessageDelivery` or
WorkDelivery plus a NodeDaemon-built `ProviderInvocation`. The retired
`ProviderDispatchEnvelope` and the ledgers from the development batch
historically named “Wave 4A” have no current
writer, reader, fallback, migration, SSE, RoleView, Dashboard, CLI, HTTP, MCP,
or adapter authority. A narrowly enumerated historical export may remain
read-only and is excluded from current projections and migration.

## Product views and clients

## Remote Node Fabric

One logical Fabric Control Plane coordinates machines, while each machine has
exactly one `ExecutionNode` identity and one current NodeDaemonLease. A
NodeGatewayLease is only a child of the exact current NodeDaemonLease
generation and never a second machine authority. The former `CompanyNode`
name is retired (DOC-108); it was always the same row, under the rule
`CompanyNode.id == ExecutionNode.id`.
Nodes initiate outbound TLS 1.3 mutual-authenticated WSS connections to the
Control Plane. They do not expose an inbound collaboration listener and do not
connect directly to sibling Nodes.

`FabricStore` operations, attempts and receipts are the sole cross-Node route
truth. A cross-Node `MessageRouteJournal` may exist only as a read-only
projection. It is not written in parallel and cannot drive replay, delivery or
application claims. A `RouteAttempt` proves transport only. Application effect
is `none | not_applied | applied | unknown` and only a generation-fenced target
result/receipt may assert it.

A routed message carries either its complete canonical immutable Message
envelope or an authenticated content-addressed reference. The target verifies
and persists that exact Message before creating the existing per-recipient
`CanonicalMessageDelivery`.
A routed RuntimeCommand carries the complete canonical command envelope; the
target resolves it through the existing NodeDaemon service and derives the
terminal effect from the canonical RuntimeCommand record. Fabric never becomes
a second Message, Delivery, RuntimeCommand, Work or provider-session Store.

Source authority is closed to `node | control_plane`; canonical bytes are
versioned. Exact replay is fingerprint-bound. Unknown effect, stale gateway or
NodeDaemon generation, wrong Company/Node/Execution Space, and incompatible
schema/protocol/capability all fail closed without business or provider side
effects. See `docs/current/architecture/remote-node-fabric.md` for the complete
contract and `docs/current/operations/remote-fabric-operations.md` for the
operator procedure.

Server-built RoleViews project current canonical state. Browsers refetch after
SSE invalidation; they do not fold raw ledgers or invent lifecycle truth.
Current inboxes use `CanonicalMessageDelivery` and `SubscriptionCursor`. Current
runtime state uses AgentSession and RuntimeCommand. Historical TeamRun,
MemberRun, native-session locator, and legacy export rows are labeled history
and cannot enable actions.

CLI, HTTP, MCP, Dashboard, skills, and plugin mirrors must expose only actions
the server can bind to authenticated identity, authority, target, exact version,
idempotency, confirmation, and current Node/Session generations. Retired
message and runtime mutation routes fail closed with typed errors.

## Acceptance boundary

Release requires:

- deterministic start/resume/turn/input/interrupt/stop replay and recovery
  tests, including terminal, socket-loss, callback-race, torn-row, and
  successor-generation cases;
- real Host→Team/Member and Member→Host/Team Message journeys with
  subscriptions, per-recipient delivery, provider receipt, ACK/cursor, and
  sibling Team/Node/Space negatives;
- Codex/Claude/Kimi/Pi permission and queue/current-turn conformance;
- executable native control conformance for all four adapters, explicit
  unavailable-provider negatives, and real provider-backed dogfood for each
  provider available in the release environment;
- executable zero-match governance for retired runtime/message authorities;
- populated RoleView and live provider/message acceptance;
- full Rust, formatting, clippy, repository governance, docs/plugin mirror,
  and fresh clean-archive gates.

The development batch historically named “Wave 5” must consume these
server-built projections and disabled reasons; it may not reconstruct runtime
state in the client. The Wave 6 development-batch dogfood must prove the
multi-provider Message/Work/RuntimeCommand journeys and recovery contracts on
real Company work before widening permissions or topology.
