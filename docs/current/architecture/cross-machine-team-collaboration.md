# Cross-Machine Team Collaboration

This document describes the shipped Wave 6 collaboration authority. The
product model remains simple: one Mission owns one flat Team, and every Member
of that Team executes on the Team's single immutable Node. Cross-machine work
is cooperation between Teams, never a Team split across machines.

## Authority flow

```text
source Team WorkApplicationService
  -> server-authored SourceWorkAttestation
  -> source NodeGateway outbox
  -> Company Control Plane route journal + WorkDelegation relationship
  -> target NodeGateway inbox
  -> target native WorkApplicationService
  -> target Work / Report / Finding / Failure
  -> immutable redacted RemoteFactPublication
  -> source Node read-only cache
```

The accepted Remote Node Fabric owns transport, attempts, receipts, ordering,
expiry and `RecoveryRequired`. The Company collaboration store owns only the
`WorkDelegation` relationship, frozen inbound policy, decisions, cancellation
requests, immutable publication metadata and cross-node projections. Each
Execution Space continues to own its native Work.

No caller may directly write a remote Store. Every cross-node mutation becomes
one closed `collaboration.business.v1` operation using exactly one of:

- `delegation_propose`
- `delegation_decide`
- `target_work_create`
- `delegation_cancel_request`
- `delegation_cancel_decide`
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

Wave 6 does not introduce `TeamMessage v2` or a second delivery ledger. The
source NodeDaemon authors the existing immutable Wave 4C `Message` once with an
optional `CollaborationScope`. The route carries canonical Message bytes (or a
content-addressed immutable object reference). The target NodeDaemon verifies
the source identity, schema, fingerprint and body digest, persists a remote
replica, then creates the existing per-recipient `CanonicalMessageDelivery`.

`CollaborationScope` is an intent hint, not authority. Before authoring, the
server resolves a frozen `CollaborationMessageAuthority` from the current
central `WorkDelegation`, accepted target Work, exact source Work binding,
target Host decision, current placement and non-revoked inbound policy. The
source Store checks that proof again under its Message write lock. The Control
Plane re-resolves the same central records before accepting the route, and the
target Store checks the frozen Work/Team/placement tuple before replica or
Delivery persistence. Nonexistent, pending, rejected, cancelled, stale or
caller-widened authority has zero Message, route, Delivery or provider effect.

Control Plane route receipts are transport evidence only. Company and Team
surfaces may show `CrossNodeDeliveryProjection`, but that projection cannot be
acknowledged or mutated as delivery authority.

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

Large/private evidence uses the accepted Wave 5 artifact service. Manifests,
encrypted blobs and one-use capabilities are not duplicated in the
collaboration store. Delegation scope binds writer Team/Work and exact reader
Host. Retention starts only after transport, terminal Delegation and source
durable import are all terminal; the latest anchor wins. All three times are
derived from canonical Fabric manifest/receipt and Delegation stores. Public
callers cannot submit retention anchors.

## Public and operator surfaces

- AgentFirm HTTP authors proposals and remote facts from server-resolved local
  Work truth and queues them through the NodeGateway.
- Control Plane Host REST lists/reads Delegations and publications and performs
  exact-revision Host decisions, cancellation decisions and artifact grants.
  Read projections are restricted to the exact source owner, source Host or
  target Host resolved from the authenticated credential; Company, Team and
  Execution Space are never caller-selected authority.
- MCP collaboration tools are read-only central projections. Retired local
  WorkDelegation writers are absent and fail as unknown tools with zero Store
  delta.
- OperatorView reports Node-local outbox/inbox depth, oldest age, current
  gateway generation, Control Plane-derived health, reconcile lag and exact
  recovery inventory. Local journal presence never implies remote health.

The executable contracts are
`crates/firm-store/tests/cross_machine_collaboration.rs`,
`crates/firm-fabric/tests/fabric_contract.rs`, and
`scripts/check-collaboration-foundation.mjs`.
