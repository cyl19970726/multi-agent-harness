# Agent Workbench Frontend Architecture

This document owns the frontend architecture and technology-stack decision for
Agent Workbench. Product purpose stays in [../dashboard.md](../dashboard.md).
Page-level UX and layout contracts stay in [pages/](pages/). Layout history and
the rejected/selected decision ledger stay in [layout-history.md](layout-history.md).
Acceptance gates stay in [acceptance.md](acceptance.md). The stack decision is
recorded as ADR [0016](../decisions/0016-tailwind-shadcn-adoption.md).

## Current Decision

```text
status: shipped (PR #7); theme repivoted to light Notion (ADR 0019)
implementation_allowed: yes, from page-local layout contracts + work-board-design.md
decision:
  React 18 + TypeScript + Vite build/runtime shell
  Tailwind CSS v4 (@tailwindcss/vite) for styling and tokens
  shadcn/ui primitives over Radix (components.json, style new-york)
  lucide-react icons; Geist + Geist Mono fonts
  light, Notion-like token theme in src/index.css (ADR 0019 supersedes the
    0016 dark operator-console theme; decoration removed)
  document atoms (DocumentSurface / DocSection / DocProperties) for the
    Notion-style Goal/Task detail pages
  dependencies live in the ROOT package.json (no per-app package.json)
  module boundary: src/app, src/surfaces, src/model, src/components
```

The Agent Workbench frontend was rebuilt and merged in PR #7 on React +
TypeScript + Vite with Tailwind CSS v4, shadcn/ui primitives over Radix,
lucide-react icons, and the Geist font family. The original rebuild used a dark
operator-console token theme; ADR
[0019](../decisions/0019-vision-goal-task-workbench-redesign.md) **repivoted the
theme to a light, Notion-like document surface** (decoration removed) and added
the document atoms used by the Goal/Task detail pages, superseding the theme
tokens of ADR [0016](../decisions/0016-tailwind-shadcn-adoption.md) (the stack
decision itself stands). The earlier hand-rolled-CSS shell was rejected for
product-architecture and acceptance reasons (card/tab dumps, vague layout specs),
not because of the build path. ADR 0016 supersedes ADR
[0014](../decisions/0014-react-vite-agent-dashboard.md) in part.

This decision must be re-opened if implementation needs routing, graph,
collaboration editing, or state-management capabilities that the current stack
cannot support.

## Source Boundary

```text
Rust harness store / CLI / API
  -> dashboard snapshot JSON
  -> frontend read model selectors
  -> Workbench primitives and page surfaces
  -> screenshot-first browser acceptance
```

The frontend does not own canonical harness state. It can derive operator
views, selection state, advisory warnings, and disabled reasons. It must not
invent assignment, evidence, review, decision, goal completion, or graph
mutation truth.

## Stack Choice

| Area | Decision | Rationale |
| --- | --- | --- |
| Framework | React 18 | Good composition for page/workbench surfaces. |
| Language | TypeScript strict mode | Snapshot/read-model contracts must remain explicit. |
| Bundler | Vite | Lightweight local dev and static `web/` output. |
| Styling | Tailwind CSS v4 via `@tailwindcss/vite` | Token-driven utility styling tied to page-local contracts; light, Notion-like theme in `src/index.css` (ADR 0019, supersedes the 0016 dark operator-console theme). |
| UI kit | shadcn/ui primitives over Radix (`components.json`, style `new-york`) in `src/components/ui` | Accessible Radix behavior with copy-in primitives the product owns and can adapt. |
| Product atoms | `src/components/workbench` | Workbench-specific atoms composed from the shadcn/ui primitives. |
| Icons | `lucide-react` | Use icon+tooltip/label where it clarifies action. |
| Fonts | Geist + Geist Mono | Operator-console typography for UI and code/data. |
| Routing | Route-ready internal state first; add router only when page specs require URL routing | Avoid adding dependency before page contracts stabilize. |
| Graph | Defer library choice until the `graph-kanban` page layout contract is accepted | Graph must be semantic and controlled, not decorative canvas. |
| State | Local app state + pure read-model selectors first | Canonical state comes from snapshot/API; avoid store abstraction until needed. |
| Dependencies | Declared in the ROOT `package.json` (no `apps/agent-dashboard/package.json`) | Single dependency surface for the gated monorepo build. |

## Component Decision

The first rebuild uses product primitives, not a generic UI kit. These are the
implementation components for the next slice:

