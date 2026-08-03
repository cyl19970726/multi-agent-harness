# ADR 0030: Provider interaction and semantic event contract

## Status

Accepted and implemented in the Agent Team substrate.

Amended by ADR
[0032](0032-provider-native-session-is-execution-truth.md): interaction routing
is durable Harness state, while provider tool/event activity remains solely in
the native provider session.

Amended by ADR
[0044](0044-durable-team-supervision-and-typed-mail.md): the current Team
Supervisor generation owns live provider transports and interaction controls;
typed actor provenance and generation fencing apply to every CLI/HTTP/MCP/UI
resolution.

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

Provider-originated questions, plan reviews, and tool approvals that require a
real external decision are durable `PendingInteraction` objects. They record
exact provider option ids, routing, resolution actor, and semantic outcome. The
adapter returns the exact selected option to the provider; it never fabricates
an answer.

A trusted full-access adapter may synchronously acknowledge an ordinary tool
permission only when the provider itself advertises an exact safe intent
(`allow_always` or `allow_once`). That acknowledgement is not a product pause:
it creates no `PendingInteraction`, no temporary Member waiting state, and no
created/resolved interaction pair. Harness keeps one bounded
`provider_control` acknowledgement without command or prompt content; the tool
lifecycle remains exclusively in the provider-native session. Unknown option
spelling, a reject-only option set, or an unregistered adapter always fails
closed through a real `PendingInteraction`.

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

- `kimi_acp`: a trusted full-access tool request with an exact ACP safe-allow
  option is acknowledged synchronously without a false pause. Every question,
  plan review, unknown request, and tool request without a safe allow option
  writes a PendingInteraction, marks the member waiting, and resumes the same
  ACP request only after an authorized response.
- `codex_app_server`: the default and only new Codex Agent Team mode. Native
  reverse requests become PendingInteractions, while `turn/steer` and
  `turn/interrupt` back the corresponding controls.
- `codex_exec`: Workflow-only for new bounded work. Its structured
  JSONL tool/artifact events may be projected from the native Codex
  session/stream but are not journaled by Harness. It is not a fallback or
  selectable Team mode; historical Team records remain readable only.
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
- Kimi deterministic ACP full-access test: repeated tool permissions with exact
  safe-allow options -> synchronous acknowledgements -> zero
  PendingInteractions and zero waiting projections.
- Kimi deterministic ACP fail-closed tests: reject-only tool permission ->
  Policy PendingInteraction, and unknown reverse request -> Human
  PendingInteraction; each resumes the same turn only after resolution.
- Mixed Codex/Kimi boundary test: Codex command events and Kimi tool events are
  readable from their native sessions but absent from Harness ledgers;
  provider thinking is absent from all Harness persistence.
- Dashboard checks: exact option id/actor is posted through the TeamRun-scoped
  resolve route and pending interactions appear in Team and Member activity.
- Schema fixtures validate ProviderIntegrationProfile and PendingInteraction.
  MemberAction semantic fields are restricted to Harness-owned coordination,
  control acknowledgements, and explicit outcomes under implemented ADR 0032.
