# Page Design Workflow

Use this reference when the frontend work needs concrete page-level UI/UX
design, not only a top-level layout direction.

## Core Page Discovery

Identify pages from product mechanisms, not from the current component tree.

```text
Vision
  -> final acceptance
  -> operator workflows
  -> core objects
  -> failure modes
  -> pages/workspaces
  -> page-level UI/UX specs
```

For every proposed page, fill this card:

```text
Core Page
  name:
  route:
  why_it_exists:
  vision_link:
  primary_user_question:
  canonical_objects:
  workflow_proof:
  failure_modes_prevented:
  primary_actions:
  safe_action_contracts:
  read_model_needs:
  desktop_layout:
  tablet_layout:
  mobile_layout:
  browser_acceptance:
```

A page is core only if it directly helps the operator understand, execute,
verify, decide, or improve the harness workflow. If a page only exposes raw
state, keep it in debug.

## Page-Level Option Loop

Run a 2-3 option loop for a core page when its layout affects workflow proof or
operator behavior.

```text
Core page brief
  -> Designers propose 2-3 page options
  -> Questioner challenges each option
  -> Reviewer / Lead selects, synthesizes, or requests more options
  -> rejected options are recorded
  -> loop stop/continue reason is recorded
  -> selected page spec is added to the design draft
```

Use multiple Designer subagents when available. If only one Designer is
available, run separate passes with different constraints. A core page or module
cannot move to implementation from a single unchallenged option.

Each option should include:

```text
Page Option
  name:
  layout:
  visual_hierarchy:
  primary_components:
  interactions:
  object_mapping:
  realtime_behavior:
  docs_context:
  graph_kanban_behavior:
  desktop_tablet_mobile:
  risks:
  implementation_cost:
```

Questioner checks:

- Does this page prove the harness workflow, or only look active?
- Is the page grounded in Vision and final acceptance?
- Does it preserve canonical object boundaries?
- Does it prevent fake assignment, fake realtime state, provider-only claims,
  missing evidence, missing review, or missing GoalEvaluation?
- Does it keep docs as mounted context rather than a copied source of truth?
- Does it work on mobile without horizontal overflow?

Continue the page loop when options do not expose real tradeoffs, key
components have unclear placement, mobile behavior is hand-waved, read-model
needs are unknown, or the Questioner identifies a missing workflow proof. Stop
when the selected page spec is implementation-ready, remaining gaps are owned
implementation tasks, or a blocker/follow-up task is recorded.

Record the page decision:

```text
Page Decision
  selected_option:
  why_it_serves_vision:
  remaining_weaknesses:
  borrowed_from_rejected_options:
  rejected_options:
  visual_placement:
  read_model_gaps:
  loop_status: continue | stop | blocked
  stop_or_continue_reason:
  next_designer_request:
```

## Hard Layout Implementation Spec Gate

Before component or CSS work starts, convert selected page decisions into a
hard layout implementation spec for every changed route, surface, or core
module. This is stricter than a design direction. It must remove ambiguity that
would let an implementation become a stacked report, card dump, or
raw-debug-first page.

Required fields:

```text
Hard Layout Spec
  spec_id:
  spec_path: docs/dashboard/hard-layout-specs/<slice>.md
  route_or_surface:
  selected_design_refs:
  reviewer_decision_ref:
  desktop_wireframe:
    viewport:
    ascii_diagram:
    columns_or_regions:
    fixed_dimensions:
    first_viewport_content:
    scroll_containers:
  tablet_wireframe:
    breakpoint:
    ascii_diagram:
    collapsed_regions:
    first_viewport_content:
  mobile_wireframe:
    breakpoint:
    ascii_diagram:
    tab_order:
    first_viewport_content:
    hidden_or_deferred_regions:
  state_matrix:
    empty:
    loading:
    loaded:
    warning:
    error:
  component_inventory:
  data_density_limits:
  text_wrapping_rules:
  forbidden_primary_surfaces:
  screenshot_acceptance:
  reviewer_stop_conditions:
```

Implementation may start only when the PR points to the spec path and the
Reviewer records `continue` with a statement that this spec is specific enough
to compare against browser screenshots. A `stop` or `blocked` decision returns
the work to the design loop.

## Core Module Multi-Designer Loop

Run module-level candidates for these surfaces before implementation:

| Module | Minimum options |
| --- | --- |
| Vision overview | 2 options for goal collection and distance-to-vision placement |
| Team workspace | 3 options for Feishu-like team/workspace/inbox composition |
| AgentMember workbench | 3 options for activity, inbox/outbox, runtime, and send-message placement |
| Goal document | 2 options for document sections and graph/Kanban placement |
| Task document | 2 options for proof-order layout and PR/evidence placement |
| Graph/Kanban | 3 options for default, focus, and mobile fallback |
| Docs context | 2 options for inspector/route placement |
| Evidence/Review/Decision | 2 options for global queue and object-local proof |
| Warnings/repair | 2 options for global queue and local callouts |
| Debug | 2 options for collapsed drawer and route behavior |
| Mobile/responsive | 3 options for tab order, inspector behavior, and graph fallback |

The Reviewer chooses one option per module, may borrow pieces from rejected
options, and records any required follow-up Designer round.

## Required Workbench Page Specs

For the Agent Workbench, normally produce page specs for:

| Page | Required UX purpose |
| --- | --- |
| Vision overview | Show vision, goal collection, complete/not-complete goals, distance-to-vision, and next goals. |
| Team workspace | Show persistent AgentTeam as a collaboration space with active Vision/Goal, role groups, queues, and decision queue. |
| AgentMember workbench | Show one member as a durable teammate with status, activity, inbox/outbox, runtime health, current task, and send-message controls. |
| Goal document | Show GoalDesign, team design, goal branch, graph/Kanban, evidence/review/decision, GoalEvaluation, docs, and next-round plan. |
| Task document | Show assignment proof, acceptance, assignee/runtime, evidence, proposal/review/decision, branch/worktree/PR, and warnings. |
| Graph/Kanban view | Show synchronized relationship and execution projections for Goal and Task. |
| Docs context | Mount project docs near the active Vision/Goal/Task/Decision without duplicating truth. |
| Evidence/Review/Decision | Make acceptance auditable and incomplete decisions visible. |
| Warnings/repair queue | Show affected object, severity, why it matters, navigation, and safe repair actions. |
| Debug drawer | Preserve raw snapshot/debug access outside the primary work surface. |

## Complete Frontend Design Draft

A complete design draft should be a stable doc artifact. It is not a final
implementation, but it should remove ambiguity before coding.

Include:

- design brief and Vision restatement;
- selected layout decision and rejected alternatives;
- route map;
- page specs for all core pages;
- selected and rejected page-level variants for risky pages;
- hard layout implementation specs for desktop, tablet, and mobile;
- component inventory and object mapping;
- visual placement map;
- interaction flows and safe action contracts;
- read-model/API needs;
- responsive behavior;
- visual system and state language;
- browser acceptance checklist.

Do not start implementation until missing page specs are either completed or
explicitly waived with rationale and follow-up tasks.

Do not continue implementation when browser screenshots show the selected spec
was not followed or was too vague. Record the failed attempt in
`layout-decisions.md`, keep the code out of the integration branch, and return
to the module option loop.
