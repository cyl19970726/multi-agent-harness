# Agent Dashboard Architecture

The Agent Dashboard is the harness control-plane UI. It reads canonical state
from the Rust harness snapshot/API and derives operator-friendly views.

This is the canonical frontend architecture document for the Agent Dashboard
module. Product-level Dashboard purpose and acceptance stay in
[../dashboard.md](../dashboard.md). The framework decision stays in
[../decisions/0014-react-vite-agent-dashboard.md](../decisions/0014-react-vite-agent-dashboard.md).
Run and build commands stay in [runbook.md](runbook.md).

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
  -> tasks for goal
  -> participating members and teams
  -> task/member scoped warnings
  -> selected task and member detail panels
```

The frontend may compute scope, counts, and advisory warnings. It must not
invent canonical task assignment, delivery, evidence, proposal, review, or
decision state. Those objects come from the snapshot.

## Component Responsibilities

| Component | Owns |
| --- | --- |
| `App.tsx` | snapshot source, live polling state, selected goal/task/member ids, safe action dispatch |
| `ControlPlane.tsx` | goal scope composition and panel wiring |
| `KanbanBoard.tsx` | task status columns for the active goal |
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
- Kanban, teams, members, messages, sessions, proposals, events, evidence, and
  decisions visibility.

The next product layer adds:

- richer task graph dependencies and blocker visualization;
- richer member runtime health from provider notifications and hooks;
- click-through warning repair flows;
- create-member and create-task actions;
- production gateway status and metrics panels.
