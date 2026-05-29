# Agent Workbench Page Specs

This directory owns page-level product and UX specs for Agent Workbench. A page
spec explains why a page or workspace exists, which canonical harness objects it
owns, what workflow proof it must show, and which failure modes it prevents.

Page specs do not own CSS dimensions, exact ASCII diagrams, or scroll geometry.
Those belong in [../hard-layout-specs/](../hard-layout-specs/).

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
links_to_hard_layout_specs:
failure_modes:
screenshot_acceptance_questions:
open_questions:
```

## Core Page Specs

| Page | Status | Hard layout |
| --- | --- | --- |
| [Vision overview](vision-overview.md) | planned | pending |
| [Team workspace](team-workspace.md) | planned | pending |
| [AgentMember workbench](agent-member-workbench.md) | planned | pending |
| [Goal document](goal-document.md) | planned | pending |
| [Task document](task-document.md) | planned | pending |
| [Graph/Kanban](graph-kanban.md) | planned | pending |
| [Docs context](docs-context.md) | planned | pending |
| [Evidence/Review/Decision](evidence-review-decision.md) | planned | pending |
| [Warnings/repair](warnings-repair.md) | planned | pending |
| [Debug](debug.md) | planned | pending |

## Boundary

- Update a page spec when the page purpose, canonical object ownership,
  information architecture, or action model changes.
- Update a hard layout spec when dimensions, first viewport, breakpoint
  behavior, or scroll ownership changes without changing the page meaning.
- Update [../layout-decisions.md](../layout-decisions.md) when a selected,
  rejected, or borrowed design decision changes.
