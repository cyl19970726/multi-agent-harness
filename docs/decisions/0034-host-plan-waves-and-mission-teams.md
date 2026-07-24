# ADR 0034: Host Plan Waves And Mission-Scoped Agent Teams

```text
status: active
date: 2026-07-23
supersedes: ADR 0025 attempt ownership; ADR 0026 Wave-as-executor hierarchy
```

## Context

The first Mission/Wave implementation made each Wave own one executor kind and
treated every AgentTeamRun as an immutable attempt of exactly one Wave. That
model is easy to validate, but it weakens the Host Agent:

- a long-running member cannot naturally continue while the Host advances its
  plan;
- two completed assignments cannot be integrated while a third keeps running;
- changing the plan forces a new TeamRun even when the same team and provider
  sessions should continue;
- the Wave becomes a scheduler and execution container instead of a concise
  record of the Host's current judgment; and
- Agent Team cannot remain an independent reusable capability.

Provider-native sessions are already the execution truth under ADR 0032.
Harness therefore does not need a Wave boundary to manufacture execution
ownership or transcript history.

## Decision

### Mission is durable intent and the team relation boundary

A Mission stores its durable Markdown context and may link zero or more
independent `AgentTeam` definitions through `agent_team_ids`.

The relation is explicit but not exclusive:

- one Mission may use multiple teams;
- one team may be linked to multiple Missions over its lifetime;
- a team may exist and run without a Mission; and
- closing a Mission never deletes, archives, or implicitly completes a team.

The Mission relation answers “which teams may the Host use for this intent?” It
does not assign work by itself.

### Wave is a versioned Host operational memo

A Wave is an ordered, append-only revision of the Host's current plan and
judgment. Its Markdown `context` should explain the important current facts:

- what changed since the preceding Wave;
- what the Host intends to do next;
- assignments or member changes the Host decided to make;
- open questions, blockers, conflicts, and integration decisions;
- work that intentionally carries into a later Wave; and
- the evidence or outcome that justified advancing.

The Host updates the current Wave while its judgment is materially unchanged
and creates Wave N+1 when the plan changes materially. Append-only rows retain
the revision history.

A Wave is not a task graph, barrier, executor container, TeamRun attempt owner,
or provider-session boundary. The Host may advance while assignments and
provider sessions remain active.

### Agent Team and assignments outlive Waves

`AgentTeam` is the stable reusable definition. A mission-scoped
`AgentTeamRun(agent_team_id, mission_id, wave_id = null)` may stay active across
multiple Waves. Its `MemberRun` and provider-native session bindings remain
stable until the Host explicitly changes or stops them.

Actual work ownership is expressed by
`TeamMessage(kind=assignment, correlation_id)`. A message may record an
optional `origin_wave_id` for navigation and explanation, but that field never
controls delivery, completion, or lifetime.

Questions and answers use the same correlated message channel. The Host can
message, steer, interrupt, add, rename, or deactivate members according to the
real provider and permission capabilities. A provider `completed` frame is not
semantic completion.

The Host Agent that creates and coordinates the team is its **Team Lead**.
Harness retains `owner_agent_id` as the compatibility wire field for this
identity, and reserves `host` to mean the current Host Agent. The Lead owns team
formation, assignment, member interaction, composition changes, integration,
and acceptance. It is a control-plane actor, not an implicit `MemberRun`; if the
Lead also performs an execution lane, the Host must explicitly add a member and
bind that member to its native provider session.

### CLI is canonical; MCP is a thin optional adapter

The complete Host control surface is shared application logic exposed through
the CLI. A thin Host orchestration skill teaches when and how to call it.

MCP may expose the same application operations for structured tool discovery,
but it is optional and must not own product semantics, storage, validation, or
an MCP-only lifecycle. Hooks notify; Dashboard visualizes and accepts human
controls.

### Compatibility boundary

Existing rows containing `Wave.executor_kind`, `executor_run_ids`,
`accepted_run_id`, and `AgentTeamRun.wave_id` remain readable. They represent
the legacy **direct Wave executor** mode only.

New Mission-scoped Agent Team work does not populate those attempt fields.
New product documentation, fixtures, and default CLI examples use Mission-linked
teams, Host-plan Waves, correlated assignments, and explicit Host advance.
Compatibility code must be isolated and must not reintroduce the old hierarchy
into the main authoring or Dashboard path.

## Consequences

- Mission closeout depends on explicit Host Wave decisions and outcome, not on
  every linked TeamRun being terminal.
- Wave history can reconstruct how Host judgment changed without duplicating
  provider transcripts or tool streams.
- Dashboard shows Mission context, linked teams, the selected Wave memo, and
  assignment/member status as related projections rather than nested runtime
  ownership.
- A running assignment may be shown as “carried forward from Wave 1” during
  Wave 2.
- Team deletion is not a Mission side effect. Detach, archive, and stop are
  separate explicit controls.
- Direct Wave executor rows remain migration evidence but are not the default
  contract.

## Acceptance

The deterministic scenario must prove:

1. create a Mission with Markdown context;
2. create and link an independent AgentTeam;
3. start a Mission-scoped TeamRun without a Wave id;
4. assign at least three correlated member lanes from Wave 1;
5. advance to Wave 2 while one lane and native session remain active;
6. continue messaging that member and preserve the same MemberRun/session;
7. add or deactivate a member and assign new work from Wave 2;
8. show Mission, Wave history, linked team, messages, and carry-over in CLI and
   Dashboard projections;
9. close the Mission without deleting or silently completing the team; and
10. prove MCP, when enabled, delegates to the same behavior as CLI.