| Component | Owns | Refuses |
| --- | --- | --- |
| `WorkbenchShell` | top bar, app rail, responsive workspace grid, source state, debug boundary | business proof logic |
| `AppRail` | stable navigation across Team, Vision, Goal, Task, Graph/Kanban, Member, Docs, Decisions, Warnings, Debug | raw object browsing |
| `TopBar` | live/offline source, active Vision/Goal, API input, refresh, search affordance | page content |
| `TeamRail` | team switcher, role groups, member rows, queue/current-work pressure | graph visualization |
| `Inspector` | selected member/task/docs/warnings/evidence/decision context | primary workflow ownership |
| `TeamWorkspace` | default collaboration workspace, activity stream, current work, decisions, warnings | roster-only dashboard |
| `MemberWorkbench` | durable teammate view: identity, current work, inbox/outbox, timeline, runtime, prompt/skills | provider-session dump |
| `VisionOverview` | goal collection, completed/not-complete proof, distance-to-vision, next proposals | single-goal status card |
| `GoalDocument` | GoalDesign, team design, branch policy, graph/Kanban preview, evidence/review/decision, evaluation | task list |
| `TaskDocument` | assignment -> report -> evidence -> proposal -> review -> decision proof order | status card |
| `GraphKanban` | Kanban default plus semantic graph focus and synchronized selected object | graph-first shell |
| `DocsContext` | mounted docs context with source paths and missing-context warnings | copied docs body |
| `DecisionCenter` | global and object-local Evidence/Proposal/Review/Decision lanes | status chip |
| `WarningsRepair` | workflow risk queue with affected object, cause, consequence, safe repair state | toast-only alerts |
| `DebugSurface` / `DebugDrawer` | raw snapshot and import/export only behind explicit debug route/drawer | primary viewport |

Shipped divergence from that plan table: the primary rail is Agents / Vision /
Work / Workflows / Docs, with Goal/Task drill-in documents and Debug behind a
top-bar toggle. The Team-shaped components (`TeamRail`, `TeamWorkspace`) did not
ship — the current UI has no Team surface — and `MemberWorkbench` shipped as the
Agents area (`AgentsList` + `AgentDetail`). Workflow-run visibility added
`surfaces/Workflows.tsx` and `components/workbench/WorkflowPanels.tsx`.

Product atoms in `src/components/workbench` are composed from the shadcn/ui
primitives and preserve the product model:

| Primitive | Purpose |
| --- | --- |
| `StatusDot` + tone maps (`tones.ts`) | text-backed state dots/labels, never color-only. |
| `Section` / `SurfaceHeader` | bounded workspace region and surface heading with kicker/action slots. |
| `DocumentSurface` / `DocSection` / `DocProperties` | Notion-style Goal/Task document atoms (ADR 0019). |
| `TimelineRow` | canonical Message/Event/Evidence/Decision rows. |
| `Avatar`, `AgentSparkline`, `CollapsibleBlock`, `MetaList`, `EmptyState`, `Kbd`, `MonoId` | supporting display atoms (`atoms.tsx`). |
| `Markdown` | markdown rendering for doc/plan bodies. |
| `OperatorForms` | safe-action dialogs that dispatch typed `api/actions.ts` descriptors. |
| `WorkflowPanels` | workflow definition/run summary panels shared by the Goal and Workflows surfaces. |

The rebuild uses Tailwind CSS v4 plus shadcn/ui primitives over Radix as the
base layer. Material UI, Ant Design, and other full component frameworks remain
out of scope.

## Dependency Policy

- shadcn/ui primitives are added through `components.json` (style `new-york`)
  into `src/components/ui`; product atoms wrap them in `src/components/workbench`.
- All dependencies are declared in the ROOT `package.json`; there is no
  `apps/agent-dashboard/package.json`.
- Do not add a second full component framework without a recorded Reviewer
  decision.
- Do not add a graph/canvas library until [work-board-design.md](work-board-design.md)
  and its layout contract require capabilities that custom SVG/HTML cannot
  provide.
- Any dependency must name the page spec it serves and how it will be
  screenshot-accepted.

## Workbench Primitives

The rebuild starts from product primitives, not dashboard widgets. (Design
vocabulary; the shipped-divergence note in Component Decision applies here too —
Team-shaped primitives did not ship.)

