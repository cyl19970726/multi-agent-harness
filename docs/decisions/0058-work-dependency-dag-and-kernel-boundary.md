# ADR 0058: Work Dependency DAG And Kernel Boundary

```text
status: accepted; implementation cutover owned by DEV-60
date: 2026-08-22
amends: ADR 0025, ADR 0033, ADR 0037, ADR 0039, ADR 0044, ADR 0050
canonical_for: flat Work dependency graph, Work readiness, Work kernel package boundary,
  constrained Member dependency proposals, and Work Module boundary
notion_crosswalk: AgentFirm Docs System / 02 Work & Message is the product
  mental model; this ADR owns the repository implementation contract
```

## Context

Work already has durable identity, lifecycle, responsibility, operations,
deliveries, multiple prerequisite ids, and explicit Host acceptance. Its first
design deliberately stopped short of a general dependency graph and allowed a
Member-owned parent/child Work hierarchy. Dynamic Workflow has since retired,
AgentTeams are flat, and parent/child Work now creates a second, misleading
topology beside prerequisites.

The product needs one comprehensible model: independent Works form a directed
acyclic graph (DAG). An edge says only that one Work cannot become executable
until another Work is accepted. It does not transfer ownership, imply Team
nesting, authorize execution, or accept either Work.

The existing package split also needs a hard boundary. Reducing file size does
not prevent CLI, Store, Dashboard, or Modules from each inventing lifecycle or
readiness rules.

## Decision

### Work is a flat node; dependencies are directed edges

For an edge `A -> B`, B depends on A. A Work may have zero, one, or many
prerequisites; one accepted Work may unlock many successors. Successors are a
derived reverse projection, never a second writable relationship.

V1 has one edge type: hard `depends_on`. Do not add soft, optional,
`converges_into`, parent, child, phase, or container edges without a later ADR.
Work decomposition creates ordinary peer Works and, where order matters,
explicit dependency edges.

The graph is scoped to authoritative Work identities. Cross-Team
responsibility remains an explicit `WorkDelegation`; a dependency never
replaces delegation and target acceptance never accepts the source Work.

### DAG and readiness invariants

The canonical dependency mutation is versioned, idempotent, attributed, and
recorded in the same ordered Work operation history as the affected Work. The
application loads the affected graph, the kernel validates it, and the Store
commits the operation and resulting projection atomically.

Every dependency write must reject:

- self-dependency;
- a missing or out-of-scope prerequisite;
- a duplicate edge;
- any direct or transitive cycle; and
- a stale expected Work or dependency-set revision.

A Work is ready to claim or start only when it is open, normal, permitted by
its assignment policy, and every prerequisite is terminal with resolution
`accepted`. Readiness and its reasons are deterministic kernel projections.
The Dashboard and transports display them; they do not calculate them.

An accepted prerequisite triggers recomputation and notification, not an
automatic claim or start. A failed or cancelled prerequisite leaves each
successor not ready and creates Host attention for replan. It never silently
fails, cancels, rewires, or accepts downstream Work. Changing dependencies on
active or review Work requires explicit Host authority and reconciliation of
the current execution binding. Terminal Work is immutable.

### Creation authority and proposals

The Host may create Team-level Work, change dependency edges, assign Work, and
govern acceptance. A Member may create self-owned or eligible unassigned Work
inside the scope and acceptance boundary of Work it already owns. This is a
peer Work, not a child Work.

A Member may propose dependency changes within that same boundary. The kernel
must reject proposals that create Team-level goals, expand permissions, cross
Teams, assign a peer, alter the parent Work's acceptance criteria, or evade
Host-only reconciliation. Until the reviewed proposal operation is present on
all mutation surfaces, Members report the proposed nodes and edges to the Host
through an ordinary Work-linked Message; the Message itself changes nothing.

### Package ownership

```text
firm-core / Work kernel
  model · lifecycle · operation · dependency · responsibility · module/gate invariants
                         ^
                         |
firm-application         |  depends only on firm-core
  WorkPersistence port · generic WorkApplication<P> · Work use cases
          ^
          | implements port
firm-store               |  depends on firm-application + firm-core
  load/lock/CAS · atomic append · rebuildable projections · outbox/attention
          ^
          |
CLI composition root ----+----> HTTP / MCP / Role Actions / RoleViews
                               Dashboard renders server facts
```

