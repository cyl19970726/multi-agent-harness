# Agent Workbench Frontend Design Index

This file is now an index for the Agent Workbench frontend design contract. It
is intentionally not a giant all-in-one design document. Product semantics,
page specs, page-local layout contracts, architecture, acceptance, and rejected
implementations live in separate files so future implementation cannot drift
from vague prose.

Product purpose stays in ../dashboard.md. Durable design
principles stay in design-principles.md. Layout
candidates, critique, and the selected/killed/deprecated decision ledger stay in
layout-history.md. Page-level specs live under
[pages/](pages/), and each page file owns its current `## Layout Contract`
with detailed desktop/tablet/mobile ASCII diagrams. Architecture and stack
decisions stay in [frontend-architecture.md](frontend-architecture.md) and ADR
[0016](../../decisions/0016-tailwind-shadcn-adoption.md) (stack). Acceptance gates stay in
acceptance.md.

Dynamic Workflow is retired and has no current Workbench route. If historical
records are exposed at all, they are a read-only legacy archive journey for
export/verify/restore-read, never a live execution or mutation surface.

Durable AgentTeams + Work direction is canonical in ../architecture-map.md.
The retired Mission + Mission Log direction (ADR 0051) and its predecessor
Mission/Wave design (ADR 0026) are history. The archived Vision/Goal/Task
work-board and Goal Workbench contracts are also history; they no longer
define the top-level product IA.

## Documentation placement

| Path | Owns |
| --- | --- |
| `../dashboard.md` | Workbench purpose, information architecture, object workflow and product acceptance. |
| `frontend-design.md` | Design reading order, page map and implementation-readiness summary. |
| `pages/README.md` and `pages/*.md` | Page indexes and page-local product, UX, responsive layout and screenshot contracts. |
| `frontend-architecture.md` | React application boundaries, data flow, primitives and stack policy. |
| `runbook.md` | Build, run, snapshot, live API and safe-action procedures. |
| `../decisions/` | Durable architectural decisions only. |

For non-trivial UI work, change the product contract and selected page spec
before implementation, then capture browser evidence and record both product
truth and visual-fidelity results. Do not mix product meaning, rejected layout
exploration and component internals in a new all-in-one document.

## Current Design Status

```text
status:
  top_level_direction: shipped
  current_implementation: Execution Workbench V3 on the active product branch
  stack: React 18 + TypeScript + Vite + Tailwind v4 + shadcn/Radix + lucide + Geist
  theme: light, Notion-like document surface (supersedes 0016 dark theme)
  mission_log_direction: retired with DOC-108; historical rows are Legacy read-only
  agent_team_page: converged onto durable Team surfaces; pages/agent-member-focus.md
  retired_coordination_ui: removed from active product surfaces
  implementation_allowed: follow the canonical page and visual contracts
```

The Agent Workbench uses Tailwind v4 + shadcn/ui (Radix) + lucide-react + Geist
with generated identity art and purpose-built execution primitives. Historical
shell decisions are retained only as design provenance in the frontend IA.
New current product UI changes must:

- start from the architecture map, ADR 0051, and the implemented flat
  AgentTeam, Team Workspace, Global Work, and current RoleView page specs;
- follow the architecture and stack decision in
  [frontend-architecture.md](frontend-architecture.md) and ADR
  [0016](../../decisions/0016-tailwind-shadcn-adoption.md);
- keep desktop/tablet/mobile ASCII diagrams in each changed page spec current;
- pass screenshot-first acceptance in acceptance.md.

Mission Detail, Mission Log, and Agent Team War Room specs are historical
design evidence only. They cannot authorize new navigation, writers, or current
page requirements.

## Workbench Product Flow

The UI must make this workflow inspectable without raw JSON or hidden chat
context:

```text
durable Team context -> append-only WorkEvent judgment/replan/recovery lineage
  -> one flat AgentTeam on one ExecutionNode
  -> shared Works / WorkDelivery / linked conversation / native sessions
  -> Member submit -> Host accept or request changes
  -> Host submit -> exact active peer accept or send linked revision feedback
  -> reviewer judgment -> plan adjustment, recovery, or Work acceptance
```

The retired coordination views are absent from active product navigation and
must not be reintroduced as compatibility UI. Stricter governance may add
review evidence without introducing another Mission lifecycle object.

## Reading Order

1. ../dashboard.md: product-level purpose and information
   architecture.
2. design-principles.md: durable UI doctrine and
   failure modes.
3. layout-history.md: candidate layout directions,
   critique, and the selected/rejected/borrowed decision ledger.
4. [pages/README.md](pages/README.md): page-spec index and template.
5. Page specs under [pages/](pages/): product/UX contract plus page-local
   layout contract per core page.
6. [frontend-architecture.md](frontend-architecture.md): technical stack,
   module boundaries, old-code disposition.
7. acceptance.md: screenshot-first browser and PM/User gates.

## Page Specs

New product work starts from the durable flat AgentTeam, Team Workspace, and
Global Work surfaces. Mission/Mission Log and historical Vision/Goal/Task UI
material are archived evidence and must not re-enter current IA, navigation, or
mutation flows.

| Page spec | Owns |
| --- | --- |
| Agent Teams Home | Durable flat AgentTeams with immutable Node placement and their runs. |
| Team Workspace | One durable Team: shared Works ownership/status, Work-linked conversation, member presence, unified activity, and artifacts. |
| Agent Conversation Workspace | One Host/Member conversation surface with exact Session activity, authored Messages, and Work responsibility. |
| Global Work Index | The read-only Global Work aggregate over authoritative TeamWork. |
| [AgentMember Focus](pages/agent-member-focus.md) | One canonical AgentMember: organization projection, responsibilities, TeamWorks, availability, and execution trust. |
| [Debug](pages/debug.md) | Raw snapshot, import/export, and low-level object views outside the primary work surface. |

Work lifecycle actions belong to their Team Workspace surfaces. Execution
pressure belongs to the TeamRun or MemberRun record that produced it. There is
no replacement global Goal-era decision or warnings page, and the retired
Mission and retired Company OS pages have no successor page in this shell.

## Source Of Truth Boundary

| File group | Owns | Refuses |
| --- | --- | --- |
| `pages/*.md` | Page purpose, user question, canonical objects, workflow proof, IA, actions, detailed desktop/tablet/mobile ASCII diagrams, dimensions, scroll ownership, failure modes, screenshot matrix. | Component internals. |
| `layout-history.md` | Candidate critique, scoring, and why a design was selected, killed, or borrowed, including rejected-implementation outcomes. | Implementation code or screenshot pass/fail logs. |
| `frontend-architecture.md` | Stack, routing, state, component boundaries, graph/canvas strategy, old-code disposition. | Product purpose or page-level UX. |
| `acceptance.md` | Browser screenshot rubric, PM/User prompts, web-quality gates, waiver policy. | Layout candidates or component architecture. |

If a layout change alters page meaning, update the relevant page spec and
layout-history.md first. If it only changes dimensions,
breakpoints, or scroll ownership, update the `## Layout Contract` in that same
page spec.

## Non-Negotiable Implementation Rule

Frontend implementation starts from current page specs, V3 visual evidence,
and the current component tree. It must not restore a retired coordination
surface or rejected shell. The architecture decision lives in
[frontend-architecture.md](frontend-architecture.md), and ADR
[0016](../../decisions/0016-tailwind-shadcn-adoption.md).
