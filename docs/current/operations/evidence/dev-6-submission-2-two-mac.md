# DEV-6 Submission 2 · two-Mac authority evidence

This is a secret-free summary of the affected real-machine checks rerun after
Submission 1. The exact submitted revision is filled only when the final Gate
has completed; no private key, enrollment token, bearer credential, PEM body,
Message body, or provider transcript is stored here.

## Fixed boundary

- Company: `agentfirm-wave5-successor-v2`
- Control Plane endpoint: `wss://192.168.1.5:9443/v1/node-gateway/connect`
- Host REST: `127.0.0.1:9786` only
- Node A: `2437c3dd-14ad-4be6-8d09-0b715fe5aa04`
- Node B: `4f8c9e05-615f-42d8-b541-adced1a4cedf`
- Schema bundle digest:
  `10ecf9e11d1b09dccd1211c9a10c8961ef0f66e2628097ecbcdb5fb878c13863`
- Transport route: direct physical LAN (`192.168.1.5` to `192.168.1.25`),
  no TUN route and no Node inbound collaboration listener
- Credential truth: explicitly approved file-backed development exception;
  mode-`0600`, non-symlink private keys remained on the Mac that generated
  them. This run does not claim the production Keychain path.

## Successor fences predecessor

Node B first connected as the predecessor and then re-enrolled under the exact
successor NodeDaemon authority. The successor became the current Gateway
generation. A subsequent connection attempt using the predecessor credential
and generation was rejected.

Before and after that stale attempt, Node B's Node-local journal had:

- identical line count: `2`;
- identical SHA-256:
  `02398eb16c891e5bc3f1c56a9c18920183915d7c5d0252ea321a30bc631103bc`;
- zero routed operations, attempts, receipts, inboxes, application results, or
  native effects attributable to the predecessor attempt.

This proves the stale process could not persist, claim, apply, or acknowledge
work after successor bind.

## Accepted offline expiry

Operation `wave5-expired-accepted-runtime-v2` was a real non-Probe
`RuntimeCommand` route from Node A to Node B.

1. Node B connected with NodeDaemon generation `12` and Gateway generation
   `5`, then disconnected.
2. While that target lease was still current, Node A submitted the operation.
   FabricStore durably recorded `control_plane_accepted` and one queued route
   attempt targeting Gateway generation `5`.
3. The operation expired while Node B was offline.
4. Node B re-enrolled against exact NodeDaemon generation `13` and connected
   as Gateway generation `6` under Control Plane generation `4`.
5. Successor reconciliation ended the original attempt with
   `error_code=operation_expired` and `effect=none`, then wrote exactly one
   terminal `operation_rejected` receipt with:
   `application_effect=not_applied` and
   `result_schema=agentfirm.remote_fabric.expired.v1`.
6. Node B remained at `inboxes=0` and `results=0`; the operation id was absent
   from both maps. Node A reconciled its outbox to the same terminal receipt.

The source also recovered a separate operation that had expired before any
Control Plane acceptance. It settled only the Node-local pre-acceptance outbox
as `local:not_applied:operation_expired`; it did not fabricate FabricStore
route truth or contact the target.

## Cleanup

The two NodeGateways, the DEV-6 Control Plane, and both temporary NodeDaemons
were stopped after evidence capture. Ports `9443` and `9786` had no remaining
listener, and the remote Mac had no remaining DEV-6 `firm daemon` or
`firm fabric` process.

## Submission binding

- PR: `#448`
- Review revision: the exact commit containing this immutable evidence page;
  the DEV-6 Task and PR submission comment record that SHA.
- Remote CI: bound to the same exact revision in the DEV-6 Task and PR
  submission comment after completion.
