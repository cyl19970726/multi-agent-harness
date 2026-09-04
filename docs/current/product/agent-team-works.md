# Agent Team Work

Status: current
Contract: AFM-2026.08.2

Repository decision: [ADR 0058](../../decisions/0058-work-dependency-dag-and-kernel-boundary.md).
Notion crosswalk: AgentFirm Docs System / `02 Work & Message` owns the
human-facing mental model; this file records the implementation-bound contract.

## Authority

Product doctrine for this topic — Work identity, ownership, the
submission/trust chain, and the Message boundary — is canonical in Notion;
see the single authority-boundary anchor in
`docs/current/documentation-governance.md` (Authority boundary: Notion vs
repository) for the current Notion location. This repository file survives
only as the implementation-bound remainder below.

## Implementation-bound invariants

```text
phase:      open -> active -> review -> closed
condition:  normal | blocked | on_hold
resolution: accepted | cancelled | failed   # closed only
```

`team_id` is a deprecated pre-cutover alias of `accountable_team_id`,
readable through the Rust serde alias and never written by current
binaries.

## Flat Work graph

Every Work is an independent responsibility node. Works do not contain other
Works and have no parent/child identity. Ordering is represented only by hard
dependency edges:

```text
Research ─┐
          ├─> Integration ─> Release review
Runtime  ─┘
```

`Integration` is ready only after both prerequisites are accepted. The same
accepted prerequisite may unlock several successors. The stored prerequisite
set is the forward authority; successors are derived. Dependency writes are
versioned Work operations and reject missing nodes, duplicates, self-edges,
stale revisions, and direct or transitive cycles.

Failed or cancelled prerequisites do not propagate a terminal resolution.
They leave successors not ready and create Host attention for explicit replan.
Changing dependencies on active/review Work is Host-only and reconciles the
execution binding. Terminal Work is immutable.

## Kernel and package boundary

The `firm-core` Work kernel owns lifecycle legality, DAG validation, readiness,
terminal immutability, responsibility, and Module/Gate invariants. `firm-store`
does not sit beneath the application layer. The Work service inside
`firm-application` defines `WorkPersistence` over core contracts and owns
generic `WorkApplication<P>` use cases without importing Store, CLI, UI, or a
Provider package. The crate's separate provider/runtime policy may depend on
the reviewed `firm-runtime-contract`. `firm-store` depends on core +
application and implements that port with locks, CAS, atomic append,
projections, notifications, and recovery. CLI composes the concrete Store and exposes the
same application through CLI, HTTP, and Role Actions. RoleViews return derived
graph facts and Dashboard renders them without recomputing authority.
NodeDaemon and providers execute effects but never own Work state.

## Creation and dependency authority

The Host governs Team-level Work and dependency mutation. Every current Work is
created unassigned. A Member may create an eligible unassigned peer Work inside
its current Work's scope and acceptance boundary; canonical TeamMembership
assignment or claim separately freezes responsibility, and scheduler admission
separately binds the current MemberRun/AgentSession generation. Creation never
records runtime ownership. A Member cannot create a Team-level goal, assign a
peer, cross a Team boundary, expand permission, or change another Work's
acceptance criteria. A Member dependency proposal has no effect until the
kernel accepts the versioned operation. A Message can explain or request the
change but never mutates the graph.

## Module scope

Work Module support is currently a closed built-in mechanism:
`integration-plan@1` plus versioned `WorkModuleBinding` and derived Gate
requirements. There is no installable or dynamically discovered Work Module
registry. Modules may configure namespaced presentation and verification; they
cannot add lifecycle states, bypass graph validation, authorize execution, or
accept Work.

## Dashboard views

Graph and Kanban are first-class views of the same Work projection, not two
products or ledgers. Graph uses `@xyflow/react` as an infinite pan/zoom canvas
with deterministic DAG layout. Its positions and viewport are presentation
state only and are never persisted as Work facts. Kanban has exactly four
phase columns: Open, Active, Review, Closed; condition and resolution appear as
orthogonal card facts.

Both views share filters, selected Work, Inspector, server readiness/reasons,
predecessor/successor navigation, and allowed actions. Graph-node dragging is
visual only. Kanban dragging has no mutation authority in V1. Every lifecycle
or dependency change goes through an explicit authenticated Inspector action;
the browser does not write graph semantics or infer readiness.

Mutation surface (all executable Work mutations):

```bash
firm team-run work list|show|create|assign|redeliver|claim|start|block|resume
firm team-run work release|submit|review|request-changes|accept|cancel|retarget
firm team-run work reconcile-projection|poll-github-ci
```

`redeliver` is the Host re-authorization of an open, never-started Work whose
`WorkDelivery` is frozen on a member generation that no longer runs; it
supersedes that delivery in an explicit `Rebound` WorkOperation without
touching the delivery row or moving responsibility. See
[member-continuation-model.md](../architecture/member-continuation-model.md).

`firm work list` (DOC-106) replaces the retired `firm company work
list/query`; it reads native Work read-only and never falls back to the
former Company task ledger.

| Plane | Examples |
|---|---|
| Work | phase, condition, resolution, owner, report, gate, decision |
| runtime | MemberRun, provider session, process lifecycle |
| Work delivery | `WorkDelivery`: Work allocation/revision transport only |
| Message delivery | `CanonicalMessageDelivery`: per-recipient queued/routed/claimed/provider-received/acknowledged/failed/expired/invalidated state |
| runtime command | `RuntimeCommand`: fenced provider effects and live controls |
| identity | `AgentMember` identity and its `TeamMembership` participation |
