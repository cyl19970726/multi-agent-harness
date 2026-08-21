# Agent Workbench Page Specs

This directory owns page-level product and UX specs for Agent Workbench. A page
spec explains why a page or workspace exists, which canonical harness objects it
owns, what workflow proof it must show, and which failure modes it prevents.

Here, "workflow proof" means proof that a user's current task journey is
understandable; it does not refer to the retired Dynamic Workflow product.
Page specs must not restore its routes, controls, or live projections.

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

The current product direction is durable flat AgentTeams with Team-run Work
and identity-first Messages (DEV-35/36/37); durable Team control is owned by
ADR 0044 and the Message/CanonicalMessageDelivery cutover by ADR 0056.
Mission/Mission Log and the Mission-scoped page specs are retired (DOC-108);
the TeamWorkspace and AgentConversationWorkspace surfaces replaced the Team
War Room and MemberRun Focus pages (DEV-38, `schemas/role-views/surface-migration.v1.json`).

| Page | Status | Layout |
| --- | --- | --- |
| [AgentMember Focus](agent-member-focus.md) | proposed | canonical identity, organization projection, and execution trust |
| [Debug](debug.md) | planned secondary surface | current raw objects and source diagnosis |

The deleted Mission Detail, Team War Room, and MemberRun Focus specs described
retired surfaces. Git history is sufficient provenance; those files must not
be used as active product input or recreated as compatibility pages.

## Boundary

- Update the same page spec when the page purpose, canonical object ownership,
  information architecture, action model, dimensions, first viewport,
  breakpoint behavior, or scroll ownership changes.
- Update ../layout-history.md when a selected,
  rejected, or borrowed design decision changes.
