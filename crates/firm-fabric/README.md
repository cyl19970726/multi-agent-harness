# firm-fabric

`firm-fabric` is AgentFirm's machine-to-machine transport trust kernel. It
owns enrollment, Node certificates, Control Plane and gateway generations,
durable routing attempts, receipts, reconciliation, and bounded encrypted
artifacts.

It does **not** own Team Work, Message, MemberRun, provider sessions, or
RuntimeCommand business semantics. Those objects remain canonical in their
existing stores. Fabric operations may carry versioned references to them, but
cannot mutate them directly.

## Trust boundary

- One logical Company Control Plane holds one live generation lease.
- Every machine runs one NodeDaemon and initiates one outbound `wss://`
  connection. Nodes never accept peer-to-peer Fabric connections.
- Enrollment tokens are short-lived, stored only as SHA-256 digests, and
  consumed once under the Store lock.
- A Node proves possession of an Ed25519 key for enrollment, certificate
  rotation, and every `NodeHello`. Certificate serial, public-key fingerprint,
  Company, Node, schema bundle, and generation are checked before mutation.
- Every normal frame is at most 256 KiB, uses subprotocol
  `agentfirm.node.v1`, is closed-contract JSON, and carries exact Company, Node, gateway
  generation, Control Plane generation, protocol version, schema version, and
  payload digest. Gateway admission requires a `VerifiedMtlsPeer` proving TLS
  1.3, the frozen WebSocket subprotocol, certificate serial, key fingerprint,
  Company and Node before `NodeHello` can mutate Store state. The socket adapter
  must construct this peer and its session fence from the verified TLS channel,
  never from message JSON.
- Application actors must be resolved by the existing AgentFirm credential
  authority at the HTTP/MCP/CLI boundary before a routed operation reaches
  this crate. The NodeDaemon integration intentionally waits for the Wave4C
  credential/session surface rather than creating a second identity system.

## Delivery state machine

```text
source Node-local outbox persisted
  -> ControlPlaneAccepted
  -> target Node-local inbox persisted
  -> TargetPersisted
  -> target inbox durably claimed
  -> application applied/rejected
  -> terminal receipt
  -> source outbox terminal
```

The Control Plane Store owns only operations, attempts and receipts. Each Node
has a separately rooted, durably authority-bound Store that is the sole owner
of its local inbox/outbox and application result. A claimed inbox without a
terminal result is reported as unresolved after restart; a duplicate claim is
`RecoveryRequired`, never permission to repeat the native effect.

Delivery is at least once; application is idempotent. A retry can move to a
successor gateway generation only while the previous attempt is unpersisted
and has `effect=none`. Once the target has persisted the operation—or the Store
cannot prove the effect—blind retry is forbidden and reconciliation is
required. Terminal receipts survive gateway generations.

The FabricStore persists hash-chained, fsync-backed transaction frames. A
failure before append leaves the old state. A lost acknowledgement after a
complete append latches the Store unavailable with `effect=unknown`; restart
recovers the complete frame and requires reconciliation. A torn final frame is
discarded and truncated before the next append.

## Working-revision checks

```bash
cargo test -p firm-fabric -- --test-threads=1
cargo clippy -p firm-fabric --all-targets -- -D warnings
node scripts/acceptance-remote-fabric.mjs
```

The acceptance script validates the closed schemas and hostile fixtures, runs
the deterministic security/replay/recovery matrix, then starts one real
Control Plane process and two real Node processes over TLS 1.3 mutual-auth WSS.
It proves target persistence before application, terminal application receipt,
source reconciliation, no Node listener, process cleanup, and secret-free
evidence output.

The source may queue while disconnected. On reconnect it first asks the
current, generation-fenced Control Plane to reconcile the operation id. Only
an empty result permits rebinding the pre-acceptance envelope to a successor
gateway generation. Any accepted receipt makes FabricStore the sole route
truth and forbids resubmission.

Production Node credentials can be read directly from macOS Keychain with
`--credential-backend macos-keychain`; CI and isolated local tests use strict
non-symlink file credentials. See
`docs/current/operations/remote-fabric-operations.md` for enrollment,
credential account names, backup/restore, rotation, drain and recovery.
