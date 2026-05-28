---
name: harness-frontend-product-design
description: "Design or review Multi-Agent Harness frontend UI/UX from the product Vision, Goal collection, TaskGraph, persistent AgentTeam, AgentMember runtime, evidence, review, and decision workflow. Use before changing Dashboard layout, routes, visual system, graph/Kanban views, AgentMember realtime surfaces, or frontend acceptance gates."
---

# Harness Frontend Product Design

Use this skill before redesigning or implementing the Agent Dashboard or any
harness frontend surface. The goal is to prevent decorative UI work that hides
whether the harness workflow actually happened.

## Required Source Docs

Read only the docs needed for the task, starting with:

- `docs/prd.md`
- `docs/concept-model.md`
- `docs/dashboard.md`
- `docs/dashboard/README.md`
- `docs/dashboard/design-principles.md`
- `docs/dashboard/layout-variants.md`
- `docs/dashboard/layout-decisions.md`
- `docs/dashboard/ui-ux-layout.md`
- `docs/dashboard/frontend-architecture.md`
- `docs/dashboard/acceptance.md`
- `docs/goal-learning-loop.md`
- `docs/agent-control-plane.md`
- `docs/workflow-git-pr.md`

Use references only when needed:

- `references/product-model.md` for Vision/Goal/Task/Team object-to-page rules.
- `references/layout-principles.md` for graph, Kanban, inspector, realtime, and visual-system rules.
- `references/subagent-design-loop.md` for the required three-variant
  Designer/Questioner/Decision loop.
- `references/acceptance-gates.md` for browser, web-quality, and harness workflow acceptance.

## Frontend Doctrine

Derive the UI from the product object model:

```text
Vision
  -> Goal collection
  -> selected Goal
  -> GoalDesign
  -> persistent AgentTeam
  -> dynamic TaskGraph
  -> Message assignment
  -> AgentMember runtime
  -> Evidence / Proposal / Review / Decision
  -> GoalEvaluation
  -> distance-to-vision
  -> NextRoundPlan / next Goal
```

Design from the top down:

1. Vision page: goal graph and goal progress across completed, active, blocked,
   proposed, and archived/rejected goals.
2. Goal workbench: selected goal, GoalDesign, goal branch, designed team,
   dynamic TaskGraph, evidence/review/decision, and goal evaluation.
3. Task surface: graph for dependencies and Kanban for execution state.
4. AgentTeam surface: persistent organization, roster, roles, queues, and health.
5. AgentMember surface: realtime activity stream, inbox/outbox, runtime health,
   prompt/skills/permissions, and send-message controls.
6. Docs surface: mounted project docs connected to the active Vision, Goal,
   Task, AgentTeam, evidence, and decisions.

Do not start from component aesthetics or a card grid. Start from the workflow
proof the user must be able to reconstruct.

The preferred mental model is a multi-agent collaboration control plane, closer
to a Feishu/Slack team workspace than a metrics dashboard. Use that familiarity
to make AgentMembers feel like durable teammates, but keep every message,
status, and action tied to canonical harness objects.

## Non-Negotiable Design Rules

- Vision is not a single goal. It is a long-lived target with a collection of
  goals and a distance-to-vision loop.
- Goal is not a task list. It owns GoalDesign, team design, branch/integration
  policy, dynamic TaskGraph, evidence, decisions, and evaluation.
- Goal and Task details should feel like collaborative work documents, not only
  cards: body, status, linked objects, messages, evidence, review, decision,
  branch/worktree refs, and history should be visible in one durable surface.
- TaskGraph is dynamic. It can be split, blocked, reprioritized, or extended
  through graph-change proposals, messages, decisions, and follow-up tasks.
- Goal and Task views need both graph and Kanban-style execution views. Graph
  explains relationships; Kanban explains operational status.
- AgentTeam should not default to graph. Treat it as a persistent operations
  console: roles, members, queues, runtime status, current task, and continuity.
- AgentMember must feel live: activity timeline, message flow, provider sessions,
  runtime health, and direct send-message affordance.
- Project docs should be accessible from the Dashboard as first-class context
  for the active Vision, Goal, Task, Team, or Decision. Do not force operators
  to leave the control plane to understand the relevant docs.
- Debug JSON, snapshot paste, and raw object lists are secondary tools. Keep them
  in a debug drawer or route, not the primary viewport.
- Visual impact must come from live topology, status, motion, density, and
  meaningful state transitions, not decorative gradients or unrelated imagery.

## Required Variant Design Loop

For substantial frontend design or redesign, run a variant-first loop before
implementation. Both design subagents must first read and restate the project
Vision, final acceptance standard, and how the selected Goal moves toward that
Vision. If either subagent cannot explain that context, do not use its output
for decisions.

