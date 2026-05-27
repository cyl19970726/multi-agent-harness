# 0014: React/Vite Agent Dashboard Frontend

## Decision

Use React, TypeScript, and Vite for the Agent Dashboard frontend.

Keep the Rust CLI/API as the source of truth for harness state. The frontend
may derive read-model warnings for operator visibility, but stable acceptance
rules must move into schemas, Rust code, CLI/API, or CI gates before they are
treated as canonical.

## Context

The original Dashboard was a vanilla HTML/JS snapshot viewer. That was a good
phase-0 audit surface, but the next product shape is a team control plane:

- selected goal, task, team, and member state;
- task detail panels with assignment, reports, evidence, proposals, reviews,
  and decisions;
- member panels with runtime health, queue, inbox/outbox, provider sessions,
  and child threads;
- derived workflow warnings;
- live polling now and SSE/WebSocket later;
- offline snapshot loading for CI artifacts and local review.

Hand-rolled DOM updates would push too much state orchestration into one file.

## Options

| Option | Result |
| --- | --- |
| Keep vanilla JS | Small dependency footprint, but state and UI composition become brittle as views become linked. |
| React + Vite | Component model, typed read model, light local build, static output can still be opened directly. |
| Next.js | Strong app framework, but unnecessary server/runtime layer for a Rust-backed local control plane. |

## Consequences

The Dashboard source should be split by responsibility:

```text
apps/agent-dashboard/src/
  types.ts          # snapshot and harness read-model types
  readModel.ts      # UI-only derived view and warning helpers
  api.ts            # live snapshot loading
  components/       # panels, boards, lists, controls
  App.tsx           # composition
```

Build output may continue to live under `apps/agent-dashboard/web/` so users
can open the Dashboard without a separate frontend server. Development uses
Vite.

## Validation

Frontend changes should pass:

```bash
pnpm check:dashboard
pnpm check
```

The first implementation should keep existing snapshot import and live polling
working before adding mutating Dashboard actions.
