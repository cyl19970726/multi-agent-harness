# Rejected Workbench Implementations

This directory records failed browser-visible frontend attempts. These records
prevent the next implementation from repeating a failed product shape.

A rejected implementation is not a shame log and not a changelog. It is a
design gate artifact: screenshot first impression, violated gates, old-code
contamination, why patching is not enough, and where to restart.

## Required Record

```text
Rejected Implementation
  attempt:
  branch_or_pr:
  screenshot_refs:
  first_impression:
  violated_hard_gates:
  mismatch_with_selected_layout:
  old_dashboard_contamination:
  why_not_patchable:
  what_to_restart_from:
  code_disposition:
  reviewer_decision:
```

## Records

| Attempt | Status | Restart point |
| --- | --- | --- |
| [PR #6 Agent Workbench shell](pr-6-agent-workbench-shell.md) | rejected | page specs + shell-v2 + architecture restart |
