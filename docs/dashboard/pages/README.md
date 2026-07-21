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

The current product direction is Mission -> ordered Wave -> executor, owned by
[the architecture map](../../architecture-map.md) and
[ADR 0026](../../decisions/0026-mission-wave-architecture.md). The implemented
primary pages are the Mission/Wave Canvas and Agent Team War Room. Historical
Vision/Goal/Task Work-board and Goal Workbench specs are archived; they do not
define the new information architecture.

| Page | Status | Layout |
| --- | --- | --- |
| [Mission/Wave Canvas](mission-wave-canvas.md) | implemented | ordered execution and gate contract |
| [Agent Team War Room](team-run-war-room.md) | implemented | one linked AgentTeamRun attempt |
| [MemberRun Focus](member-run-focus.md) | planned | run-scoped member detail |
| [Team workspace](team-workspace.md) | future Standing Agents | page-local concept |
| [AgentMember workbench](agent-member-workbench.md) | compatibility/future Standing Agent | page-local contract |
| [Evidence/Review/Decision](evidence-review-decision.md) | planned | page-local contract |
| [Warnings/repair](warnings-repair.md) | planned | page-local contract |
| [Debug](debug.md) | planned | page-local contract |

## Boundary

- Update the same page spec when the page purpose, canonical object ownership,
  information architecture, action model, dimensions, first viewport,
  breakpoint behavior, or scroll ownership changes.
- Update [../layout-history.md](../../company-os/frontend-information-architecture.md) when a selected,
  rejected, or borrowed design decision changes.
