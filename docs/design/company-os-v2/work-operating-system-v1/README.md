# Work Operating System expected designs

This directory defines the next Work design contract on top of the approved
Company OS V2.2 visual language. It expands the original single ledger screen
into a coherent family of views over one WorkItem truth.

## Review sequence

```text
Current Store-live
  -> one trademark WorkItem, no native Milestone
Expected target
  -> multi-business-line Overview / Board / All Work / Milestones / Workload / Detail
Future actual
  -> browser captures after the native model and projections are implemented
```

The expected screens are product intent, not evidence that Milestone,
multi-business-line queries, or workload capacity already exist.

## Required screens

1. `work-overview--desktop-1536x1024.png`
2. `work-board--desktop-1536x1024.png`
3. `work-all--desktop-1536x1024.png`
4. `work-milestones--desktop-1536x1024.png`
5. `work-timeline--desktop-1536x1024.png`
6. `work-workload--desktop-1536x1024.png`
7. `work-item-focus--desktop-1536x1024.png`

The first generation round concentrates on desktop information architecture.
Responsive expected designs follow after the desktop family is reviewed so a
premature mobile composition does not constrain the core model.

## Shared visual direction

- warm ivory editorial workbench, fine graphite borders, coral pressure accent;
- left Company OS navigation, calm page header, view tabs, and compact filters;
- serious operating density with generous grouping rhythm;
- actor portraits and semantic line icons as recognition aids;
- no decorative chart wall, oversized gradients, glassmorphism, or card soup;
- dense data remains code-renderable and accessible; generated raster assets
  are references, never shipped UI surfaces.

See [Work Operating System](../../../company-os/work-operating-system.md) for
the canonical product contract and `prompts/` for the exact generation briefs.
