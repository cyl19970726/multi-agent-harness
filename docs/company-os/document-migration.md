# Documentation migration map

This map prevents older, still-useful execution documentation from silently
reasserting the former product model.

## Status meanings

- **Canonical**: current product or architecture authority.
- **Execution canonical**: current authority inside the execution foundation.
- **Compatibility**: implemented/readable behavior retained during migration.
- **Historical**: decision provenance; do not use to design new product work.
- **Superseded**: replaced for new design; keep only until links and readers are
  migrated.

## Current map

| Document family | Status | Replacement or retained scope |
| --- | --- | --- |
| `docs/company-os/**` | Canonical | Company OS product, objects, governance, UI, and examples |
| `docs/prd.md`, `docs/architecture-map.md` | Canonical | Repository-level product requirements and architecture entry |
| ADR 0027 | Canonical | Docs + Organization product center and WorkItem bridge |
| ADR 0026 | Execution canonical | Mission/Wave hierarchy, executor boundaries, compatibility, transient thinking |
| ADR 0025 and Agent Team page specs | Execution canonical | Agent Team and MemberRun control plane only |
| Dynamic Workflow/runtime/provider docs | Execution canonical | Provider-neutral execution details |
| `docs/archive/legacy-goal-task-v1/VISION.md` | Archived | Replaced by `docs/company-os/vision.md` for product direction |
| Legacy execution-loop material | Archived | Replaced by ADR 0026 and `execution-foundation.md` for new execution |
| Legacy operating-loop material | Archived | Retained only to interpret old records; not a Company OS work contract |
| Archived ADR 0019 and Goal/Task Workbench designs | Archived | Replaced by Company OS IA and WorkItem model |
| Archived ADR 0024 | Archived | Retained only to interpret legacy phase record modes; Waves do not inherit legacy dependency graph semantics |
| `docs/concept-model.md`, `docs/data-model.md`, `docs/core-modules.md`, `docs/architecture.md` | Superseded product framing, compatibility reference | Migrate reusable implementation detail into Company OS docs or execution docs |
| Existing Dashboard read-model/runbook docs | Compatibility | Retain implemented execution operations; update product IA through Company OS specs |

## Migration rules

1. Do not delete ledgers or rewrite old IDs to make the new model appear native.
2. Add a replacement header when an older document is likely to guide new
   product design incorrectly.
3. Keep executable implementation detail until its new canonical home exists.
4. Update `docs/registry.json` after canonical paths and lifecycle statuses are
   stable.
5. A document is not fully retired while code, schemas, tests, or operators
   still depend on it; label that dependency instead.
