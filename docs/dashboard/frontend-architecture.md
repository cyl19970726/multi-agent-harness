# Agent Workbench Frontend Architecture

This document owns the frontend architecture and technology-stack decision for
Agent Workbench. Product purpose stays in [../dashboard.md](../dashboard.md).
Page-level UX contracts stay in [pages/](pages/). Hard layout geometry stays in
[hard-layout-specs/](hard-layout-specs/). Acceptance gates stay in
[acceptance.md](acceptance.md).

## Current Decision

```text
status: planned
implementation_allowed: no
decision:
  keep React + TypeScript + Vite as the build/runtime shell for now
  rebuild the product architecture from Workbench primitives
  delete or quarantine failed dashboard/PR #6 UI composition
  do not use shadcn
```

React + TypeScript + Vite are not the cause of the failed frontend. The failure
was product architecture and acceptance discipline: old dashboard components,
card/tab composition, and vague layout specs shaped the implementation. The
next rebuild keeps the lightweight build path while replacing the UI structure.

This decision must be re-opened if implementation needs routing, graph,
collaboration editing, state management, or browser acceptance capabilities that
the current stack cannot support.

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
| Framework | React 18 | Existing build path, good composition for page/workbench surfaces. |
| Language | TypeScript strict mode | Snapshot/read-model contracts must remain explicit. |
| Bundler | Vite | Lightweight local dev and static `web/` output. |
| UI kit | None; no shadcn | The product needs custom Workbench primitives, not generic card/dialog defaults. |
| Icons | `lucide-react` allowed | Existing dependency; use icon+tooltip/label where it clarifies action. |
| Styling | Custom CSS with design tokens | Avoid kit-driven aesthetics and keep layout tied to hard specs. |
| Routing | Route-ready internal state first; add router only when page specs require URL routing | Avoid adding dependency before page contracts stabilize. |
| Graph | Defer library choice until `graph-kanban` hard spec is accepted | Graph must be semantic and controlled, not decorative canvas. |
| State | Local app state + pure read-model selectors first | Canonical state comes from snapshot/API; avoid store abstraction until needed. |

## Dependency Policy

- Do not add shadcn.
- Do not add a generic component library without a recorded Reviewer decision.
- Do not add a graph/canvas library until [pages/graph-kanban.md](pages/graph-kanban.md)
  and its hard layout spec require capabilities that custom SVG/HTML cannot
  provide.
- Prefer custom Workbench primitives over generic cards.
- Any dependency must name the page spec it serves and how it will be
  screenshot-accepted.

## Workbench Primitives

The rebuild starts from product primitives, not dashboard widgets:

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
| `GraphFocus` | Controlled semantic graph focus when accepted by hard layout spec. |
| `Inspector` | Secondary context for selected Member, Task, Docs, Warnings, Evidence, Decision. |
| `DebugDrawer` | Raw snapshot/import/export outside the primary viewport. |

## Old Code Disposition

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

Target shape after rebuild:

```text
src/
  app/
    App.tsx
    WorkbenchShell.tsx
    selection.ts
  api/
    client.ts
  model/
    types.ts
    readModel.ts
    warnings.ts
  surfaces/
    team/
    member/
    vision/
    goal/
    task/
    graph-kanban/
    docs/
    decisions/
    warnings/
    debug/
  ui/
    primitives/
    tokens.css
    layout.css
```

This is a target architecture, not permission to implement. Implementation
begins only after page specs and hard layout specs are accepted.

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
- no shadcn dependency;
- page specs linked from implemented surfaces;
- hard layout specs linked from implemented slices;
- screenshot-first PM/User acceptance;
- rejected implementation records for failed browser-visible attempts.
