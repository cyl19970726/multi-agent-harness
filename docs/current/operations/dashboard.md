# Agent Workbench

## Authority

Product doctrine for this topic — Workbench information architecture,
executor control-plane views, and Agent Team ownership visibility — is
canonical in Notion; see the single authority-boundary anchor in
`docs/current/documentation-governance.md` (Authority boundary: Notion vs
repository) for the current Notion location. This repository file
survives only as the implementation-bound remainder below.

## Implementation-bound invariants

`Agent Workbench` is the product name; `Agent Dashboard` remains a
compatibility module/path name in `apps/agent-dashboard`, snapshots, and
commands — do not rename the directory or CLI/API strings to match the
product name without a coordinated migration.

Retired coordination pages (Mission detail, Team War Room, retired Company OS pages) are not part of active navigation or authoring.

| Document | Owns |
| --- | --- |
| `docs/current/architecture/architecture-map.md` | cross-module product and runtime map |
| `docs/current/operations/dashboard.md` | Workbench product purpose and information architecture |
| `docs/current/dashboard/pages/*.md` | page purpose, proof, actions, and layout contracts |
| `docs/current/dashboard/frontend-architecture.md` | frontend modules, routing, and read-model plumbing |
| `apps/agent-dashboard/src/model/*.ts` | implemented projections and selectors |
| `docs/current/dashboard/runbook.md` | local run/build/snapshot entry points |
| `docs/current/dashboard/frontend-design.md` | shared visual doctrine and layout decisions |
