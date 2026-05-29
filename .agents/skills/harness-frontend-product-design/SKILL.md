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
5. Run the stage-gated frontend loop below. Do not skip directly from design
   direction to component or CSS work.
6. Record selected designs, rejected designs, borrowed ideas, remaining gaps,
   and loop stop/continue decisions in `docs/dashboard/`.
7. Produce page-local layout contracts before component or CSS work. Each
   changed `docs/dashboard/pages/<page>.md` must include detailed
   desktop/tablet/mobile ASCII box diagrams, region dimensions, first-viewport
   content, scroll boundaries, empty states, and browser screenshot acceptance.
   Text-only wireframe prose is not enough.
8. Record a frontend architecture and technical-stack decision before
   implementation. The current component tree is not evidence for the next
   architecture.
9. Turn accepted specs into small implementation tasks with clear owned paths.
10. Keep a Questioner/Critic active during implementation. If screenshots show
   a stacked report, raw-debug-first view, overflow, card dump, or weak workflow
   proof, stop coding, record the rejected implementation, and return to design.
11. After implementation, run browser-based PM and User acceptance agents using
   the prompts in `references/acceptance-gates.md`. They must inspect the
   working UI and screenshots, not only static docs or code.
12. Require browser screenshots, PM/User acceptance output, and web-quality
   evidence before implementation acceptance.

## Stage-Gated Frontend Workflow

Frontend acceptance starts during design intake, not after code is written.
Every stage has explicit agents, artifacts, and hard stop rules:

```text
Vision/docs intake
  -> core page discovery loop
  -> top-level layout candidate loop
  -> core module option loop
  -> architecture and technical-stack decision
  -> page-local layout contract gate
  -> implementation screenshot critic loop
  -> PM/User browser acceptance loop
  -> PR and learning gate
```

| Stage | Required agents | Required artifact | Cannot continue when |
| --- | --- | --- | --- |
| Vision/docs intake | Lead + Questioner | Vision, selected Goal, workflow proof, docs cleanup decision | The workbench purpose is unclear or the work starts from the existing component tree. |
| Core page discovery | Designer + Questioner + Reviewer | Core page cards and object ownership | AgentTeam, AgentMember, Goal, Task, Docs, Graph/Kanban, warnings, or debug boundaries are missing. |
| Top-level layout | Multiple Designer passes + Questioner + Reviewer | 3 layout candidates, killed options, borrowed ideas | Only one layout exists or variants do not expose real tradeoffs. |
| Module options | Designer + Questioner + Reviewer | 2-3 options for each core module | Cards, details, message flow, docs, warnings, or mobile placement are underspecified. |
| Architecture/stack | Lead + Architect + Critic | Stack decision and module boundary decision | The decision inherits the old dashboard structure or adds a UI kit without proof. |
| Page layout contract | Reviewer + Implementation Questioner | Detailed desktop/tablet/mobile ASCII diagrams inside each changed page spec | Any changed route lacks exact first-viewport placement, dimensions, and scroll ownership in its page document. |
| Implementation | Implementer + Implementation Questioner | Screenshot comparison per slice | Browser screenshots look like a dashboard, card dump, raw tool, or long report. |
| PM/User acceptance | PM agent + User agent | Browser findings and fixes | Either agent has unresolved P0/P1 findings. |
| PR/learning | Lead + Critic | Selected/rejected designs, screenshots, failures, waivers | Failed attempts are not recorded or screenshots are treated as passive attachments. |

Screenshots are not passive evidence. The screenshot itself is the acceptance
object. A frontend change with visible data, clean console output, and no
horizontal overflow still fails when the first viewport does not read as the
selected Agent Workbench layout.

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
- `references/subagent-design-loop.md` for the required multi-candidate
  Designer/Questioner/Reviewer loop.
- `references/page-design-workflow.md` for core page discovery, page-level
  option loops, and complete frontend design draft requirements.
- `references/architecture-stack-decision.md` for frontend architecture,
  technology-stack, dependency, graph/canvas, styling, and old-code quarantine
  decisions before implementation.
- `references/implementation-loop.md` for implementation-slice planning,
  screenshot Critic checks, rejected-implementation recording, and PM/User
  handoff.
- `references/acceptance-gates.md` for browser, web-quality, and harness workflow acceptance.

Reference ownership:

- `product-model.md`: canonical object semantics only.
- `layout-principles.md`: layout, graph/Kanban, realtime, and visual rules.
- `subagent-design-loop.md`: subagent roles, prompts, independence,
  multi-round review, and decision templates.
- `page-design-workflow.md`: page discovery, page specs, and design drafts.
- `architecture-stack-decision.md`: stack choice, dependency policy, graph
  library policy, no-shadcn rule, code quarantine, and module boundaries.
- `implementation-loop.md`: implementation slices, browser screenshot
  comparison, rejected implementation, PM/User handoff, and PR evidence.
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
- `docs/dashboard/frontend-design.md`: Workbench design index, page-spec map,
  current status, and implementation-readiness summary. It is
  not the single large design source of truth.
