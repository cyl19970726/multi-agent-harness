# Documentation migration map

This map prevents older, still-useful execution documentation from silently
reasserting the former product model.

## Status meanings

- **Canonical**: current product or architecture authority.
- **Execution canonical**: current authority inside the execution foundation.
- **Implemented**: active behavior whose schemas, stores, APIs, or UI exist.
- **Historical**: decision provenance; do not use to design new product work.
- **Superseded**: replaced for new design and scheduled for archive/removal.

## Current map

| Document family | Status | Replacement or retained scope |
| --- | --- | --- |
| `docs/company-os/**` | Canonical | Company OS product, objects, governance, UI, and examples |
| `docs/prd.md`, `docs/architecture-map.md` | Canonical | Repository-level product requirements and architecture entry |
| ADR 0027 | Canonical | Docs + Organization product center and WorkItem bridge |
| ADR 0026 + ADR 0028 | Execution canonical | Mission/Wave hierarchy, executor boundaries, retired-stack boundary, transient thinking |
| ADR 0025 and Agent Team page specs | Execution canonical | Agent Team and MemberRun control plane only |
| Dynamic Workflow/runtime/provider docs | Execution canonical | Provider-neutral execution details |
| `docs/archive/legacy-goal-task-v1/VISION.md` | Archived | Replaced by `docs/company-os/vision.md` for product direction |
| Legacy execution-loop material | Archived | Replaced by ADR 0026 and `execution-foundation.md` for new execution |
| Legacy operating-loop material | Archived | Retained only to interpret old records; not a Company OS work contract |
| Archived ADR 0019 and Goal/Task Workbench designs | Archived | Replaced by Company OS IA and WorkItem model |
| Archived ADR 0024 | Archived | Removal provenance only; not active planning context |
| `docs/concept-model.md`, `docs/data-model.md`, `docs/architecture.md` | Execution canonical | Native Mission/Wave relationships, projections, and source-of-truth rules |
| Existing Dashboard read-model/runbook docs | Implemented | Active execution operations aligned to Mission Canvas and Team War Room |

## Migration rules

1. Export and verify historical stores before deletion when preservation is
   required; explicitly disposable stores may be cleaned directly.
2. Add a replacement header when an older document is likely to guide new
   product design incorrectly.
3. Keep executable implementation detail until its new canonical home exists.
4. Update `docs/registry.json` after canonical paths and lifecycle statuses are
   stable.
5. A retired object is never kept in active documentation merely because a
   residual internal field still needs code cleanup; track that debt directly.
