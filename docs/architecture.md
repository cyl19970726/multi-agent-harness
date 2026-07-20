# Architecture

## Product Boundary

Star Harness is the coordination product. A business project is a tool
environment connected through an adapter.

```text
Star Harness
  Mission / Wave / executor selection
  Agent Team control plane
  Dynamic Workflow runtime
  Host-facing plugins, MCP tools, skills, CLI
  Provider-neutral execution substrate
  Artifact refs / outcomes / lightweight Wave gate
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
- the Mission -> Wave -> executor hierarchy;
- the shared runtime and dashboard infrastructure;
- what is implemented, planned, or transitional.

This file is the compact narrative that explains the same boundary in prose.

## Canonical Product Hierarchy

The product direction is:

```text
Mission -> Wave -> executor
```

- A `Mission` is the durable objective and outcome container.
- A `Wave` is a lightweight ordered unit inside a Mission. It has objective,
  exit criteria, status, executor reference, outcome, and a lightweight gate.
- An executor is one of `agent_team`, `dynamic_workflow`, or `host`.

A Wave is intentionally small. It does not own or require a legacy dependency graph.
Dependencies, branches, worktrees, or workflow fan-out may still exist inside
current implementations, but they are internal execution mechanics, not the
product concept a future operator should start from.

## Compatibility Terms

Mission/Wave is the canonical product vocabulary. Native Mission/Wave ledgers,
schemas, public authoring, Agent Team linkage, retry lineage, and the Wave gate
now exist, while older Goal surfaces remain readable compatibility paths.

| Canonical product term | Current compatibility surface | Rule |
| --- | --- | --- |
| `Mission` | Read-only provenance projection from existing `Goal` rows | Native Mission is authoritative for new work. Compatibility projections use `compat-goal:*` ids and are never written back. |
| `Wave` | Existing `legacy phase record` ids are compatibility provenance only | Native Wave is authoritative for new work. A legacy phase record is never synthesized into a Wave because its dependency graph semantics differ. |
| Mission closeout | Optional legacy `outcome evaluation` | Native closeout uses an explicit Mission outcome summary; richer evaluation remains an optional compatibility/governance layer. |

The migration is non-destructive: old ledgers stay readable and are not
rewritten by Mission/Wave commands.

## Executor Kinds

### `agent_team`

Use Agent Team when the Wave needs living collaborators with persistent session
state, explicit assignment, handoff, review, and role ownership inside the Wave.

The canonical execution proof is message-driven:

```text
TeamMessage(kind=assignment)
  -> correlation_id
  -> MemberAction / blocker / handoff / review_result / delegation
  -> artifacts, checks, and explicit outcomes
```

Assignment-message correlation replaces legacy dependency graph semantics as the primary
explanation of who owns what inside a Wave. Automatic handoff preserves the
assignment correlation; manual CLI, HTTP, and MCP sends can reuse it directly
or inherit it from a validated same-run causation message.

### `dynamic_workflow`

Use Dynamic Workflow when the Wave is a one-shot structured execution problem:
plan, compile, run, collect artifacts, and exit. It shares the same provider
runtime substrate, but it is not an Agent Team and does not pretend to be one.

### `host`

Use `host` when the resident Host Agent does the Wave directly. The host may use
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
| Durable event stream and dashboard read model | Agent Team, Workflow runs, Host-observable execution |
| Artifact references, outcome summaries, and lightweight Wave gate | all executor kinds |

Shared infrastructure does not collapse distinct product objects into one. A
Wave executed by Agent Team is still different from a Wave executed by Dynamic
Workflow or directly by the host.

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

Persist explicit actions, artifacts, summaries, blockers, and outcomes instead.

New Kimi execution no longer persists `thinking` actions, and current snapshots
hide historical thinking rows without deleting the ledger. The Console now has a
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

Standing Agents + Docs is future architecture. The current documentation goal
must not imply that it is already implemented or that Agent Team runs are
standing organizations.

## Compatibility Migration

The accepted migration is staged and non-destructive:

1. Docs: make Mission/Wave canonical, mark Goal/legacy phase record references
   transitional, and add one architecture map.
2. Schema and store: implemented for native Mission/Wave plus non-destructive
   Goal compatibility projection.
3. Runtime: Agent Team joins, attempts, and gate are implemented; Dynamic
   Workflow and Host Wave routing remain.
4. CLI/API/MCP/Dashboard: native authoring and the Mission-first Console are
   implemented for Agent Team Waves. Dynamic Workflow and Host retain executor
   seams rather than falsely claiming routed Console execution.
5. Stored data, fixtures, tests, skills, and governance: update validators,
   snapshots, docs registry, and acceptance paths after the runtime seam is
   stable.

No stage should require deleting existing runtime code before the replacement is
proven.

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
