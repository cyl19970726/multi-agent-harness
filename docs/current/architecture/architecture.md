# Architecture

## Product Boundary

Star Harness is the coordination product. A business project is a tool
environment connected through an adapter.

```text
Star Harness
  Durable AgentTeams / accountable Work / identity-first Messages
  Agent Team control plane / durable Team Supervisor / typed mail
  Dynamic Workflow runtime
  Host-facing plugins, MCP tools, skills, CLI
  Provider-neutral execution substrate
  Artifact refs / outcomes / explicit Host judgment and Work acceptance
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
- the AgentTeam -> TeamMembership -> AgentMember and AgentTeam -> Work relations;
- the shared runtime and dashboard infrastructure;
- what is implemented, planned, or transitional.

This file is the compact narrative that explains the same boundary in prose.

## Canonical Product Hierarchy

The product direction is:

```text
AgentTeam -> TeamMembership -> AgentMember
AgentTeam -> Work (accountable_team_id) -> append-only WorkEvent lineage
AgentTeam -> AgentTeamRun -> MemberRun      # internal diagnostics/history
```

- An `AgentTeam` is the durable agency: one Host membership, one immutable Node
  placement, and a flat Member set. It is created without any Mission.
- A `Work` is the durable unit of accountable responsibility; its append-only
  `WorkEvent` lineage records allocation, blocking, submission, the Host's
  judgment, replan, recovery, and acceptance evidence.
- Agent Team, Dynamic Workflow, and Host work keep distinct runtime truth. A
  Work record may explain their use but does not own their lifecycle.
- `AgentTeamRun` and `MemberRun` are internal diagnostics and history; they
  never scope durable identity or Work responsibility.

A `WorkEvent` is intentionally small. It is not a lifecycle object and
does not own or require a task graph, executor attempt, synchronization
barrier, or provider session.
Dependencies, branches, worktrees, or workflow fan-out may still exist inside
current implementations, but they are internal execution mechanics, not the
product concept a future operator should start from.

## Active Coordination Contract

Durable Teams, Work, and identity-first Messages are the active coordination
vocabulary. Native ledgers, schemas, authoring, Team/Membership linkage, Work
operations, and Host acceptance are implemented. The retired Mission, Mission
Log, and Wave create/update/advance/gate paths are legacy read/export history
only (DOC-108, and ADR 0051 before it). ADR
0050 accepts Agent Team Works as the replacement for
Assignment-message ownership; its schemas/runtime/UI cutover is in progress and
must land without a compatibility ownership path. The superseded stack is removed from active reads,
commands, and UI under [ADR 0028](../../decisions/0028-retire-goal-phase-task-graph.md).
Optional evaluation remains governance layered on an outcome, not a second
closeout model.

## Executor Kinds

### `agent_team`

Use Agent Team when the work needs living collaborators with persistent
session state, explicit Work ownership, review, and role ownership across the
Team's lifetime.

Each `MemberRun` may own one active end-to-end Work and use provider-native
subagents for bounded internal work. The subagents return to that member and do
not gain a Harness mailbox, Workspace identity, Work ownership, or
independent acceptance. Use another Member when those durable properties are
needed.

The accepted execution proof is Work-driven:

```text
Work assignment/claim -> WorkOperation(WorkEvent + resulting Work + deliveries)
  -> WorkDelivery
  -> MemberRun + Workspace + NativeSessionRef
  -> Work block / submission / review / acceptance
  -> linked correlated Message/reply where conversation or pause exists
  -> explicit outcomes and artifact/check refs
