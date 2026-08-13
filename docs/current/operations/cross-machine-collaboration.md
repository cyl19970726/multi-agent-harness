# Cross-Machine Collaboration Operations

Operate collaboration only after both Nodes have passed the Remote Node Fabric
enrollment, lease, generation and reconciliation gates in
[Remote Fabric operations](remote-fabric-operations.md). Do not open a second
Node listener and do not copy node private keys between machines.

## Before a journey

Record the exact build SHA, Company id, Control Plane generation, source and
target Node ids, NodeDaemon generations, gateway generations, protocol/schema
digest, source and target Execution Space ids, and immutable Team revisions.
Abort if any expected machine identity differs. Never discover or rebind a
machine by scanning the network.

Verify:

1. each Team's canonical `node_id` matches exactly one authorized Mac;
2. both NodeGateway sessions are current children of current NodeDaemon leases;
3. source and target Stores contain the expected Team and WorkApplicationService
   state;
4. no legacy local collaboration ledger is admitted as writable authority;
5. the Control Plane Host REST remains loopback-only or behind the approved TLS
   reverse proxy.

## Expected journey

The source Host proposes from an exact local Work revision. The target Host
accepts the central relationship; only the target Node creates native Work.
Messages reuse the canonical Message/Delivery fabric. The target publishes
server-resolved Reports, Findings or Failure analyses and explicitly grants
completed artifacts. The source Host imports evidence and independently
accepts or revises source Work.

For every mutation preserve its idempotency key, expected revision, routed
operation id, ordering key and terminal receipt. An exact replay must return
the original result. A changed fingerprint, actor, placement, generation or
revision must fail closed.

## Attention and recovery

Treat these as attention, not success or Work failure:

- Control Plane accepted but target application has no terminal receipt;
- inbox/outbox state is `RecoveryRequired`;
- target Node is offline, stale or successor-fenced;
- placement or Team revision changed;
- artifact upload is incomplete/corrupt/expired;
- publication digest or source/target Work scope differs;
- cancellation request is pending without a target Host decision/native Work
  event.

Never edit JSONL state to repair a route. Use the existing Remote Fabric
reconcile/diagnostic surfaces and preserve ambiguous effects as unknown. A
source Work remains open until its own Host acts, regardless of target outcome.

## Real two-Mac acceptance

Use only the two explicitly authorized Macs and the reviewed build. Run the
full proposal -> target decision -> native target Work -> Message -> fact and
artifact -> source integration journey, then exercise offline, replay,
successor-generation, cancellation race, corrupt artifact and stale placement
cases. Evidence must be secret-free and bind exact SHAs, identities,
generations, operation ids, Work revisions and artifact digests. It must also
prove all child processes and test listeners were stopped.

The submission gate accepts only the recomputable
`agentfirm.wave6-two-mac-evidence.v2` bundle. Its manifest hashes the raw
central collaboration ledger, Control Plane and both Node Fabric journals,
both canonical trust ledgers, secret-free provider transcripts and imported
artifact bytes. `scripts/acceptance-cross-machine-collaboration.mjs`
independently derives the current Delegation/Work, Node and Gateway
generations, immutable Message replica, per-recipient Delivery, terminal
receipt, provider markers and artifact digest. Self-reported success booleans
are not evidence. The bundle binds the submitted revision to the validator's
exact Git `HEAD`, avoiding a self-referential SHA inside the evidence commit. A
later evidence-only commit is allowed only when the validator proves every
intervening path is under `docs/current/operations/evidence/`.

If the second authorized Mac or its approved endpoint is unavailable, report
the real-machine gate as blocked. A second process on one Mac is deterministic
test evidence, not two-Mac dogfood.
