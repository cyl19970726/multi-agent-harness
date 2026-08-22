# Architecture

## Authority

Product doctrine for this topic — the product/adapter boundary, canonical
product hierarchy, executor kinds, and surface responsibilities — is
canonical in Notion; see the single authority-boundary anchor in
`docs/current/documentation-governance.md` (Authority boundary: Notion vs
repository) for the current Notion location. The canonical diagrams live
in [architecture-map.md](architecture-map.md). This repository file
survives only as the implementation-bound remainder below, and stays
registered in `docs/registry.json` as a `core_docs` entry enforced by
`harness governance check`.

Dynamic Workflow is retired. Current execution architecture consists of Agent
Team coordination and Host/provider-local implementation details; historical
Workflow records are available only through legacy archive
export/verify/restore-read and are never a second live ledger.

## Implementation-bound invariants

| Surface | Owns | Refuses |
| --- | --- | --- |
| Docs | product hierarchy, architecture boundaries, migration plan | field truth and runtime truth |
| Schemas | machine contracts | roadmap prose |
| Rust code | real runtime, persistence, validation, transport | future-state narrative |
| CLI / MCP / plugins | executable operator and host surfaces | hidden-only workflows |
| Dashboard | read model and safe operator actions | canonical source of truth |

When these surfaces disagree, schema and code describe current reality,
while architecture docs describe the accepted direction and the migration
path between them.

### Work package flow

```text
firm-core Work kernel
  <- firm-application: WorkPersistence port + generic WorkApplication<P>
       <- firm-store: concrete port implementation + atomic persistence
            <- CLI composition root -> HTTP / MCP / Role Actions
                                  -> server RoleViews -> Dashboard
```

Only the kernel owns lifecycle legality, hard-dependency DAG validation,
readiness, and terminal immutability. The Work application service defines a
core-facing persistence port without importing Store/CLI/Provider; the wider
application crate may retain its reviewed runtime-contract policy dependency.
Store depends on application + core and
implements that port. CLI composes them. Store owns CAS and atomic append, not
policy, and transports do not reproduce use cases. RoleViews expose
predecessors, derived successors, readiness, and reasons; Dashboard renders
those facts and submits authorized commands. NodeDaemon, provider packages,
and Modules do not own Work state. See
[ADR 0058](../../decisions/0058-work-dependency-dag-and-kernel-boundary.md).

Graph and Kanban consume the same RoleView. The Graph renderer uses
`@xyflow/react`; deterministic layout coordinates, viewport, and selection are
ephemeral presentation state and have no schema or Store representation.
Kanban derives exactly Open/Active/Review/Closed from server phase. Both reuse
one Inspector and allowed-action path. Dragging cannot authorize a dependency
or lifecycle mutation in V1, and no browser component may become a semantic
graph writer.

`TeamMessage`, `TeamMessageProjection`, `team_messages.jsonl`, and their
embedded/manual ACK paths are legacy read/export only; current clients
author `Message` and act on `CanonicalMessageDelivery`.
