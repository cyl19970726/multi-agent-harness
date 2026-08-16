# Cross-Machine Collaboration Operations

Operate collaboration only after both Nodes have passed the Remote Node Fabric
enrollment, lease, generation and reconciliation gates in
[Remote Fabric operations](remote-fabric-operations.md). Do not open a second
Node listener and do not copy node private keys between machines.

## Before a journey

Record the exact build SHA, Fabric tenant id, Control Plane generation, source
and target Node ids, NodeDaemon generations, gateway generations,
protocol/schema digest, control-store identity, and immutable Team revisions.
The retained `company_id` transport field is a Fabric isolation key, not a
Company or Organization product authority. Execution Space selection locates
an operator-owned Store; it does not own collaboration admission. Abort if any
expected machine or Store identity differs. Never discover or rebind a machine
by scanning the network.

Verify:

1. each Team's canonical `node_id` matches exactly one authorized Mac;
2. both NodeGateway sessions are current children of current NodeDaemon leases;
3. the authoritative control Store contains the expected Team,
   TeamMembership, Work, MessageSubscription and delivery state, while each
   Team remains placed on exactly one Node;
4. no legacy local collaboration ledger is admitted as writable authority;
5. the Control Plane Host REST remains loopback-only or behind the approved TLS
   reverse proxy.

## Expected journey

Work and conversation use separate authority paths. The source Host may
propose an explicit WorkDelegation from an exact Work revision; the target Host
accepts that responsibility relationship, and only the target Node creates
native target Work. An ordinary AgentMember peer Message does not require a
WorkDelegation or an admission-time recipient runtime. It uses the canonical
Message -> MessageSubscription -> CanonicalMessageDelivery path. Admission
queues exactly one Team-addressed delivery under the subscription's frozen
policy/capability and Team/Node generations. The target atomically claims that
delivery against one current TeamMembership plus membership and NodeDaemon
generations before applying it to the resolved AgentMember/session. The target
may separately publish server-resolved Reports, Findings or Failure analyses
and explicitly grant completed artifacts. The source Host imports evidence and
independently accepts or revises source Work.

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

The DEV-7 affected rerun may use the explicitly authorized, pre-existing
Tailscale overlay as a user-provided trusted network path. That exception does
not make Tailscale, NAT traversal, Internet exposure or peer discovery an
AgentFirm product capability. The Control Plane gateway listener binds only
the authorized Tailscale address, its TLS certificate carries that exact IP
SAN, Host REST remains loopback-only, and AgentFirm mTLS, enrollment, actor and
generation fences remain authoritative over the overlay.

The submission gate accepts only the recomputable
`agentfirm.wave6-two-mac-evidence.v3` bundle. Its manifest hashes the raw
control-store collaboration ledger, Control Plane and both Node Fabric
journals, both canonical trust ledgers, secret-free provider transcripts and
imported artifact bytes.
`scripts/acceptance-cross-machine-collaboration.mjs` independently derives the
current Team/TeamMembership/MessageSubscription authority, the single queued
Team delivery and exact membership-generation claim, current Node and Gateway
generations, immutable Message replica, resolved per-recipient
CanonicalMessageDelivery, terminal receipt, concurrent Artifact Grant replay
safety, canonical `ArtifactImport`, provider `AgentSession` + `RuntimeCommand`
+ terminal acknowledgement/transcript digests, raw transaction selectors and
cleanup command results. Explicit WorkDelegation evidence remains
responsibility evidence and never authorizes the ordinary peer Message.
Self-reported success booleans and caller-authored path allowlists are not
evidence. Every Control Plane, Gateway and NodeDaemon process must identify the
exact tested build. If evidence is committed after that build, the validator's
fixed policy permits only paths below `docs/current/operations/evidence/`; no
manifest field can widen this set.

If the second authorized Mac or its approved endpoint is unavailable, report
the real-machine gate as blocked. A second process on one Mac is deterministic
test evidence, not two-Mac dogfood.
