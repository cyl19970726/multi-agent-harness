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

The Vision, Task, Work-board (graph/Kanban), and Docs surfaces are still owned
by [../work-board-design.md](../work-board-design.md) and ADR
[0019](../../decisions/0019-vision-goal-task-workbench-redesign.md) (light Notion
document layout + unified Work board). The Goal surface now has a page-local
contract again because `goal-goal-workbench-v1` narrows Goal detail into the
phase-first workbench and screenshot-gated acceptance surface.

| Page | Status | Layout |
| --- | --- | --- |
| [Goal Workbench](goal.md) | active | page-local contract |
| [Team workspace](team-workspace.md) | planned | page-local contract |
| [AgentMember workbench](agent-member-workbench.md) | planned | page-local contract |
| [Evidence/Review/Decision](evidence-review-decision.md) | planned | page-local contract |
| [Warnings/repair](warnings-repair.md) | planned | page-local contract |
| [Debug](debug.md) | planned | page-local contract |

## Boundary

- Update the same page spec when the page purpose, canonical object ownership,
  information architecture, action model, dimensions, first viewport,
  breakpoint behavior, or scroll ownership changes.
- Update [../layout-history.md](../layout-history.md) when a selected,
  rejected, or borrowed design decision changes.