The Work kernel is pure domain policy. It must not depend on Store, CLI, HTTP,
MCP, Dashboard, NodeDaemon, provider adapters, GitHub, or Notion. It owns the
three lifecycle axes, legal operations, graph validation, readiness, terminal
immutability, responsibility invariants, and Module/Gate constraints.

`firm-application` depends only on `firm-core`. It defines the
`WorkPersistence` port and generic `WorkApplication<P>` use cases; it never
imports the concrete Store or a runtime package. `firm-store` depends on
`firm-core` plus `firm-application` and implements that port with concurrency
control, atomic append, rebuildable projections, notifications, and recovery.
It does not invent state transitions. The CLI is the composition root: it
wires the concrete Store into the generic application and exposes the same use
cases through every mutation transport. NodeDaemon and providers execute
effects but own no Work state. Dashboard submits commands and renders server
facts; it is not a readiness authority.

The Work kernel may remain a cohesive module inside `firm-core`. A separate
crate is justified only by a real reuse or dependency boundary; an empty
layering crate is not required. Maintained source files remain at most 1,500
lines, but line count is an outcome check rather than the package design.

### Module boundary

The implemented Module scope at this decision is closed and narrow:
`integration-plan@1` is a built-in definition and `WorkModuleBinding` freezes
its exact version, resolved configuration, and fingerprint for gate creation.
There is no general installable or dynamically discovered Work Module registry.

A Module may contribute namespaced configuration, presentation metadata, and
versioned Gate requirements. A future reviewed extension may let it propose
Work nodes or dependency edges, but core authorization, cycle checks,
readiness, lifecycle, and acceptance always remain kernel-owned. A Module
cannot add lifecycle states, directly mutate Work, authorize provider effects,
or treat its own completion as Host acceptance. Multiple-module action names
and configuration must be namespaced and conflict-checked before any open
registry is claimed.

## Supersession

This ADR supersedes only the following clauses; the rest of each historical
decision remains evidence and, where stated, active:

- ADR 0025 and ADR 0044: “no Task Graph” means no separate universal task
  ledger or workflow executor; it no longer excludes the Work dependency DAG.
- ADR 0033: Harness still does not schedule Git/worktree steps, but Work may
  declare kernel-owned dependencies independent of workspace mechanics.
- ADR 0037 and ADR 0039: dependency scheduling is no longer only manual or
  encoded in conversation; Work edges are durable and readiness is derived.
- ADR 0050: the rejection of a general dependency graph, the claim that
  dependencies are merely minimal blockers, and Member-created child Work are
  superseded. Work is flat and dependencies form the one current DAG.

ADR 0028 remains active: its retired Goal/GoalPhase/Task graph is not revived.
The Work DAG operates only over the current Work authority.

## Consequences

- Parallel fan-out and multi-prerequisite fan-in are explicit and queryable.
- Parent/child Work fields and UI disappear from active schemas, code,
  fixtures, APIs, Skills, and docs; historical evidence remains readable under
  an exact allowlist.
- Failed or cancelled prerequisites become visible Host replan work rather
  than hidden propagation.
- All transports share one legal-operation and readiness implementation.
- Module claims remain honest until an actual open registry, conflict policy,
  and extension API are implemented and reviewed.

## Validation

The cutover is accepted only when:

1. active roots contain zero `parent_work_id`, `child Work`, or equivalent
   current-contract surfaces outside the exact read-only core decode seam,
   its compatibility test, and the explicit historical allowlist;
2. self-edge, duplicate, missing prerequisite, direct cycle, transitive cycle,
   fan-in, fan-out, stale revision, concurrent update, failed prerequisite, and
   cancelled prerequisite tests pass;
3. CLI, HTTP, MCP, Role Actions, RoleViews, and Dashboard use the same
   application/kernel semantics;
4. package-boundary checks require application-to-core-only dependency,
   Store implementation of the application port, CLI composition, no
   core-to-outer-layer dependency, and no maintained file over 1,500 lines;
5. Dashboard graph facts come from server projections and explain readiness;
6. canonical Skills and generated plugin mirrors teach peer Work plus explicit
   dependencies and contain no child-Work procedure; and
7. Notion's Work & Message mental model and this repository crosswalk describe
   the same flat-DAG model at the reviewed revision.
