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

The current product direction is Mission context -> append-only Mission Log ->
one flat AgentTeam, owned by ADR 0051 and ADR 0034, while
durable Team control is owned by ADR 0044 and the current identity-first
Message/CanonicalMessageDelivery cutover by ADR 0056. The
implemented primary pages are Mission Detail and Log and Agent Team War Room. Historical
Vision/Goal/Task Work-board and Goal Workbench specs are archived; they do not
define the new information architecture.

| Page | Status | Layout |
| --- | --- | --- |
| [Mission Detail and Log](mission-detail-log.md) | implemented | durable context, Mission-owned Team, append-only Host judgment/replan/recovery/closeout log |
| [Agent Team War Room](team-run-war-room.md) | implemented | one Mission-owned, Node-placed long-lived TeamRun |
| [MemberRun Focus](member-run-focus.md) | implemented candidate | run-scoped member detail |
| [AgentMember Focus](agent-member-focus.md) | proposed | canonical identity, organization projection, and execution trust |
| [Debug](debug.md) | planned secondary surface | current raw objects and source diagnosis |

The deleted Team workspace, AgentMember workbench, Evidence/Review/Decision,
and Warnings/repair specs described the retired Goal/Task/Proposal/Gap stack.
Git history is sufficient provenance; those files must not be used as active
product input or recreated as compatibility pages.

## Boundary

- Update the same page spec when the page purpose, canonical object ownership,
  information architecture, action model, dimensions, first viewport,
  breakpoint behavior, or scroll ownership changes.
- Update ../layout-history.md when a selected,
  rejected, or borrowed design decision changes.
