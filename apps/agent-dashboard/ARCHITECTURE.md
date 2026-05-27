# Agent Dashboard Architecture

The Agent Dashboard is the harness control-plane UI. It reads canonical state
from the Rust harness snapshot/API and derives operator-friendly views.

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
[../../docs/decisions/0014-react-vite-agent-dashboard.md](../../docs/decisions/0014-react-vite-agent-dashboard.md).

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
src/api.ts         # snapshot loading and live polling helpers
src/App.tsx        # top-level state and layout
src/components/    # control-plane panels
src/styles.css     # product UI styling
```

Keep large logic out of `App.tsx`. If a component needs more than local
rendering state, move the derivation into `readModel.ts`.

## Acceptance

The first React version must preserve:

- pasted JSON snapshot loading;
- file snapshot loading;
- live polling from `/v1/snapshot`;
- Kanban, teams, members, messages, sessions, proposals, events, evidence, and
  decisions visibility.

The next product layer adds:

- goal-centered team control-plane layout;
- selected task and selected member details;
- inbox/outbox grouped by task and member;
- workflow warnings derived from snapshot state;
- later safe actions routed through Rust CLI/API.
