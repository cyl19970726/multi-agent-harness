# Agent Workbench Hard Layout Specs

This directory owns screenshot-verifiable implementation layout contracts.
Hard layout specs are written only after page specs and layout decisions are
clear enough to remove implementation ambiguity.

Page specs explain why a page exists. Hard layout specs define how the accepted
page or slice must appear in desktop, tablet, and mobile browsers.

## Required Fields

```text
spec_id:
status:
implements_page_specs:
source_of_truth_boundary:
reviewer_decision:
desktop_wireframe:
  viewport:
  ascii_diagram:
  fixed_dimensions:
  first_viewport_content:
  scroll_containers:
tablet_wireframe:
  viewport:
  ascii_diagram:
  collapsed_regions:
  first_viewport_content:
mobile_wireframe:
  viewport:
  ascii_diagram:
  tab_order:
  first_viewport_content:
state_matrix:
forbidden_primary_surfaces:
screenshot_acceptance:
rejected_when:
```

## Specs

| Spec | Status | Implements | Notes |
| --- | --- | --- | --- |
| [shell-v2](shell-v2.md) | draft | page-ready shell | New restart point for the next implementation. |
| [agent-workbench-shell-v1](agent-workbench-shell-v1.md) | deprecated | old shell | Superseded after PR #6 showed the spec was too vague. |

## Boundary

- Hard specs may include ASCII diagrams, dimensions, breakpoint behavior,
  scroll ownership, first-viewport content, and screenshot acceptance.
- Hard specs must not redefine product purpose or page semantics. Update
  [../pages/](../pages/) first when meaning changes.
- Implementation cannot begin from a hard spec with `status: draft` unless the
  Reviewer records `continue`.
