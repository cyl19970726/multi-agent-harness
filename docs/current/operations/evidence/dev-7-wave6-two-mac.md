# DEV-7 Wave 6 · two-Mac collaboration evidence

This is the historical secret-free record of the first authorized real-machine
Wave 6 journey. It predates the recomputable v2 evidence contract and is not
valid for a later submitted revision. The final DEV-7 submission must attach a
new v2 bundle and pass the executable validator against that exact revision.

This record describes the authorized real-machine Wave 6 journey.
The machine credentials, bearer values, certificate bodies, provider session
transcripts and one-use artifact capability are deliberately absent. The
machine-readable companion is
[`dev-7-wave6-two-mac.json`](dev-7-wave6-two-mac.json).

The affected v3 rerun uses an explicitly authorized, pre-existing Tailscale
overlay only as user-provided trusted network transport between the two real
Macs. It does not claim a shared physical LAN or AgentFirm support for
Tailscale, NAT traversal, public exposure or P2P discovery. AgentFirm mTLS,
enrollment, generation and actor authority remain independently enforced.

## Fixed authority

- Company: `agentfirm-wave5-dogfood`
- source Node: `2437c3dd-14ad-4be6-8d09-0b715fe5aa04`
- target Node: `4f8c9e05-615f-42d8-b541-adced1a4cedf`
- Control Plane generation: `5`
- source and target Gateway generation: `7`
- protocol/schema digest:
  `f4b82259bb854f6cf51957d1487e62b399830f7e8c20a683f69d51bec54e87dc`
- exact post-fix replay binary revision:
  `de600f5a41e166373f9d621cfba29339619ecbe9`

Each Team stayed on one immutable Node. The journey was collaboration between
Team A and Team B; no Team Member crossed machines.

## Positive journey

1. Real Codex provider sessions on both Macs returned the bounded markers
   `WAVE6_NODE_A_PROVIDER_OK` and `WAVE6_NODE_B_PROVIDER_OK`.
2. Source Work authority produced a server-authored attestation and routed
   proposal `collaboration-propose:wave6-real-a-to-b`.
3. The exact target Host accepted the relationship. Only target Node B created
   and later accepted `remote-work:delegation:wave6-real-a-to-b`.
4. Source Node A authored immutable canonical Message
   `message:wave6-real-message-a-to-b-v7` once. Node B persisted the replica
   before creating the existing per-recipient delivery
   `message:wave6-real-message-a-to-b-v7:wave6-host-b-real`.
5. Target Node B created a native Finding and Result report. The result
   publication carried the target Host's accepted Work decision proof; the
   Control Plane atomically stored the publication and advanced the
   Delegation to `result_available`. Source Node A stored only its read-only
   cache.
6. A 55-byte Company-internal artifact was initiated and completed under the
   accepted Wave 5 artifact authority, then the target Host granted the exact
   source Host a one-use read capability. Source Node A applied the routed
   grant. The stored SHA-256 is
   `9ebdda64fe69675d3b6334e74522c0ae63dca5f596af438b0cee870b485833d2`.

The source Work remained independently open; target success never fabricated
source Work completion.

## Replay and fail-closed evidence

- Exact result-publication replay using the original idempotency key,
  revision, expiry, Work/placement and fact returned the original terminal
  outbox without a second route or business mutation.
- Reusing that key with a changed expiry returned an idempotency conflict.
- Deterministic collaboration and fabric suites cover offline expiry,
  successor generation, stale placement, cancellation races, corrupt artifact
  data, torn journal recovery and unknown-effect `RecoveryRequired` handling.

## Cleanup

Both NodeGateways, both temporary NodeDaemons, both local HTTP services and the
Control Plane were stopped through their owning process sessions. Ports
`9443`, `9786`, `9886` and `9887` had no listener, and neither authorized Mac
retained a DEV-7 `firm` process.