| Primitive | Purpose |
| --- | --- |
| `WorkbenchShell` | Top bar, app navigation, workspace, inspector, debug boundary. |
| `AppRail` | Stable product navigation across Vision, Team, Work, Member, Docs, Warnings, Debug. |
| `TeamRail` | Team switcher, role groups, member rows, queue/current work pressure. |
| `Workspace` | Primary work surface for Team, Work, Vision, or document surfaces. |
| `MemberWorkbench` | Durable member view: identity, current work, inbox/outbox, timeline, runtime, actions. |
| `DocumentSurface` | Goal and Task document sections with proof order. |
| `MessageTimeline` | Canonical activity rows tied to messages, sessions, evidence, proposals, decisions. |
| `LaneBoard` | Kanban/list projection for Goal/Task execution. |
| `GraphFocus` | Controlled semantic graph focus when accepted by the Graph/Kanban page layout contract. |
| `Inspector` | Secondary context for selected Member, Task, Docs, Warnings, Evidence, Decision. |
| `DebugDrawer` | Raw snapshot/import/export outside the primary viewport. |

## Old Code Disposition

This disposition was executed in the PR #7 rebuild: the listed old components
and styles no longer exist. `api.ts`/`types.ts`/`vite.config.ts` were retained,
and `readModel.ts` migrated to `src/model/readModel.ts`. The table stays as the
record of what was decided.

| Path/pattern | Decision | Reason |
| --- | --- | --- |
| `apps/agent-dashboard/src/components/SummaryGrid.tsx` | delete or quarantine | Encodes metrics/dashboard-first composition. |
| `apps/agent-dashboard/src/components/RawViews.tsx` | quarantine behind Debug only or replace | Raw views cannot drive primary viewport. |
| `apps/agent-dashboard/src/components/ControlPlane.tsx` | delete or replace | Old composition encourages card/tab dashboard. |
| `apps/agent-dashboard/src/components/*Detail*.tsx` | review before reuse | Detail panels may be useful only if converted to page/workbench primitives. |
| `apps/agent-dashboard/src/styles/*.css` | delete or replace | Old styles encode failed layout and dashboard density. |
| `apps/agent-dashboard/src/App.tsx` from PR #6 | delete | Rejected implementation, not patchable. |
| `apps/agent-dashboard/src/api.ts` | retain | Stable API helper if it stays layout-neutral. |
| `apps/agent-dashboard/src/types.ts` | retain | Snapshot types are layout-neutral. |
| `apps/agent-dashboard/src/readModel.ts` | review/migrate | Retain only pure selectors that serve page specs. |
| `apps/agent-dashboard/vite.config.ts` | retain | Build boundary still valid. |

No old component may drive the first viewport unless the Reviewer records it as
a retained Workbench primitive with a page spec and screenshot acceptance path.

## Module Boundary

Shipped shape after the rebuild (PR #7):

```text
src/
  app/            # App composition, WorkbenchShell, selection state, SSE hook
  surfaces/       # agents (list + detail), vision, goal, task, work board,
                  #   workflows, docs, debug page surfaces
  model/          # read-model selectors, warnings, workflow selectors/shape
  components/
    ui/           # shadcn/ui primitives over Radix (components.json, new-york)
    workbench/    # product atoms composed from the ui primitives
  api.ts, api/    # snapshot/SSE/projects fetch + typed write-action descriptors
  types.ts        # snapshot and UI object types
  index.css       # Tailwind v4 entry + light Notion-like token theme (ADR 0019)
```

shadcn configuration lives in `apps/agent-dashboard/components.json`. The build
boundary stays under `apps/agent-dashboard/`, but all dependencies are declared
in the ROOT `package.json`.

## Graph Strategy

Initial implementation should default to lane/list views. Graph is added as a
controlled focus surface only when:

- nodes and edges are semantic;
- selection synchronizes with document/inspector context;
- mobile has a list fallback;
- topology changes route through Proposal/Decision, not local mutation;
- screenshots prove graph does not become the default Team product.

Possible future options:

- custom SVG/HTML for small semantic graphs;
- React Flow or equivalent when pan/zoom/minimap/collapse/search are necessary;
- no canvas for initial slice if Kanban/list can satisfy acceptance.

## Acceptance Implications

Architecture acceptance requires:

- import audit proving old dashboard components do not drive first viewport;
- styling through Tailwind v4 plus shadcn/ui primitives over Radix, not a
  second full component framework;
- page specs and page-local layout contracts linked from implemented surfaces;
- screenshot-first PM/User acceptance;
- rejected implementation outcomes recorded in
  [layout-history.md](layout-history.md) for failed browser-visible attempts.
