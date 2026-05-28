# Agent Dashboard Architecture

The Agent Dashboard is the harness control-plane UI. It reads canonical state
from the Rust harness snapshot/API and derives operator-friendly views.

This is the canonical frontend architecture document for the Agent Dashboard
module. Product-level Dashboard purpose and acceptance stay in
[../dashboard.md](../dashboard.md). Core UI/UX principles stay in
[design-principles.md](design-principles.md). Layout variants and accepted
decisions stay in [layout-variants.md](layout-variants.md) and
[layout-decisions.md](layout-decisions.md). Browser and web-quality acceptance
stays in [acceptance.md](acceptance.md). The framework decision stays in
[../decisions/0014-react-vite-agent-dashboard.md](../decisions/0014-react-vite-agent-dashboard.md).
Run and build commands stay in [runbook.md](runbook.md). Global shell, route
layout, core layouts, and responsive behavior stay in
[ui-ux-layout.md](ui-ux-layout.md).

## Source Boundary

```text
Rust CLI/API/store
  -> dashboard snapshot JSON
  -> frontend read model
  -> panels, warnings, and operator navigation
```

The frontend does not own harness state. It can derive warnings that help the
operator see workflow gaps, but those warnings are advisory until promoted to a
Rust/schema/CI gate.

## Frontend Stack

Decision: React + TypeScript + Vite. See
[../decisions/0014-react-vite-agent-dashboard.md](../decisions/0014-react-vite-agent-dashboard.md).

Reasons:

- selected goal/task/member state will be central to the control plane;
- team roster, inbox/outbox, runtime sessions, proposals, evidence, and
  warnings need composable panels;
- TypeScript keeps the snapshot read model explicit;
- Vite keeps the dev/build path light and can emit static files to `web/`.

## Module Shape

The current component tree is not the target architecture for the rebuild. It
may be used only as migration context. The rebuild should replace the old
summary/Kanban/detail/raw-view stack with route-ready modules derived from
[frontend-rebuild-design.md](frontend-rebuild-design.md).

```text
src/types.ts       # snapshot and harness object types
src/readModel.ts   # derived maps, selected entities, warnings
src/api.ts         # snapshot loading, live polling, and safe action helpers
src/App.tsx        # top-level state and layout
src/components/    # control-plane panels
src/styles.css     # product UI styling
```

Keep large logic out of `App.tsx`. If a component needs more than local
rendering state, move the derivation into `readModel.ts`.

## Control Plane Read Model

The Control Plane is goal-scoped:

```text
selected goal
  -> goal graph and goal Kanban projections
  -> tasks for goal
  -> task graph and task Kanban projections
  -> participating members and teams
  -> task/member scoped warnings
  -> selected task and member detail panels
```

The frontend may compute scope, counts, and advisory warnings. It must not
invent canonical task assignment, delivery, evidence, proposal, review, or
decision state. Those objects come from the snapshot.

## Layout Boundary

The frontend layout should follow [ui-ux-layout.md](ui-ux-layout.md):

```text
top bar
  -> Team rail and Team workspace shell
  -> Goal/Task document surfaces
  -> controlled graph/Kanban relationship tabs
  -> Member/Task/Docs/Warn inspector
  -> collapsed debug drawer
```

The default page should not render raw objects or snapshot paste controls as
primary content during live operation. Debug and import surfaces are still
required, but they belong behind an explicit drawer or mode. Components may
change presentation, but they must preserve the workflow proof chain:

```text
vision -> goal -> goal graph/Kanban -> task graph/Kanban
  -> message -> member/runtime -> evidence -> review -> decision
```

## Component Responsibilities

| Component | Owns |
| --- | --- |
| `App.tsx` | snapshot source, live polling state, selected goal/task/member ids, safe action dispatch |
| `ControlPlane.tsx` | goal scope composition and panel wiring |
| `GoalMap.tsx` | goal graph and goal lane projections for generated goals, blockers, follow-ups, and evaluation readiness |
| `TaskWorkSurface.tsx` | task graph and task Kanban projections for the active goal |
| `TaskDetail.tsx` | assignment proof, reports, evidence, sessions, proposal, review, decision, review request action |
| `MemberDetail.tsx` | inbox/outbox, runtime health, provider sessions, child threads, member/message/session actions |
| `WarningsPanel.tsx` | warning display and task/member navigation links |
| `readModel.ts` | selectors and scope helpers |
| `warnings.ts` | advisory warning derivation |

If a warning becomes a gate, move the rule out of `warnings.ts` and into the
Rust schema/CLI/review gate first, then let the Dashboard display the result.

## Acceptance

The first React version must preserve:

- pasted JSON snapshot loading;
- file snapshot loading;
- live polling from `/v1/snapshot`;
- goal graph/lane projections, task graph/lane projections, teams, members,
  messages, sessions, proposals, events, evidence, and decisions visibility.

The next product layer adds:

- richer task graph dependencies and blocker visualization;
- richer member runtime health from provider notifications and hooks;
- click-through warning repair flows;
- create-member and create-task actions;
- production gateway status and metrics panels.
