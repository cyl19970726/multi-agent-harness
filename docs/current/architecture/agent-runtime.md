# Node Runtime and Message Fabric

```text
status: accepted implementation contract
authority: AF-ADR-011
canonical_for: Agent identity/session separation, NodeDaemon runtime ownership,
  messaging, provider dispatch, runtime control, recovery, and provider parity
```

Provider runtime is machine-local infrastructure. It does not own Company
identity, Team membership, Work responsibility, acceptance, or provider-native
transcripts. One machine-scoped NodeDaemon owns every local AgentSession,
provider process/thread, provider delivery, and runtime-control effect across
the machine's registered Execution Spaces.

## Canonical separation

```text
AgentIdentity
  ├─ TeamMembership (collaboration overlay)
  └─ AgentSession (machine-local provider runtime)

Work revision
  └─ WorkExecutionBinding
       └─ exact TeamMembership + AgentIdentity + AgentSession generation

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
| `AgentIdentity` | stable addressable agent identity and organization status | provider process, Team membership, Work, transcript |
| `AgentSession` | one provider session on one Node and exact NodeDaemon generation | Team identity, Work acceptance |
| `TeamMembership` | one AgentIdentity's active participation in one flat Team | provider lifecycle |
| `WorkExecutionBinding` | one exact Work revision bound to one membership and AgentSession generation | authored conversation |
| `Message` | immutable source-authored conversation | Work ownership or runtime control |
| `MessageSubscription` | authorized recipient policy and delivery mode | a second Message or browser-chosen recipient truth |
| `CanonicalMessageDelivery` | per-recipient queue, claim, provider receipt and ACK/cursor state | provider transcript |
| `RuntimeCommand` | crash-recoverable prepare/settle journal for provider/process effects | conversation or Work state |
| `ProviderInvocation` | target-NodeDaemon-built provider input derived from a claimed delivery | public mutation authority |

`TeamRun` and `MemberRun` may remain as coordination and historical
projections. They are not provider runtime authority. No CLI, HTTP, MCP,
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
current membership, AgentIdentity, AgentSession generation, Node placement,
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
AgentIdentity permission ceiling under the same Store lock before any session,
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

- requested permission must fit both the AgentIdentity ceiling and provider
  adapter capability;
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
- Codex has a proven NodeDaemon-owned app-server start/resume/stop path;
  standalone cancel remains disabled until a native turn is bound;
- Claude, Kimi, and Pi remain disabled for standalone AgentSession lifecycle
  until their NodeDaemon-owned handles are executable; their existing Team
  transports do not imply global Session conformance;
- provider binary/version availability is probed explicitly; installation alone
  does not grant a capability, and an unavailable or unprovable provider is
  disabled rather than reported as conformance PASS;
- the browser cannot declare provider compatibility, permission, current turn,
  or effect success;
- provider transcripts, tool calls, commands, files, reasoning, and child
  threads remain in native provider storage unless explicitly promoted as a
  result/evidence reference.

Provider adapters consume only canonical claimed MessageDelivery or
WorkDelivery plus a NodeDaemon-built `ProviderInvocation`. The retired
`ProviderDispatchEnvelope` and Wave4A Team message ledgers have no current
writer, reader, fallback, migration, SSE, RoleView, Dashboard, CLI, HTTP, MCP,
or adapter authority. A narrowly enumerated historical export may remain
read-only and is excluded from current projections and migration.

## Product views and clients

Server-built RoleViews project current canonical state. Browsers refetch after
SSE invalidation; they do not fold raw ledgers or invent lifecycle truth.
Current inboxes use canonical MessageDelivery and SubscriptionCursor. Current
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

Wave 5 must consume these server-built projections and disabled reasons; it may
not reconstruct runtime state in the client. Wave 6 dogfood must prove the
multi-provider Message/Work/RuntimeCommand journeys and recovery contracts on
real Company work before widening permissions or topology.
