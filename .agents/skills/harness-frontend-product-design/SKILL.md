---
name: harness-frontend-product-design
description: "Design or review the Multi-Agent Harness Agent Workbench UI/UX from the product Vision, Goal collection, TaskGraph, persistent AgentTeam, AgentMember runtime, evidence, review, and decision workflow. Use before changing Workbench layout, routes, visual system, graph/Kanban views, AgentMember realtime surfaces, frontend acceptance gates, or multi-round Designer/Reviewer layout decisions."
---

# Harness Frontend Product Design

Use this skill before redesigning or implementing the Agent Workbench or any
harness frontend surface. The product surface is a workbench, not a dashboard:
operators should be able to observe, decide, message, repair, and continue
agent work. `Dashboard` remains a legacy path/command/module name only. The
goal is to prevent decorative UI work that hides whether the harness workflow
actually happened.

## How To Use This Skill

1. Define the frontend decision being made: top-level shell, core page map,
   module detail, implementation plan, or acceptance review.
2. Load only the docs needed for that decision, using the source-doc tiers
   below. Do not load every reference by default.
3. Restate the Vision, selected Goal, and final acceptance standard before
   proposing UI.
4. Map canonical harness objects to pages and workflow proof.
5. Run the required Designer -> Questioner -> Decision loop for substantial
   layouts, and the smaller option loop for risky pages or modules.
6. Record selected designs, rejected designs, borrowed ideas, remaining gaps,
   and loop stop/continue decisions in `docs/dashboard/`.
7. Turn accepted specs into small implementation tasks with clear owned paths.
8. Require browser screenshots and web-quality evidence before implementation
   acceptance.

## Required Source Docs

Load docs progressively. Start with the smallest tier that can answer the
current decision.

Always load for frontend product design:

- `docs/prd.md`
- `docs/concept-model.md`
- `docs/dashboard.md`
- `docs/dashboard/README.md`

Load when changing Workbench layout, routes, or visual system:

- `docs/dashboard/design-principles.md`
- `docs/dashboard/layout-variants.md`
- `docs/dashboard/layout-decisions.md`
- `docs/dashboard/frontend-design.md`
- `docs/dashboard/frontend-architecture.md`
- `docs/dashboard/acceptance.md`

Load when the design touches workflow semantics:

- `docs/goal-learning-loop.md`
- `docs/agent-control-plane.md`
- `docs/workflow-git-pr.md`

Load when placing or indexing docs:

- `docs/README.md`
- `docs/registry.json`

Use skill references only when needed:

- `references/product-model.md` for Vision/Goal/Task/Team object-to-page rules.
- `references/layout-principles.md` for graph, Kanban, inspector, realtime, and visual-system rules.
- `references/subagent-design-loop.md` for the required three-variant
  Designer/Questioner/Decision loop.
- `references/page-design-workflow.md` for core page discovery, page-level
  option loops, and complete frontend design draft requirements.
- `references/acceptance-gates.md` for browser, web-quality, and harness workflow acceptance.

Reference ownership:

- `product-model.md`: canonical object semantics only.
- `layout-principles.md`: layout, graph/Kanban, realtime, and visual rules.
- `subagent-design-loop.md`: subagent roles, prompts, independence,
  multi-round review, and decision templates.
- `page-design-workflow.md`: page discovery, page specs, and design drafts.
- `acceptance-gates.md`: validation commands, evidence, thresholds, and waivers.

## Artifact Placement

The canonical design record belongs in `docs/dashboard/`, not inside the skill.
Use the skill for process and guardrails; use docs for product decisions.

- `docs/dashboard/design-principles.md`: durable frontend doctrine and visual
  principles.
- `docs/dashboard/layout-variants.md`: candidate layouts, critiques, killed
  directions, and useful parts kept.
- `docs/dashboard/layout-decisions.md`: selected main direction, borrowed
  ideas, remaining gaps, reviewer loop status, and stop/continue reasons.
- `docs/dashboard/frontend-design.md`: selected shell, route map, core page
  cards, object mapping, visual placement, safe actions, read-model needs,
  responsive behavior, implementation sequence, and acceptance pointers.
- `docs/dashboard/acceptance.md`: browser screenshots, console, accessibility,
  performance, web-quality, and harness workflow acceptance evidence plan.
- `docs/dashboard/README.md`: index of the current Workbench design contract.

When a new frontend design document supersedes older Workbench docs, update
`docs/dashboard/README.md`, `docs/README.md`, and `docs/registry.json`, then
delete or mark the old docs as deprecated. Do not leave multiple canonical
layout specs that conflict.

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

The preferred mental model is a multi-agent collaboration workbench, closer to
a Feishu/Slack team workspace than a metrics dashboard. Use that familiarity to
make AgentMembers feel like durable teammates, but keep every message, status,
and action tied to canonical harness objects.

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

## Design Loop

For substantial frontend design or redesign, load
`references/subagent-design-loop.md` and run the required loop:

```text
Designer proposes three distinct directions
  -> Questioner challenges them independently
  -> Decision Agent selects, synthesizes, or requests another round
  -> repeat until acceptance, no useful new signal, or an explicit blocker
```

The Decision Agent / Reviewer may choose one design as the main direction while
still requiring changes. Every decision must record the main selected layout,
its remaining weaknesses, useful ideas borrowed from rejected layouts, killed
alternatives, whether another Designer round is required, and why the loop
continues or stops.

For page or module risk, load `references/page-design-workflow.md` and run a
smaller 2-3 option loop before implementation. Rejected layouts and rejected
page options are first-class evidence; keep them in `docs/dashboard/`.

## Likely Failure Modes

Watch for these before approving design or implementation:

- Vision collapsed into one Goal instead of a collection and learning loop.
- Goal reduced to a task list without GoalDesign, branch, team, evidence,
  decision, evaluation, and distance-to-vision.
- AgentTeam treated as disposable or graph-first instead of a persistent
  collaboration workspace.
- AgentMember realtime state faked by decorative activity instead of messages,
  sessions, runtime health, inbox/outbox, and send-message controls.
- Graph used as the whole product, or Kanban omitted where operators need
  execution state.
- Docs copied into the UI instead of mounted as source-linked context.
- Raw JSON, snapshot paste, or debug state promoted to the primary viewport.
- Visual polish hiding missing assignment, evidence, review, decision, or
  browser acceptance.

## Before Implementation

Before writing component code, the design record must include:

- core page discovery from Vision, PRD, object model, and failure modes;
- docs cleanup decision: retained docs, deleted/superseded docs, and updated
  registry/index links;
- route map and page hierarchy;
- selected top-level layout, rejected alternatives, and borrowed ideas;
- page-level specs for Vision, Team, AgentMember, Goal, Task, Graph/Kanban,
  Docs, Evidence/Review/Decision, Warnings, and Debug;
- visual placement for primary surface, secondary surface, inspector/drawer,
  and mobile position;
- read-model/API needs and implementation sequence;
- acceptance plan using `references/acceptance-gates.md`.

## Implementation Gate

For implementation review, load `references/acceptance-gates.md`. Do not
approve a frontend implementation until browser evidence covers desktop,
tablet, and mobile; console and overflow checks are clean; accessibility,
performance, and best-practices results are recorded; and workflow acceptance
proves Vision, Goal, TaskGraph, AgentTeam, AgentMember, evidence, review,
decision, and GoalEvaluation are visible.
