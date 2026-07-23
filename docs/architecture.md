# Architecture

## Product Boundary

Star Harness is the coordination product. A business project is a tool
environment connected through an adapter.

```text
Star Harness
  Mission intent / Host-plan Waves / team relations
  Agent Team control plane
  Dynamic Workflow runtime
  Host-facing plugins, MCP tools, skills, CLI
  Provider-neutral execution substrate
  Artifact refs / outcomes / explicit Host advance
  Agent Dashboard

Project Adapter
  project CLI / API / dashboard
  project permissions and budget policy
  project-specific artifacts and evidence rules
```

The generic core owns coordination, messages, artifacts/outcomes, optional
governance primitives, and agent-facing interfaces. Adapters own domain
execution and domain evaluation.

## Canonical Map

The canonical diagrams for the current product direction live in
[architecture-map.md](architecture-map.md). That document is the quickest way to
see:

- the product capability stack;
- the Mission -> Host-plan Wave and Mission <-> AgentTeam relations;
- the shared runtime and dashboard infrastructure;
- what is implemented, planned, or transitional.

This file is the compact narrative that explains the same boundary in prose.

## Canonical Product Hierarchy

The product direction is:

```text
Mission -> ordered Host-plan Wave
Mission <-> independent AgentTeam -> AgentTeamRun -> MemberRun
```

- A `Mission` is the durable objective and outcome container.
- A `Wave` is a lightweight, versioned Markdown record of the Host's current
  plan, changed facts, judgment, and advance outcome.
- Agent Team, Dynamic Workflow, and Host work keep distinct runtime truth. A
  Wave may explain their use but does not own their lifecycle.

A Wave is intentionally small. It does not own or require a task graph,
executor attempt, synchronization barrier, or provider session.
Dependencies, branches, worktrees, or workflow fan-out may still exist inside
current implementations, but they are internal execution mechanics, not the
product concept a future operator should start from.

## Active Coordination Contract

Mission/Wave is the only active plan vocabulary. Native ledgers, schemas,
authoring, Mission-Team linkage, assignment correlation, Wave history/advance,
and Mission closeout are implemented. The superseded stack is removed from active reads,
commands, and UI under [ADR 0028](decisions/0028-retire-goal-phase-task-graph.md).
Optional evaluation remains governance layered on an outcome, not a second
closeout model.

## Executor Kinds

### `agent_team`

Use Agent Team when the Mission needs living collaborators with persistent
session state, explicit assignment, handoff, review, and role ownership across
one or more Waves.

The canonical execution proof is message-driven:

```text
TeamMessage(kind=assignment)
  -> correlation_id
  -> Harness blocker / handoff / review / PendingInteraction
  -> explicit outcomes and artifact/check refs
  -> NativeSessionRef for member execution detail
```

Assignment-message correlation replaces legacy dependency graph semantics as the primary
explanation of who owns what. Automatic handoff preserves the
assignment correlation; manual CLI, HTTP, and MCP sends can reuse it directly
or inherit it from a validated same-run causation message.

### `dynamic_workflow`

Use Dynamic Workflow for a one-shot structured execution problem:
plan, compile, run, collect artifacts, and exit. It shares the same provider
runtime substrate, but it is not an Agent Team and does not pretend to be one.

### `host`

Use Host execution when the resident Host Agent does work directly. The host may use
its provider's native subagents internally. Those subagents are host/provider
implementation detail unless optional hooks expose observable delegation facts.

The harness should record observable inputs, outputs, artifacts, and decisions,
not invent canonical child records for provider-native helpers it does not
control.

## Shared Infrastructure Contracts

Different executors keep different semantics, but they should reuse the same
infrastructure contracts where possible.

| Shared contract | Used by |
| --- | --- |
| Provider-neutral execution instance/session substrate | Agent Team member sessions, Dynamic Workflow leaves, Host-driven observed execution, future Standing Agents |
| Capability snapshot and adapter metadata | host plugins, workflow leaves, Agent Team member provisioning |
| Permission and budget ceiling | all executor kinds |
| Artifact references and explicit outcome summaries | all executor kinds |
| Harness coordination stream + ephemeral native activity projection | Agent Team and Host-observable execution; Workflow keeps its own run/step truth |
| Artifact references, outcome summaries, and Host Wave decisions | all execution kinds |

Shared infrastructure does not collapse distinct product objects into one.
Agent Team, Dynamic Workflow, and Host work stay distinct even when one Wave
context refers to several of them.

The repository currently applies a stricter Evidence -> Proposal -> Review ->
Decision -> outcome evaluation chain while self-hosting changes. That is repository
governance during migration, not a mandatory product contract for every Wave.

## Thinking Policy

The target contract makes thinking transient live-only state.

- It may appear in a live host UI or SSE stream when a provider exposes it.
- It is bounded and sanitized.
- It is never persisted as canonical harness history.
- It is never replayable state.
- It is never execution evidence.
- It is never forwarded into another member's context.

Persist Harness-owned coordination, artifact/check references, blockers,
handoffs, control acknowledgements, and explicit outcomes instead. Provider
chat/tool/command/file/turn history remains in the native session.

New Kimi execution does not persist `thinking` actions, and active stores do
not retain historical thinking rows. The Console has a
sanitized `member_activity` SSE preview with expiry: it is delivered only to
currently connected clients, is project-scoped, and is never added to JSONL,
snapshots, replay, messages, or evidence. It is a preview, not an audit trail.

## Current And Future Layers

The near-term product stack is:

```text
Host plugin
  -> Mission/Wave orchestration
  -> executor selection
  -> shared runtime + artifacts + dashboard
```

The later layer is:

```text
Standing Agents + Docs
  -> long-lived business operations
  -> built on the same runtime/artifact/evidence substrate
  -> not part of the current implementation goal
```

Standing Agents + Docs are the current product direction with additive
contracts still being implemented. Documentation must distinguish those
planned Company OS contracts from proven schemas and must never treat Agent
Team runs as standing organizations.

## Current Implementation Boundary

Native Mission/Wave authoring, Agent Team joins and attempts, explicit gates,
Mission closeout, CLI/API/MCP calls, and the Mission-first Dashboard are
implemented. Dynamic Workflow and Host retain their executor-specific truth;
the UI must show an honest unavailable state where routed controls are not yet
implemented. Residual names from the superseded stack are tracked as code
removal debt, not compatibility commitments.

## Surface Responsibility

Keep the responsibility split explicit:

| Surface | Owns | Refuses |
| --- | --- | --- |
| Docs | product hierarchy, architecture boundaries, migration plan | field truth and runtime truth |
| Schemas | machine contracts | roadmap prose |
| Rust code | real runtime, persistence, validation, transport | future-state narrative |
| CLI / MCP / plugins | executable operator and host surfaces | hidden-only workflows |
| Dashboard | read model and safe operator actions | canonical source of truth |

When these surfaces disagree, schema and code describe current reality, while
architecture docs describe the accepted direction and the migration path between
them.
