# Cross-Machine Team Collaboration

This document describes the collaboration authority shipped in the development
batch historically named “Wave 6”. Wave 4C/5/6 labels in this document identify
development batches, not a current product `Wave` object. The
product model remains simple: a Team is durable, flat, and Mission-free
(DOC-108), and every Member
of that Team executes on the Team's single immutable Node. Cross-machine work
is cooperation between Teams, never a Team split across machines.

## Authority flows

```text
responsibility:
source Team WorkApplicationService
  -> server-authored SourceWorkAttestation
  -> source NodeGateway outbox
  -> Control Plane route journal + durable WorkDelegation relationship
  -> target NodeGateway inbox
  -> target native WorkApplicationService
  -> target Work / Report / Finding / Failure
  -> immutable redacted RemoteFactPublication
  -> source Node read-only cache

conversation:
source AgentMember -> immutable Message
  -> policy/capability-fenced MessageSubscription
  -> one Team-addressed CanonicalMessageDelivery
  -> Control Plane route journal -> target NodeGateway inbox
  -> atomic TeamMembership + membership/NodeDaemon generation claim
  -> resolved AgentMember/session CanonicalMessageDelivery
```

The accepted Remote Node Fabric owns transport, attempts, receipts, ordering,
expiry and `RecoveryRequired`. One durable control-store authority owns Team,
TeamMembership, Work, Message, MessageSubscription,
CanonicalMessageDelivery, explicit WorkDelegation relationships, decisions,
cancellation requests, immutable publication metadata and cross-node
projections. The retained `company_id` protocol field is only a Fabric tenant
isolation key; it does not introduce Company or Organization product authority.
Execution Spaces locate Stores and execution resources and never authorize a
business admission.

No caller may directly write a remote Store. Every cross-node mutation becomes
one closed `collaboration.business.v1` operation using exactly one of:

- `delegation_propose`
- `delegation_decide`
- `target_work_create`
- `delegation_cancel_request`
- `delegation_cancel_decide`
- `peer_message_deliver`
- `team_message_deliver`
- `remote_fact_publish`
- `artifact_grant`

An accepted transport receipt is not a business decision. Higher-level state
is folded only after the exact terminal application receipt. An unknown effect
remains `RecoveryRequired`; it is never translated into Work failure or
completion.

## Work and placement

`TargetPlacementRef` freezes Team id, durable Team revision, immutable Node id
and `placement_generation = 1`. A machine move creates a new Team identity.
Proposal authority is proven by a source-Node WorkApplicationService
attestation over the exact current Work revision/event, Team, Host/owner and
gateway generation. Public request bodies cannot claim these facts.

Target acceptance validates the current target Host and placement before it
creates one native target Work. Target Work completion does not complete source
Work. The source Host must explicitly integrate the remote result and then use
the source Work's normal review/acceptance path.

Cancellation is also two-party. The source Host can create a durable request;
only the target Host can decide it, and an accepted cancellation must name the
actual target native Work cancellation event. Transport cancellation is never
treated as Work cancellation.

## Message and delivery

The cross-machine collaboration batch does not introduce a second Message or
delivery ledger. The source NodeDaemon authors the existing immutable current
`Message` once. An ordinary AgentMember peer Message is admitted by the current
MessageSubscription and its frozen authorization policy, policy revision,
policy digest, `collaboration.peer_message_deliver` capability, target Team and
immutable Node placement. WorkDelegation is neither required nor consulted.

Admission creates exactly one Team-addressed CanonicalMessageDelivery. It does
not select a recipient runtime, AgentSession or admission-time
TeamMembership. The route carries canonical Message bytes (or a
content-addressed immutable object reference) plus the frozen subscription and
Team/Node fence. The target atomically claims the queued delivery against one
eligible current TeamMembership, its membership generation and the current
NodeDaemon generation, then applies the resolved per-recipient delivery to the
claimed AgentMember/session. An offline recipient remains queued; a stale or
revoked policy, capability, Team placement, membership generation, NodeDaemon
generation or claim fails closed without a second Message or delivery.

Control Plane route receipts are transport evidence only. Read projections may
show cross-node state, but no projection can be acknowledged or mutated as
delivery authority.

## Facts and artifacts

The target Work-owning Node remains authority for full Reports, Findings and
Failure analyses. Publication accepts only a server-resolved canonical native
fact authored by the exact target Host or current Work owner. It creates an
immutable, redacted and digest-bound `RemoteFactSnapshot`; callers cannot
upload an arbitrary fact body. `fact_work_ref` remains byte-equal to the
Delegation's immutable target-Work creation reference, while
`native_fact_work_ref` proves the exact later Work revision and active binding
that authored the fact. The target Node derives the former from its applied
target-Work receipt; the Control Plane independently resolves the live central
Delegation before storing the publication. The source Node receives a read-only
cache.

Large/private evidence uses the accepted Wave 5 artifact service. The source
Node validates the frozen Delegation and grant before asking the Control Plane
to consume the one-use capability. Artifact bytes then travel in ordered,
digest-bound chunks, and the source Node atomically persists a source-owned
`ArtifactImport` plus independently readable bytes. A grant validation or
`OperationApplied` receipt is never an import. Retention starts only after
transport, terminal Delegation and that canonical import are all terminal;
the latest anchor wins and duration is a positive bounded server policy.
Public callers cannot submit retention anchors.

## Public and operator surfaces

- AgentFirm HTTP authors proposals and remote facts from server-resolved local
  Work truth and queues them through the NodeGateway.
- Control Plane Host REST lists/reads Delegations and publications and performs
  exact-revision Host decisions, cancellation decisions and artifact grants.
  Read projections are restricted to the exact source owner, source Host or
  target Host resolved from the authenticated credential; Fabric tenant, Team
  and Execution Space are never caller-selected authority.
- HTTP list endpoints return bounded, opaque server-signed cursors
  bound to Fabric tenant, actor, filter and a frozen Store sequence. Hidden
  sibling rows advance the raw scan but never consume a visible page.
- Harness coordination has no MCP server. Cross-machine mutations use the
  authenticated CLI or HTTP application surface; central projections remain
  read-only.
- `firm team-run message send` authenticates the exact `external_interactive`
  Host binding by surface and native thread id, then authors an intra-Team
  Message through the same application seam as
  `POST /v1/agentfirm/team-runs/{run}/messages/send`. Supervisor-bound Members
  use `firm member message send`.
- OperatorView reports Node-local outbox/inbox depth, oldest age, current
  gateway generation, Control Plane-derived health, reconcile lag and exact
  recovery inventory. Local journal presence never implies remote health.
- `firm team message send|inbox|claim` authors and reads ordinary peer-Team
  Messages through the source NodeDaemon RuntimeCommand; the shared Team Inbox
  projection is served read-only at `GET /v1/views/team-inbox/<team-id>`.
  Direct TeamMembership targets bind one
  durable delivery at admission; Team targets stay queued until one exact
  membership generation claims them.

The executable contracts are
`crates/firm-store/tests/cross_machine_collaboration.rs`,
`crates/firm-fabric/tests/fabric_contract.rs`,
`crates/firm-cli/tests/team_peer_messaging.rs`, and
`scripts/check-collaboration-foundation.mjs`.
