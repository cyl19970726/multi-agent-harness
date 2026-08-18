# Remote Node Fabric

Status: current canonical architecture contract delivered in the development
batch historically named “Wave 5”. In this document, Wave 5/4C labels identify
development batches, not a current product `Wave` object.

## Purpose and boundary

Remote Node Fabric connects AgentFirm execution across machines without
creating a second Firm, Node, identity, Message, Work or runtime authority.
There is one logical Fabric Control Plane. Every machine runs one NodeDaemon
and one outbound NodeGateway connection.

- One `ExecutionNode` identity per machine; the former `CompanyNode` name is
  retired and always denoted the same row (`CompanyNode.id == ExecutionNode.id`).
- NodeGatewayLease is a child of the exact current NodeDaemonLease generation.
- Nodes use outbound `wss://` with TLS 1.3 mTLS and
  `agentfirm.node.v1`; Nodes expose no inbound collaboration listener.
- The certificate-derived Firm and Node identity, never Hello or payload
  JSON, selects machine authority.
- Source authority is closed to `node | control_plane`.
- Protocol, schema and deterministic canonical-byte versions are mandatory.

## One route truth

FabricStore is Firm-scoped and owns `RoutedOperation`, transport-only
`RouteAttempt`, and generation-fenced `RouteReceipt`. It is the sole cross-Node
route truth. Node-local Stores own pre-acceptance outboxes, target inboxes and
application results. `firm-store` owns their filesystem roots.

Wave 4C MessageRouteJournal is not a second route ledger. For cross-Node work it
may only project FabricStore state read-only. There is no dual write, fallback
reader, migration or replay from MessageRouteJournal.

```text
source outbox persisted
  -> ControlPlaneAccepted
  -> target inbox persisted
  -> TargetPersisted
  -> target claim
  -> canonical application
  -> OperationApplied | OperationRejected | RecoveryRequired
  -> source reconciliation
```

Delivery is at least once; application is idempotent. RouteAttempt never proves
application. The only application certainty values are `none`, `not_applied`,
`applied`, and `unknown`. Unknown forbids blind replay.

## Canonical payloads

A message route carries the complete immutable canonical Message envelope or
an authenticated content-addressed `message_object_ref`. The target verifies
the body digest, Firm, Team and identity scope, then persists the Message in
the existing trust Store before creating a per-recipient
`CanonicalMessageDelivery`.

A runtime route carries the complete immutable ControlCommandEnvelope. The
target verifies its fingerprint, Node/Session/generation scope and dispatches
through the existing NodeDaemon socket. The canonical RuntimeCommand terminal
record, not socket success, decides application effect.

Artifacts use digest-bound manifests, bounded size/capability scope, encrypted
Firm-local storage, short-lived one-use upload/download capabilities and
tamper rejection. Large bytes never enter the 256 KiB Fabric frame.

## Replay, crash and successor rules

The Control Plane Store and each Node-local Store use hash-chained fsync-backed
transaction frames. A failure before append is effect-none. A complete append
with lost acknowledgement is effect-unknown until reopen and reconciliation. A
torn final frame is truncated; mid-journal corruption is rejected.

An offline source outbox reconnects by reconciling its operation id through the
current gateway generation. An empty result proves the Control Plane never
accepted it and permits rebinding the pre-acceptance envelope. Any receipt
means FabricStore owns route truth; the source waits for terminal
reconciliation and never resubmits. Successor target generations may retry only
when the earlier attempt proves `effect=none`.

## Product and operator projections

Control Plane Host REST lists Nodes with snapshot-bound opaque cursors. Node detail,
diagnostics, enrollment, revoke, drain, artifact and recovery operations are
server-authoritative and Host-authenticated. Operator RoleView projects the
Node-local gateway session, outbox/inbox depth, oldest age, reconcile lag and
RecoveryRequired operations. SSE only invalidates; clients refetch canonical
views.

The required gate is `pnpm acceptance:remote-fabric`. It combines schema and
hostile-fixture checks, deterministic trust/recovery tests and one real
three-process mTLS/WSS journey with secret-free evidence.
