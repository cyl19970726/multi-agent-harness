# Agent Workbench Frontend Design Index

This file is now an index for the Agent Workbench frontend design contract. It
is intentionally not a giant all-in-one design document. Product semantics,
page specs, page-local layout contracts, architecture, acceptance, and rejected
implementations live in separate files so future implementation cannot drift
from vague prose.

Product purpose stays in [../dashboard.md](../dashboard.md). Durable design
principles stay in [design-principles.md](../company-os/frontend-information-architecture.md). Layout
candidates, critique, and the selected/killed/deprecated decision ledger stay in
[layout-history.md](../company-os/frontend-information-architecture.md). Page-level specs live under
[pages/](pages/), and each page file owns its current `## Layout Contract`
with detailed desktop/tablet/mobile ASCII diagrams. Architecture and stack
decisions stay in [frontend-architecture.md](frontend-architecture.md) and ADR
[0016](../decisions/0016-tailwind-shadcn-adoption.md) (stack). Acceptance gates stay in
[acceptance.md](../company-os/frontend-information-architecture.md).

Mission/Wave direction is canonical in
[../architecture-map.md](../architecture-map.md) and
[ADR 0026](../decisions/0026-mission-wave-architecture.md). Existing
Vision/Goal/Task work-board and Goal Workbench contracts are archived; they no
longer define the top-level product IA.

## Current Design Status

```text
status:
  top_level_direction: shipped
  current_implementation: merged (PR #7)
  stack: React 18 + TypeScript + Vite + Tailwind v4 + shadcn/Radix + lucide + Geist
  theme: light, Notion-like document surface (supersedes 0016 dark theme)
  mission_wave_direction: planned; architecture-map.md + ADR 0026
  agent_team_page: planned; pages/team-run-console.md
  legacy_goal_task_ui: shipped compatibility surface
  implementation_allowed: Mission/Wave work requires updated page contracts
```

The compatibility Agent Workbench frontend was rebuilt and merged in PR #7 on the Tailwind v4
+ shadcn/ui (Radix) + lucide-react + Geist stack. The earlier hand-rolled-CSS
shell (PR #6) was rejected; that outcome and the full layout decision ledger are
recorded in [layout-history.md](../company-os/frontend-information-architecture.md). New Mission/Wave changes must:

- start from [the architecture map](../architecture-map.md), ADR 0026, and the
  planned Agent Team page spec;
- follow the architecture and stack decision in
  [frontend-architecture.md](frontend-architecture.md) and ADR
  [0016](../decisions/0016-tailwind-shadcn-adoption.md);
- keep desktop/tablet/mobile ASCII diagrams in each changed page spec current;
- pass screenshot-first acceptance in [acceptance.md](../company-os/frontend-information-architecture.md).

## Workbench Product Flow

The UI must make this workflow inspectable without raw JSON or hidden chat
context:

```text
Mission -> ordered Wave -> executor attempt
  -> assignment/actions/artifacts/outcome
  -> Wave gate -> next Wave or Mission closeout
```

Goal/legacy phase record/Task/Proposal/Decision views remain available for compatibility
and stricter self-hosting governance, but are not mandatory product objects for
every Wave.

## Reading Order

1. [../dashboard.md](../dashboard.md): product-level purpose and information
   architecture.
2. [design-principles.md](../company-os/frontend-information-architecture.md): durable UI doctrine and
   failure modes.
3. [layout-history.md](../company-os/frontend-information-architecture.md): candidate layout directions,
   critique, and the selected/rejected/borrowed decision ledger.
4. [pages/README.md](pages/README.md): page-spec index and template.
5. Page specs under [pages/](pages/): product/UX contract plus page-local
   layout contract per core page.
6. [frontend-architecture.md](frontend-architecture.md): technical stack,
   module boundaries, old-code disposition.
7. [acceptance.md](../company-os/frontend-information-architecture.md): screenshot-first browser and PM/User gates.

## Page Specs

New product work starts from Mission detail, ordered Waves, and the Agent Team
page. Historical Vision/Goal/Task UI material is archived and must not be
copied into Mission/Wave IA.

| Page spec | Owns |
| --- | --- |
| [Team workspace](pages/team-workspace.md) | Future Standing Agents workspace; not current AgentTeamRun IA. |
| [Agent Team page](pages/team-run-console.md) | One AgentTeamRun attempt in its Mission/Wave context: assignment/message ownership, member cockpit, actions, artifacts, gate context, and honest capability degradation. |
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
[layout-history.md](../company-os/frontend-information-architecture.md) first. If it only changes dimensions,
breakpoints, or scroll ownership, update the `## Layout Contract` in that same
page spec.

## Non-Negotiable Implementation Rule

No frontend implementation may start from the old dashboard component tree or
from the rejected PR #6 Workbench shell. Implementation builds on the shipped
rebuild (PR #7) and starts from page specs with accepted page-local layout
contracts, the architecture decision in
[frontend-architecture.md](frontend-architecture.md), and ADR
[0016](../decisions/0016-tailwind-shadcn-adoption.md).
