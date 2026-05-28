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
- `docs/dashboard/ui-ux-layout.md`
- `docs/dashboard/frontend-architecture.md`
- `docs/dashboard/acceptance.md`
- `docs/goal-learning-loop.md`
- `docs/agent-control-plane.md`
- `docs/workflow-git-pr.md`

Use references only when needed:

- `references/product-model.md` for Vision/Goal/Task/Team object-to-page rules.
- `references/layout-principles.md` for graph, Kanban, inspector, realtime, and visual-system rules.
- `references/subagent-design-loop.md` for the required Designer/Questioner loop.
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

Do not start from component aesthetics or a card grid. Start from the workflow
proof the user must be able to reconstruct.

## Non-Negotiable Design Rules

- Vision is not a single goal. It is a long-lived target with a collection of
  goals and a distance-to-vision loop.
- Goal is not a task list. It owns GoalDesign, team design, branch/integration
  policy, dynamic TaskGraph, evidence, decisions, and evaluation.
- TaskGraph is dynamic. It can be split, blocked, reprioritized, or extended
  through graph-change proposals, messages, decisions, and follow-up tasks.
- Goal and Task views need both graph and Kanban-style execution views. Graph
  explains relationships; Kanban explains operational status.
- AgentTeam should not default to graph. Treat it as a persistent operations
  console: roles, members, queues, runtime status, current task, and continuity.
- AgentMember must feel live: activity timeline, message flow, provider sessions,
  runtime health, and direct send-message affordance.
- Debug JSON, snapshot paste, and raw object lists are secondary tools. Keep them
  in a debug drawer or route, not the primary viewport.
- Visual impact must come from live topology, status, motion, density, and
  meaningful state transitions, not decorative gradients or unrelated imagery.

## Required Subagent Design Loop

For substantial frontend design or redesign, run a two-subagent loop before
implementation. Both subagents must first read and restate the project Vision,
final acceptance standard, and how the selected Goal moves toward that Vision.
If either subagent cannot explain that context, do not use its design output for
decisions.

```text
Designer subagent
  -> restates Vision and selected Goal context
  -> proposes page hierarchy, layouts, visual system, and interaction model

Questioner subagent
  -> restates Vision and selected Goal context
  -> challenges product assumptions, workflow proof, layout tradeoffs, and
     acceptance gaps

Lead
  -> acts as gate, checks docs and acceptance, records design decisions, and
     turns accepted changes into tasks
```

If subagent tools are unavailable, record a blocker or explicit waiver in the
harness state. Do not silently replace the loop with one person's hidden
reasoning for non-trivial frontend redesign.

## Output Artifacts

Before code changes, produce or update the appropriate docs:

- page map and route hierarchy;
- object-to-page mapping;
- layout spec for Vision, Goal, TaskGraph, AgentTeam, AgentMember, Evidence,
  Review, Decision, Warnings, and Debug;
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
