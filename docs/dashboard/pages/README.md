# Agent Workbench Page Specs

This directory owns page-level product and UX specs for Agent Workbench. A page
spec explains why a page or workspace exists, which canonical harness objects it
owns, what workflow proof it must show, and which failure modes it prevents.

Page specs own their own layout contracts. Each page file must include detailed
desktop, tablet, and mobile ASCII diagrams plus first-viewport content, region
dimensions, scroll ownership, and screenshot acceptance questions.

## Page Spec Template

```text
status:
owner_role:
canonical_for:
route_or_surface:
primary_user_question:
why_it_exists:
non_goals:
canonical_objects:
workflow_proof:
source_docs:
read_model_inputs:
page_level_agent_loop:
  designer_options:
  questioner_challenges:
  reviewer_decision:
  rejected_options:
  borrowed_ideas:
selected_information_architecture:
primary_actions:
secondary_actions:
empty_loading_error_states:
responsive_requirements:
layout_contract:
  desktop_ascii:
  tablet_ascii:
  mobile_ascii:
  region_dimensions:
  first_viewport_content:
  scroll_ownership:
  screenshot_acceptance:
failure_modes:
screenshot_acceptance_questions:
open_questions:
```

## Core Page Specs

| Page | Status | Layout |
| --- | --- | --- |
| [Vision overview](vision-overview.md) | planned | page-local contract |
| [Team workspace](team-workspace.md) | planned | page-local contract |
| [AgentMember workbench](agent-member-workbench.md) | planned | page-local contract |
| [Goal document](goal-document.md) | planned | page-local contract |
| [Task document](task-document.md) | planned | page-local contract |
| [Graph/Kanban](graph-kanban.md) | planned | page-local contract |
| [Docs context](docs-context.md) | planned | page-local contract |
| [Evidence/Review/Decision](evidence-review-decision.md) | planned | page-local contract |
| [Warnings/repair](warnings-repair.md) | planned | page-local contract |
| [Debug](debug.md) | planned | page-local contract |

## Boundary

- Update the same page spec when the page purpose, canonical object ownership,
  information architecture, action model, dimensions, first viewport,
  breakpoint behavior, or scroll ownership changes.
- Update [../layout-decisions.md](../layout-decisions.md) when a selected,
  rejected, or borrowed design decision changes.
- `../hard-layout-specs/` is historical only and must not receive current page
  layout contracts.
