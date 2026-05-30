# Architecture And Stack Decision

Use this reference before implementing or rebuilding any Agent Workbench
frontend surface. The decision must be recorded in
`docs/dashboard/frontend-architecture.md` before component or CSS work starts.

## Decision Card

```text
Architecture / Stack Decision
  vision_link:
  frontend_goal:
  selected_pages:
  current_old_code_disposition:
  framework_choice:
  state_model:
  routing_model:
  styling_strategy:
  component_primitives:
  graph_canvas_strategy:
  dependency_policy:
  ui_primitive_strategy:
  read_model_boundary:
  accessibility_and_keyboard_strategy:
  browser_acceptance_implications:
  rejected_options:
  reviewer_decision: continue | stop | blocked
```

## Hard Rules

- Do not select a stack by inheriting the old dashboard component tree.
- The shipped stack is Tailwind CSS v4 + shadcn/ui primitives over Radix +
  lucide-react + Geist, with dependencies in the root `package.json` (see ADR
  `docs/decisions/0016-tailwind-shadcn-adoption.md`). Compose product atoms in
  `src/components/workbench` over the shadcn/ui primitives in
  `src/components/ui`.
- Do not add a second full component framework unless the Reviewer records why
  it serves the Workbench product model better than the shadcn/Radix base.
- Delete or quarantine old dashboard components when they encode the failed
  dashboard/card-dump information architecture.
- Preserve only stable foundations that do not dictate layout: API helpers,
  snapshot types, and pure read-model selectors when they still serve the new
  page specs.
- Component architecture starts from Workbench primitives, not cards:
  `AppRail`, `TeamRail`, `Workspace`, `MemberWorkbench`, `DocumentSurface`,
  `MessageTimeline`, `LaneBoard`, `Inspector`, `Drawer`, and `CommandBar`.
- Graph is a controlled semantic view for Goal/Task relationships. It is not a
  default Team UI and not a decorative canvas.
- Team is a collaboration workspace. AgentMember is a durable teammate
  workbench. Goal and Task are collaborative documents.

## Technology Evaluation

Evaluate:

- React + TypeScript + Vite retained or replaced;
- routing strategy for page-ready surfaces;
- whether graph needs a library, custom SVG, or delayed implementation;
- whether Kanban/lane views need custom primitives or a library;
- CSS approach and design tokens;
- state and selector strategy;
- build/static artifact requirements;
- browser automation and screenshot testing implications.

For each option record:

```text
Option
  name:
  benefits:
  risks:
  old_code_contamination_risk:
  ability_to_match_page_specs:
  browser_acceptance_cost:
  decision:
```

## Old Code Disposition

Record every retained old file or component:

```text
Old Code Disposition
  path:
  decision: delete | quarantine | migrate | retain
  reason:
  allowed_surface:
  blocked_from_primary_viewport:
  replacement_plan:
```

Reviewer approval is blocked when old dashboard components still drive the
first viewport or page hierarchy.
