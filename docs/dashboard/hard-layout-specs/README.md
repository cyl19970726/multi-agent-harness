# Agent Workbench Historical Hard-Layout Attempts

This directory is historical/deprecated. It keeps old hard-layout attempts so
future reviewers can understand why the Workbench reset happened.

Current implementation layout contracts no longer live here. They live inside
each page document under `docs/dashboard/pages/<page>.md` in the `## Layout
Contract` section. A new implementation must read those page-local contracts,
not this directory.

## Historical Fields

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
| [shell-v2](shell-v2.md) | deprecated | old shell reset | Superseded by page-local layout contracts. |
| [agent-workbench-shell-v1](agent-workbench-shell-v1.md) | deprecated | old shell | Superseded after PR #6 showed the spec was too vague. |

## Boundary

- Hard specs may include ASCII diagrams, dimensions, breakpoint behavior,
  scroll ownership, first-viewport content, and screenshot acceptance for
  historical analysis.
- Current page layouts must be updated in [../pages/](../pages/), not here.
- Implementation cannot begin from this directory. It must begin from accepted
  page-local layout contracts.