```text
Designer subagent
  -> restates Vision and selected Goal context
  -> proposes exactly three layout variants with tradeoffs

Required variants:
  1. Team workspace first, similar to Feishu/Slack collaboration space
  2. Goal/Task document workspace first
  3. Control plane + graph hybrid

Questioner subagent
  -> restates Vision and selected Goal context
  -> objectively challenges each variant against Vision, PRD, workflow proof,
     layout tradeoffs, implementation risk, and acceptance gaps

Decision Agent / Lead
  -> scores the variants, chooses one or synthesizes a hybrid, records the
     decision and rejected alternatives, then turns accepted design into tasks
```

If subagent tools are unavailable, record a blocker or explicit waiver in the
harness state. Do not silently replace the loop with one person's hidden
reasoning for non-trivial frontend redesign.

The Questioner must be independent. It does not serve the Designer, does not
optimize for visual novelty, and must judge each variant only against Vision,
PRD, workflow proof, acceptance, implementation feasibility, and operator
efficiency.

The Decision Agent may be the Lead Agent, but it must not simply endorse the
Designer. It should use an explicit rubric:

```text
workflow proof: 25%
Team/Member collaboration model: 20%
Goal/Task document model: 15%
graph/Kanban balance: 15%
realtime control and observability: 10%
implementation complexity: 10%
mobile/accessibility quality: 5%
```

## Refinement Loop

After a top-level layout is selected, run smaller option loops for the modules
that still carry product risk. Do not let the selected layout freeze every
detail too early.

Use this sequence:

```text
Selected layout
  -> identify risky / high-impact modules
  -> Designer proposes 2-3 module variants
  -> Questioner challenges each variant
  -> Decision Agent / Lead selects or synthesizes
  -> record selected and rejected variants
  -> update layout docs before implementation
```

Use module-level variants when designing:

- Team list and Team detail workspace;
- AgentMember workbench, status header, inbox/outbox, activity stream, and
  prompt/skills/permissions panel;
- Goal document surface, Goal graph/Kanban switch, GoalEvaluation, and
  distance-to-vision area;
- Task document surface, Task graph/Kanban switch, assignment proof, and
  branch/worktree/PR area;
- Dashboard-mounted docs navigation and contextual docs panel;
- Evidence, Review, Decision, Warnings, and Debug drawer;
- desktop, tablet, and mobile placement.

For every option loop, record:

```text
selected_variant:
  why_it_serves_vision:
  tradeoffs:
  implementation_notes:
rejected_variants:
  - name:
    killed_because:
    useful_parts_kept:
visual_placement:
  primary_surface:
  secondary_surface:
  inspector_or_drawer:
  mobile_position:
acceptance_evidence_needed:
```

Rejected layouts are first-class design evidence. Do not delete them from the
design record; future agents need to know which ideas were killed and why.

## Output Artifacts

Before code changes, produce or update the appropriate docs:

- page map and route hierarchy;
- object-to-page mapping;
- three candidate layout variants with tradeoffs;
- Questioner critique for each variant;
- selected variant or hybrid with Decision Agent / Lead rationale;
- rejected layout record with killed-because rationale and any useful parts
  kept for the selected design;
- second-pass variants for important modules when the selected layout still has
  unresolved card/detail/inspector/docs placement choices;
- visual placement map for UI elements: primary surface, secondary surface,
  inspector/drawer, and mobile position;
- layout spec for Vision, Goal, TaskGraph, AgentTeam, AgentMember, Evidence,
  Review, Decision, Warnings, and Debug;
- Goal/Task document-workspace behavior;
- Dashboard-mounted docs behavior;
- graph/Kanban behavior and collapse rules;
- visual system: theme, state colors, typography, density, motion, and realtime
  status treatment;
- acceptance plan with browser screenshots and web-quality checks.

Detailed design belongs under `docs/`, usually `docs/dashboard/`. This skill
owns the process and guardrails, not the canonical page spec.

## Implementation Gate

Do not approve a frontend implementation until it proves:

- Vision/Goal/Task hierarchy is visible.
- Completed and not-complete goals are distinct.
- Goal shows designed AgentTeam and dynamic TaskGraph.
- Task and Goal provide graph plus Kanban execution views where useful.
- AgentTeam appears persistent, not disposable.
- AgentMember realtime stream and send-message interaction are visible.
- Browser screenshots cover desktop, tablet, and mobile.
- Console is clean of React key warnings and runtime errors.
- Accessibility, performance, Core Web Vitals, and best-practices checks pass or
  have documented exceptions and follow-up tasks.
