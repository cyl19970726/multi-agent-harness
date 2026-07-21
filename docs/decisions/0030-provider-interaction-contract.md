# ADR 0030: Provider interaction and semantic event contract

## Status

Accepted and implemented in the Agent Team substrate.

Amended by ADR
[0032](0032-provider-native-session-is-execution-truth.md): interaction routing
is durable Harness state, while provider tool/event activity remains solely in
the native provider session.

## Context

Codex, Kimi, Claude, and future providers expose different capabilities in
different execution modes. Codex interactive surfaces can request input and
approval, while `codex exec` is non-interactive. Kimi ACP can pause a turn with
`session/request_permission`, while Kimi headless delivery has a different
protocol. A provider may also report a tool lifecycle as `completed` even when
its business interaction was dismissed or unanswered.

A provider-level boolean capability matrix cannot represent these differences,
and the Dashboard must not turn transport completion into a false product claim.

## Decision

Every Agent Team `MemberRun` snapshots a `ProviderIntegrationProfile` naming the
provider, concrete execution mode, interaction mode, event fidelity, lifecycle
support, native-subagent observation, and transient-thinking policy.

Provider-originated questions, tool approvals, and plan reviews are durable
`PendingInteraction` objects. They record exact provider option ids, routing,
resolution actor, and semantic outcome. The adapter returns the exact selected
option to the provider; it never fabricates an answer.

Routing defaults are:

- clarification and plan review -> Lead;
- ordinary tool permission -> policy layer;
- unknown or authority-bearing decisions -> Human;
- legal, financial, organization, destructive, or permission-bound effects
  remain subject to their product Approval/authority contract and cannot be
  approved merely because a Lead Agent responded.

`PendingInteraction` records both provider and semantic resolution state,
correlated with the provider call id. A Harness control acknowledgement may
summarize the resolution, but provider tool lifecycle stays in the native
session. Provider `completed` does not imply semantic `answered`, `approved`,
or `succeeded`.

The Host-facing MCP surface exposes `team_run_resolve_interaction`; it must use
the exact provider option id and an actor allowed by the interaction route.
The same authorization rule applies to CLI, HTTP, and Dashboard callers.

Thinking remains sanitized transient live state only. It is never a
`PendingInteraction`, `MemberAction`, message, artifact, or evidence record.

## Execution-mode behavior

- `kimi_acp`: `session/request_permission` pauses the same turn. Harness writes
  a PendingInteraction, marks the member waiting, and resumes the same ACP
  request after an authorized response.
- `codex_exec`: structured JSONL tool/artifact events are read from the native
  Codex session/stream and may be projected live, but are not journaled by
  Harness. Fresh mid-turn input is unavailable. A future Codex question must end the round as
  an explicit blocker and continue in a follow-up turn, or use a separately
  implemented interactive execution mode.
- unknown reverse-RPC: fail closed and report an adapter gap.
- provider-native subagents/background tasks: observe honest attribution when
  exposed; do not claim Harness lifecycle control without a wired control path.

## Consequences

- Capability truth is mode-specific and reconstructable per run.
- Team Activity can present questions and approvals as actionable pressure.
- Provider version/protocol changes require profile and acceptance updates.
- Adapters need explicit coverage for interaction, lifecycle, errors, context,
  artifacts, permissions, subagents, background tasks, auth, quota, and privacy,
  not only a tool-name inventory.

## Validation

- Kimi deterministic ACP test: question -> PendingInteraction -> Lead option ->
  same-turn resume -> semantic `answered`.
- Kimi deterministic ACP policy test: tool permission -> Policy route -> Lead
  rejection -> exact Policy option -> same-turn resume -> semantic `approved`.
- Mixed Codex/Kimi target test: Codex command events and Kimi tool events are
  readable from their native sessions but absent from Harness ledgers;
  provider thinking is absent from all Harness persistence.
- Dashboard checks: exact option id/actor is posted through the TeamRun-scoped
  resolve route and pending interactions appear in Team and Member activity.
- Schema fixtures validate ProviderIntegrationProfile and PendingInteraction.
  Current MemberAction semantic fields are migration debt until ADR 0032 is
  fully implemented.
