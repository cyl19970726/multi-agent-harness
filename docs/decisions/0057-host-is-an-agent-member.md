# ADR 0057: Team Host Is An AgentMember

- Status: accepted; implementation tracked by DEV-59
- Date: 2026-08-22
- Product authority: SPEC-DEV-59-01 v2
- Implementation issue: [#501](https://github.com/cyl19970726/multi-agent-harness/issues/501)
- Supersedes: ADR 0040 where it models the current Host as a separate native-task delivery species

## Context

`AgentTeam.host_agent_id` and `TeamMembership(role=host)` already name the
Host as an `AgentMember`, but execution still used an external Host actor,
Host-specific provider bindings, and a pull/hook inbox. That split could store
Member-to-Host coordination without a continuously managed recipient able to
claim the exact delivery, enter a provider cycle, and settle an exact-session
receipt.

## Decision

1. Host authority resolves from exactly one active Host `TeamMembership` for
   `AgentTeam.host_agent_id`. Compatibility actor `"host"` is historical-read
   data, never current mutation authority.
2. Every current TeamRun contains exactly one Host `MemberRun`. A managed Host
   uses the same `MemberRun -> AgentSession -> NodeDaemon ->
   TeamRuntimeAdapter` path as an ordinary Member. Provider bindings are
   `codex_app_server`, `claude_agent_sdk`, `kimi_acp`, and `pi_rpc`.
3. `host_runtime_mode=managed` is the automated-Team default. The Host's
   canonical Message deliveries and status attentions are claimed only for its
   exact AgentSession, runtime generation, and current NodeDaemon generation.
   Provider receipt settles transport; it does not accept Work.
4. `host_runtime_mode=external_interactive` keeps the same AgentMember and Host
   authority but uses a detached, user-driven MemberRun. Its delivery contract
   is `pull_only`: Harness creates no AgentSession or RuntimeCommand, performs
   no provider admission or turn, and cannot settle a provider receipt. UI
   visibility or hook success never becomes a recipient ACK.
5. Work, Message, status attention, and RuntimeCommand remain separate source
   planes. The runtime may present them as one cycle input batch but cannot use
   one plane to authorize or mutate another.
6. Host and Member share execution capability and lifecycle language. Role
   policy remains separate from runtime identity. Work creation, assignment,
   dependency topology, and readiness belong to the canonical flat Work DAG
   application service rather than this Host-runtime decision.
7. Host-visible status derives deterministically from canonical Work/runtime
   facts. Duplicate source ids are idempotent, Host-authored status does not
   self-wake, and one claimed batch produces at most one provider cycle.
8. Historical `external` values decode as `external_interactive`; they do not
   fabricate an AgentSession, native session, provider receipt, or managed
   delivery. Mode changes require explicit Close/Reopen generation fencing and
   never silently fall back.

## Consequences

- `HostRuntimeBinding` is no longer a standard Team execution surface. Any
  retained external transport descriptor is historical capability metadata;
  no current CLI, HTTP, MCP, daemon, or hook path may execute it.
- CLI, HTTP, MCP, and Dashboard create Teams in managed mode unless the caller
  explicitly selects external interactive ownership.
- Operator views expose the exact Host AgentMember/MemberRun, ownership mode,
  delivery guarantee, queued actionable count, and the external pull warning.
- A managed Host uses `ReadOnly` when the selected provider can prove it.
  Kimi ACP cannot; it may retain an honestly frozen `FullAccess` ceiling only
  with an explicit Host `provider_cwd_hint` distinct from the Team execution
  root. Host coding still requires explicit Host-owned Work and an
  independently reserved workspace. The active Host MemberRun is the durable
  Store authority for that canonical cwd: the Store canonicalizes roots,
  freezes `provider_cwd_hint` as immutable MemberRun provenance, rejects
  conflicting initial/raw/dynamic admission under the same lock, and
  revalidates the reservation before session materialization. This preserves
  one driver per writable workspace without inventing a Kimi sandbox.

## Acceptance

- Store admission rejects a missing, duplicate, wrong-identity, or mode-mismatched Host MemberRun.
- Managed Message and status delivery cannot settle across an AgentSession or NodeDaemon generation change.
- External delivery never claims managed receipt semantics.
- Host/Member runtime unification introduces no Work-containment authority or nested Team topology.
- All four providers select the ordinary Team runtime binding for managed Hosts.
- Managed Kimi Host admission fails before AgentSession materialization when
  the Host workspace is absent, aliases the Team execution root, or is already
  reserved by another active MemberRun.
- Dynamic Workflow retirement, package boundaries, source-size ceiling, deterministic tests, and exact-SHA independent Review remain green.