```

Work owner and state explain who owns what. Messages may link Work for
discussion but do not assign, submit, or accept it.

Ordinary Host/member/peer collaboration and provider questions stay in
correlated identity-first `Message` rows. Each authorized recipient receives a
separate `CanonicalMessageDelivery`; provider requests and responses are
Message kinds rather than another interaction ledger. Session permissions are frozen before provider
start; in-ceiling work proceeds directly and out-of-ceiling work fails closed. The Host
observes Work state, while minimal blockers make dependent Work ready; there is
no general conditional-delivery graph.

One latest-wins `TeamSupervisorLease` generation owns each active TeamRun's
provider transports, delivery claims, and real Steer/Interrupt/Close controls.
Typed actor and recipient references distinguish Host, Member, stable Agent,
and external Operator provenance. The Supervisor verifies the provider
transport before claiming queued mail, records a provider receipt before
delivery, and fences every cross-process control by generation. An idle member
keeps its native session and remains addressable; only explicit Close is
terminal.

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
| Provider-neutral execution instance/session substrate | Agent Team member sessions, Dynamic Workflow leaves, Host-driven observed execution, future Agent Memberships |
| Capability snapshot and adapter metadata | host plugins, workflow leaves, Agent Team member provisioning |
| Permission and budget ceiling | all executor kinds |
| Artifact references and explicit outcome summaries | all executor kinds |
| Harness coordination stream + ephemeral native activity projection | Agent Team and Host-observable execution; Workflow keeps its own run/step truth |
| Artifact references, outcome summaries, and Host judgments on the Work record | all execution kinds |
| Durable Supervisor lease, typed actor routing, canonical per-recipient delivery claim/receipt/ACK | persistent Agent Team members only |

Shared infrastructure does not collapse distinct product objects into one.
Agent Team, Dynamic Workflow, and Host work stay distinct even when one Work
record refers to several of them.

The repository currently applies a stricter Evidence -> Proposal -> Review ->
Decision -> outcome evaluation chain while self-hosting changes. That is repository
governance during migration, not a mandatory product contract for every Work.

## Thinking Policy

The target contract makes thinking transient live-only state.

- A provider-declared display-safe summary may appear only in the exact Session
  owner's live UI/SSE stream. Host role alone does not grant Member-private live
  access.
- It is bounded and sanitized.
- It is never persisted as canonical harness history.
- It is never replayable state.
- It is never execution evidence.
- It is never forwarded into another member's context.

Persist Harness-owned WorkOperations (resulting Works + WorkEvents + delivery
deltas), authored conversation,
artifact/check references, control acknowledgements, and explicit outcomes instead. Provider
chat/tool/command/file/turn history remains in the native session.

New Kimi execution does not persist `thinking` actions, and active stores do
not retain historical thinking rows. The Console has a sanitized
`live_provider_activity` SSE preview with expiry: it is delivered only to the
currently connected exact AgentMember owner, is Execution-Space scoped, and
is never added to JSONL, snapshots, replay, messages, or evidence. It is a
preview, not an audit trail.

## Current And Future Layers

The near-term product stack is:

```text
Host plugin
  -> durable AgentTeam + Work orchestration
  -> executor selection
  -> shared runtime + artifacts + dashboard
```

The later layer is:

```text
Standing AgentMembers beyond one Team
  -> long-lived business operations
  -> built on the same runtime/artifact/evidence substrate
  -> not part of the current implementation goal
```

Durable AgentTeams, Work, and identity-first Messages are the current
product direction with additive contracts still being implemented.
Documentation must distinguish planned contracts from proven schemas and must
never treat Agent Team runs as standing organizations. The legacy Company OS
contract set is retired (DOC-108).

## Current Implementation Boundary

Durable AgentTeam creation and attempts, Team-run Work, identity-first
Messages, CLI/API/MCP calls, and the operator Dashboard are implemented.
Mission, Mission Log, and Wave are retired legacy read/export surfaces
(DOC-108). Persistent Codex app-server, Claude Agent SDK, and Kimi ACP
members share the same durable Supervisor and typed mailbox contract; bounded
provider exec paths remain Dynamic Workflow-only. Dynamic Workflow and Host
retain their executor-specific truth;
the UI must show an honest unavailable state where routed controls are not yet
implemented. Residual names from the superseded stack are tracked as code
removal debt, not compatibility commitments. In particular, `TeamMessage`,
`TeamMessageProjection`, `team_messages.jsonl`, and their embedded/manual ACK
paths are Legacy read/export only; current clients author `Message` and act on
`CanonicalMessageDelivery`.

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
