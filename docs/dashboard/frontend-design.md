# Agent Workbench Frontend Design Index

This file is now an index for the Agent Workbench frontend design contract. It
is intentionally not a giant all-in-one design document. Product semantics,
page specs, page-local layout contracts, architecture, acceptance, and rejected
implementations live in separate files so future implementation cannot drift
from vague prose.

Product purpose stays in [../dashboard.md](../dashboard.md). Durable design
principles stay in [design-principles.md](design-principles.md). Layout
candidates and decisions stay in [layout-variants.md](layout-variants.md) and
[layout-decisions.md](layout-decisions.md). Page-level specs live under
[pages/](pages/), and each page file owns its current `## Layout Contract`
with detailed desktop/tablet/mobile ASCII diagrams. Historical hard-layout
attempts stay under [hard-layout-specs/](hard-layout-specs/) only for failure
analysis. Failed browser-visible attempts live under
[rejected-implementations/](rejected-implementations/). Architecture and stack
decisions stay in [frontend-architecture.md](frontend-architecture.md).
Acceptance gates stay in [acceptance.md](acceptance.md).

## Current Design Status

```text
status:
  top_level_direction: restart required
  current_pr_6_implementation: rejected
  frontend_design_source: page specs with page-local layout contracts
  implementation_allowed: no
```

Implementation cannot resume until:

- the active page specs in [pages/](pages/) are reviewed;
- the architecture and stack decision in
  [frontend-architecture.md](frontend-architecture.md) is updated;
- the old dashboard/failed Workbench implementation is deleted or quarantined;
- every changed page spec has a detailed `## Layout Contract` with
  desktop/tablet/mobile ASCII diagrams;
- screenshot-first acceptance in [acceptance.md](acceptance.md) is the gate.

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
3. [layout-variants.md](layout-variants.md): candidate layout directions and
   critique.
4. [layout-decisions.md](layout-decisions.md): selected, rejected, and borrowed
   decisions.
5. [pages/README.md](pages/README.md): page-spec index and template.
6. Page specs under [pages/](pages/): product/UX contract plus page-local
   layout contract per core page.
7. [frontend-architecture.md](frontend-architecture.md): technical stack,
   module boundaries, old-code disposition.
8. [hard-layout-specs/README.md](hard-layout-specs/README.md): historical
   layout-attempt index only.
9. [acceptance.md](acceptance.md): screenshot-first browser and PM/User gates.

## Page Specs

| Page spec | Owns |
| --- | --- |
| [Vision overview](pages/vision-overview.md) | Vision context, goal collection, completion state, distance-to-vision, next goals. |
| [Team workspace](pages/team-workspace.md) | Persistent AgentTeam collaboration space, role groups, queues, activity, decision pressure. |
| [AgentMember workbench](pages/agent-member-workbench.md) | One durable member as a teammate: current work, inbox/outbox, activity, runtime, prompt/skills, actions. |
| [Goal document](pages/goal-document.md) | GoalDesign, team design, branch policy, graph/Kanban, evidence, decision, evaluation, next round. |
| [Task document](pages/task-document.md) | Assignment proof, report, evidence, proposal, review, decision, Git refs, warnings. |
| [Graph/Kanban](pages/graph-kanban.md) | Relationship graph plus execution lanes for Goals and Tasks. |
| [Docs context](pages/docs-context.md) | Mounted project docs linked to active Vision, Goal, Task, Member, Evidence, or Decision. |
| [Evidence/Review/Decision](pages/evidence-review-decision.md) | Acceptance proof chain and decision queues. |
| [Warnings/repair](pages/warnings-repair.md) | Workflow risks, affected objects, navigation, repair metadata. |
| [Debug](pages/debug.md) | Raw snapshot, import/export, and low-level object views outside the primary work surface. |

## Source Of Truth Boundary

| File group | Owns | Refuses |
| --- | --- | --- |
| `pages/*.md` | Page purpose, user question, canonical objects, workflow proof, IA, actions, detailed desktop/tablet/mobile ASCII diagrams, dimensions, scroll ownership, failure modes, screenshot matrix. | Component internals. |
| `hard-layout-specs/*.md` | Historical hard-layout attempts retained for failure analysis only. | Current implementation gates or page layout contracts. |
| `layout-decisions.md` | Why a design was selected, killed, or borrowed. | Implementation code or screenshot pass/fail logs. |
| `frontend-architecture.md` | Stack, routing, state, component boundaries, graph/canvas strategy, old-code disposition. | Product purpose or page-level UX. |
| `acceptance.md` | Browser screenshot rubric, PM/User prompts, web-quality gates, waiver policy. | Layout candidates or component architecture. |

If a layout change alters page meaning, update the relevant page spec and
`layout-decisions.md` first. If it only changes dimensions, breakpoints, or
scroll ownership, update the `## Layout Contract` in that same page spec.

## Non-Negotiable Implementation Rule

No frontend implementation may start from the old dashboard component tree or
from the rejected PR #6 Workbench shell. The next implementation starts from
page specs with accepted page-local layout contracts and the architecture
decision.