- `docs/dashboard/pages/<page>.md`: page-level product and UX specs. Use one
  file per core page/workspace: Vision overview, Team workspace, AgentMember
  workbench, Goal document, Task document, Graph/Kanban, Docs context,
  Evidence/Review/Decision, Warnings/repair, and Debug. Each page file owns its
  own detailed layout contract and ASCII diagrams.
- `docs/dashboard/frontend-architecture.md`: current frontend architecture and
  technical-stack decision. Update it before implementation when the work
  changes framework, state model, component architecture, graph/canvas
  approach, styling strategy, dependency policy, or old-code disposition.
- `docs/dashboard/hard-layout-specs/`: historical/deprecated layout attempts
  only. Do not place current page layouts there.
- `docs/dashboard/rejected-implementations/<attempt>.md`: failed browser-visible
  implementations, first-impression screenshot review, violated gates, old-code
  contamination, and restart point.
- `docs/dashboard/acceptance.md`: browser screenshots, console, accessibility,
  performance, web-quality, and harness workflow acceptance evidence plan.
- `docs/dashboard/README.md`: index of the current Workbench design contract.

When a new frontend design document supersedes older Workbench docs, update
`docs/dashboard/README.md`, `docs/README.md`, and `docs/registry.json`, then
delete or mark the old docs as deprecated. Do not leave multiple canonical
layout contracts that conflict.

## ASCII Layout Diagrams

Every changed page spec under `docs/dashboard/pages/` must include detailed
ASCII box diagrams for desktop, tablet, and mobile. These diagrams are the
implementation contract for region placement, fixed dimensions, collapsed
regions, first-viewport hierarchy, and scroll ownership. A written list of
columns or a prose description does not satisfy the layout gate.

Use plain ASCII characters so the diagram remains stable in Markdown, PR
comments, terminals, and provider transcripts:

```text
+------+------------------------+--------------+
| rail | primary workspace      | inspector    |
+------+------------------------+--------------+
```

Reviewer approval is `blocked` when a changed route, surface, or core module
lacks detailed ASCII diagrams in its own page document for any accepted
viewport.

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
multiple Designers propose distinct directions
  -> Questioner challenges them independently
  -> Reviewer selects, synthesizes, kills, or requests another round
  -> repeat until acceptance, no useful new signal, or an explicit blocker
```

Use distinct Designer subagents when available. If only one agent is available,
run separate Designer passes with different constraints and record them as
separate candidates. The Reviewer may choose one design as the main direction
while still requiring changes. Every decision must record the selected layout,
remaining weaknesses, useful ideas borrowed from rejected layouts, killed
alternatives, whether another Designer round is required, and why the loop
continues or stops.

For page or module risk, load `references/page-design-workflow.md` and run a
smaller 2-3 option loop before implementation. Core modules must use this
loop, especially Vision overview, Team workspace, AgentMember workbench, Goal
document, Task document, Graph/Kanban, Docs context, Evidence/Review/Decision,
Warnings/repair, Debug, and mobile/responsive placement. Rejected layouts,
rejected page options, and rejected implementation attempts are first-class
evidence; keep them in `docs/dashboard/`.

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
- page-level specs under `docs/dashboard/pages/` for Vision, Team,
  AgentMember, Goal, Task, Graph/Kanban, Docs, Evidence/Review/Decision,
  Warnings, and Debug;
- a page-local layout contract inside every changed page spec, with concrete
  desktop/tablet/mobile ASCII box diagrams, region sizing, scroll behavior,
  first-viewport content, empty states, overflow constraints, and screenshot
  acceptance;
- visual placement for primary surface, secondary surface, inspector/drawer,
  and mobile position;
- Reviewer selection records for each core module, including killed options and
  borrowed ideas;
- Questioner/Critic concerns resolved or converted into follow-up tasks;
- frontend architecture and technology-stack decision using
  `references/architecture-stack-decision.md`;
- old code disposition: deleted, quarantined, migrated, or explicitly retained
  with rationale;
- read-model/API needs and implementation sequence;
- acceptance plan using `references/acceptance-gates.md`.

Implementation must not begin when the design only states a direction such as
"Feishu-like" or "Team workspace first." It must say exactly what appears in
the first viewport, where each core module lives, how it responds on tablet and
mobile, and how browser screenshots will prove the result.

## Implementation Gate

For implementation review, load `references/acceptance-gates.md`. Do not
approve a frontend implementation until browser evidence covers desktop,
tablet, and mobile; console and overflow checks are clean; accessibility,
performance, and best-practices results are recorded; and workflow acceptance
proves Vision, Goal, TaskGraph, AgentTeam, AgentMember, evidence, review,
decision, and GoalEvaluation are visible. Implementation review also requires
two read-only browser acceptance agents: a PM agent that judges end-to-end
product logic, and a User agent that judges operator usability while using the
actual interface. Their prompts, screenshot refs, findings, fixes, and any
waivers must be recorded with the PR or harness evidence.

If the first browser screenshots show that the implementation reads as a
stacked report, metrics dashboard, card dump, raw snapshot tool, or visually
confusing surface, stop implementation. Record the failed attempt in
`layout-decisions.md`, keep the code out of the goal branch, and return to the
Designer -> Questioner -> Reviewer loop before trying again.
