# Remote Fabric Operations

This runbook operates one logical AgentFirm Control Plane and its outbound
machine gateways. Stop if an expected Company, Node, NodeDaemon generation,
certificate fingerprint, Store root or backup digest differs.

## Credentials

CI and isolated tests use regular non-symlink PEM/key files. Private key and
bearer/key files must have mode `0600`. Production macOS Nodes use:

```text
firm fabric node-gateway serve \
  --credential-backend macos-keychain \
  --keychain-service agentfirm.remote-fabric \
  ...
```

For Company `<company>` and Node `<node>`, create generic-password Keychain
items under service `agentfirm.remote-fabric` with these exact accounts:

- `<company>:<node>:client-certificate`
- `<company>:<node>:client-private-key`
- `<company>:<node>:control-plane-ca`

The CLI reads them directly into the TLS configuration; it does not write a
temporary private-key file. Missing, empty or unsupported credentials fail
closed before a Fabric frame is sent. Certificate serial and public-key
fingerprint are public enrolled identity supplied explicitly through
`--certificate-serial` and `--public-key-fingerprint`; keeping them outside
Keychain avoids unnecessary per-item ACL prompts for login LaunchAgents.

One bounded development dogfood may instead use non-symlink mode-`0600`
credential files when the user explicitly approves that exact run because
interactive Keychain ACL prompts would prevent unattended restart testing.
The files must stay on the Node that generated the private key, outside the
repository and evidence directory, and must be removed or rotated after the
run. The resulting evidence must state `file-backed development exception`;
it proves mTLS and credential file fail-closed behavior, **not** the production
Keychain path. Release/production admission still requires a separate
Keychain-backed run.

## Enrollment and rotation

1. Host creates a short-lived one-use enrollment through
   `POST /v1/fabric/enrollments`.
2. The Node generates its private key and CSR locally.
3. The Node consumes the token through `POST /v1/fabric/nodes/enroll`.
4. Verify returned CompanyNode id equals the existing ExecutionNode id and the
   certificate fingerprint equals the local key.
5. Install the certificate material in the credential backend, start the
   current NodeDaemon, then start NodeGateway as its exact generation child.

Rotation requires proof from the current key and exact Node revision. Revoked,
expired, wrong-Company or predecessor certificates cannot connect. Never
delete the prior credential until the successor has connected and the current
gateway projection is verified.

## Backup

Use a new destination directory:

```text
firm fabric control-plane backup \
  --company <company> \
  --output <new-backup-directory> \
  --firm-home <firm-home>
```

The Store lock freezes a complete transaction boundary. The backup contains a
hash-chained journal and a manifest binding schema version, transaction
sequence, state digest, journal digest and byte length. Copy both together to
durable encrypted storage. Artifact encryption keys, capability signing keys,
CA private keys and Host bearer credentials are backed up separately in the
credential system; they are intentionally absent from the Store backup.

## Restore

1. Stop the Control Plane and confirm no process owns its Store or listener.
2. Preserve the old directory for forensic recovery; never restore over it.
3. Select a new empty Company Remote Fabric Store root.
4. Run:

```text
firm fabric control-plane restore \
  --company <company> \
  --backup <backup-directory> \
  --firm-home <new-firm-home>
```

Restore validates every journal frame and all manifest digests, then writes
only to the empty root. Tamper, torn data, schema mismatch, symlink paths and
non-empty targets fail closed. Start the Control Plane as a successor
generation, restore credential-system keys, and let every Node reconnect and
reconcile before admitting new work.

## Drain, revoke and recovery

- Drain rejects new target work but lets already persisted operations settle.
- Revoke fences new gateway connections immediately; rotate credentials only
  through the exact current Node revision.
- `RecoveryRequired` means effect cannot be proven. Inspect the Node-local
  application result and provider/runtime canonical record. Resolve as applied
  or not-applied with evidence; never blind replay.
- If the Control Plane generation changes, old gateway frames and heartbeats
  are stale. Nodes reconnect, reconcile accepted operations and rebind only
  operation ids the current Control Plane proves absent.

## Verification

Run `pnpm acceptance:remote-fabric`. Preserve its evidence directory for the
reviewed SHA. It must contain exactly the documented JSON evidence files, no
private keys/tokens, a three-process result, empty Node inbound-listener list,
terminal target effect and source reconciliation. Confirm all child processes
have exited before declaring the gate complete.

## Real two-Mac dogfood

The deterministic three-process gate does not satisfy the real-machine gate.
Use two explicitly authorized Macs with distinct initialized ExecutionNode ids
and the same Company Control Plane. Do not use SSH discovery or copy private
keys between machines.

1. On the Control Plane Mac, start the reviewed `firm fabric control-plane
   serve` revision behind the trusted TLS endpoint. Record its exact Git SHA,
   Company id, Control Plane generation and schema bundle digest.
2. Create one enrollment per existing ExecutionNode id. Each Mac generates its
   own key and CSR, consumes only its enrollment, and installs the returned
   material in macOS Keychain using the accounts above, unless this exact
   development run has the explicit file-backed exception above.
3. Start the current NodeDaemon and then `firm fabric node-gateway serve` on
   both Macs with `--credential-backend macos-keychain` (or the explicitly
   recorded development exception). Record the two exact
   Node ids, NodeDaemon generations and gateway generations. Reject the run if
   either Node exposes an inbound collaboration listener.
4. On Node A create a bounded diagnostic body such as
   `{"probe":"two-mac-a-to-b"}` and queue it with `firm fabric route queue
   --kind probe`, targeting Node B and a real Node-B Execution Space. Keep the
   operation id, idempotency key and ordering key stable for replay checks.
5. Verify the Control Plane journal reaches `operation_applied`, Node A
   reconciles the same terminal receipt, and an exact replay returns the
   original result without a second target application.
6. While Node B's predecessor gateway is still current, rotate its certificate
   for the successor NodeDaemon generation; this revokes the old serial and
   expires the predecessor gateway authority. Stop the predecessor process,
   wait until the server-derived lease is offline, queue a second probe on Node
   A, and prove it remains durable/nonterminal. Restart Node B with the rotated
   certificate,
   require a successor gateway generation, and verify the queued operation
   applies exactly once. A reconnect that merely self-reports a higher daemon
   generation under the predecessor certificate must fail with zero effects.
7. Capture `remote_fabric_status` and `remote_fabric_operation_show` through
   the local MCP surface or the equivalent authenticated Host REST reads.
   Evidence must bind the immutable submitted SHA, Company/Node ids, Control
   Plane and gateway generations, operation ids, protocol/schema digest,
   disconnect/reconnect timestamps, terminal receipts and application count.
   Evidence must not contain enrollment tokens, bearer credentials, PEM/key
   bytes, Keychain values, Message bodies, provider transcripts or repository
   content.

If a second authorized Mac or a reachable trusted TLS endpoint is unavailable,
report the real dogfood gate as blocked. Never substitute loopback processes or
an extra process on one Mac while calling the result “two-Mac”.
