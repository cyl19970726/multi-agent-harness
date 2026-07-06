# Agent Workbench Frontend Design Index

This file is now an index for the Agent Workbench frontend design contract. It
is intentionally not a giant all-in-one design document. Product semantics,
page specs, page-local layout contracts, architecture, acceptance, and rejected
implementations live in separate files so future implementation cannot drift
from vague prose.

Product purpose stays in [../dashboard.md](../dashboard.md). Durable design
principles stay in [design-principles.md](design-principles.md). Layout
candidates, critique, and the selected/killed/deprecated decision ledger stay in
[layout-history.md](layout-history.md). Page-level specs live under
[pages/](pages/), and each page file owns its current `## Layout Contract`
with detailed desktop/tablet/mobile ASCII diagrams. Architecture and stack
decisions stay in [frontend-architecture.md](frontend-architecture.md) and ADRs
[0016](../decisions/0016-tailwind-shadcn-adoption.md) (stack) +
[0019](../decisions/0019-vision-goal-task-workbench-redesign.md) (light Notion
theme + Vision→Goal→Task redesign). Acceptance gates stay in
[acceptance.md](acceptance.md).

## Current Design Status

```text
status:
  top_level_direction: shipped
  current_implementation: merged (PR #7)
  stack: React 18 + TypeScript + Vite + Tailwind v4 + shadcn/Radix + lucide + Geist
  theme: light, Notion-like document surface (ADR 0019, supersedes 0016 dark theme)
  vision_goal_task: unified Work board + Notion document detail (work-board-design.md)
  goal_detail_contract: phase-first Goal Workbench (pages/goal.md)
  frontend_design_source: page specs + work-board-design.md + ADR 0019
  implementation_allowed: yes
```

The Agent Workbench frontend was rebuilt and merged in PR #7 on the Tailwind v4
+ shadcn/ui (Radix) + lucide-react + Geist stack. The earlier hand-rolled-CSS
shell (PR #6) was rejected; that outcome and the full layout decision ledger are
recorded in [layout-history.md](layout-history.md). Further changes must:

- start from the active page specs in [pages/](pages/) and their
  `## Layout Contract` sections;
- follow the architecture and stack decision in
  [frontend-architecture.md](frontend-architecture.md) and ADRs
  [0016](../decisions/0016-tailwind-shadcn-adoption.md) +
  [0019](../decisions/0019-vision-goal-task-workbench-redesign.md);
- for Vision/Task/Work-board surfaces, follow
  [work-board-design.md](work-board-design.md) (Notion document layout, unified
  Work board); for Goal detail, follow the active
  [Goal Workbench page contract](pages/goal.md);
- keep desktop/tablet/mobile ASCII diagrams in each changed page spec current;
- pass screenshot-first acceptance in [acceptance.md](acceptance.md).

## Workbench Product Flow

The UI must make this workflow inspectable without raw JSON or hidden chat
context:

```text
Vision -> Goal collection -> GoalDesign -> AgentTeam -> TaskGraph
  -> Message assignment -> AgentMember runtime -> Evidence -> Proposal
  -> Review -> Decision -> GoalEvaluation -> distance-to-vision
  -> next Goal / follow-up Task / GoalCase
```

## Reading Order

1. [../dashboard.md](../dashboard.md): product-level purpose and information
   architecture.
2. [design-principles.md](design-principles.md): durable UI doctrine and
   failure modes.
3. [layout-history.md](layout-history.md): candidate layout directions,
   critique, and the selected/rejected/borrowed decision ledger.
4. [pages/README.md](pages/README.md): page-spec index and template.
5. Page specs under [pages/](pages/): product/UX contract plus page-local
   layout contract per core page.
6. [frontend-architecture.md](frontend-architecture.md): technical stack,
   module boundaries, old-code disposition.
7. [acceptance.md](acceptance.md): screenshot-first browser and PM/User gates.

## Page Specs

The **Vision, Task, Work-board (graph/Kanban), and Docs** surfaces are owned by
[work-board-design.md](work-board-design.md) and ADR
[0019](../decisions/0019-vision-goal-task-workbench-redesign.md): a light,
Notion-like document layout, a unified `[Goals | Tasks]` Work board with a goal
filter and derived ready/waiting chips, Task slide-over, and live docs rendering
for Vision `source_refs`. The **Goal detail** surface now has an active
page-local contract because Goal Workbench v1 makes it the phase-first execution
and proof surface.

| Page spec | Owns |
| --- | --- |
| [work-board-design.md](work-board-design.md) | Vision/Goal/Task structures + examples, unified Work board, Goal/Task/Vision detail document layout, derived task-graph. |
| [Goal Workbench](pages/goal.md) | Phase-first Goal detail, proof chain, task grouping, stage demotion, and screenshot gate. |
| [Team workspace](pages/team-workspace.md) | Persistent AgentTeam collaboration space, role groups, queues, activity, decision pressure. |
| [AgentMember workbench](pages/agent-member-workbench.md) | One durable member as a teammate: current work, inbox/outbox, activity, runtime, prompt/skills, actions. |
| [Evidence/Review/Decision](pages/evidence-review-decision.md) | Acceptance proof chain and decision queues. |
| [Warnings/repair](pages/warnings-repair.md) | Workflow risks, affected objects, navigation, repair metadata. |
| [Debug](pages/debug.md) | Raw snapshot, import/export, and low-level object views outside the primary work surface. |

## Source Of Truth Boundary

| File group | Owns | Refuses |
| --- | --- | --- |
| `pages/*.md` | Page purpose, user question, canonical objects, workflow proof, IA, actions, detailed desktop/tablet/mobile ASCII diagrams, dimensions, scroll ownership, failure modes, screenshot matrix. | Component internals. |
| `layout-history.md` | Candidate critique, scoring, and why a design was selected, killed, or borrowed, including rejected-implementation outcomes. | Implementation code or screenshot pass/fail logs. |
| `frontend-architecture.md` | Stack, routing, state, component boundaries, graph/canvas strategy, old-code disposition. | Product purpose or page-level UX. |
| `acceptance.md` | Browser screenshot rubric, PM/User prompts, web-quality gates, waiver policy. | Layout candidates or component architecture. |

If a layout change alters page meaning, update the relevant page spec and
[layout-history.md](layout-history.md) first. If it only changes dimensions,
breakpoints, or scroll ownership, update the `## Layout Contract` in that same
page spec.

## Non-Negotiable Implementation Rule

No frontend implementation may start from the old dashboard component tree or
from the rejected PR #6 Workbench shell. Implementation builds on the shipped
rebuild (PR #7) and starts from page specs with accepted page-local layout
contracts, the architecture decision in
[frontend-architecture.md](frontend-architecture.md), and ADR
[0016](../decisions/0016-tailwind-shadcn-adoption.md).
