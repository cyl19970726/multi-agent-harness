# Agent Dashboard

The Agent Dashboard is the operational UI for the generic harness.

Product-level design and acceptance are in
[../dashboard.md](../dashboard.md). Frontend architecture is in
[frontend-architecture.md](frontend-architecture.md). The read model is in
[read-model.md](read-model.md). The React/Vite decision is in
[../decisions/0014-react-vite-agent-dashboard.md](../decisions/0014-react-vite-agent-dashboard.md).

It must not become a project-specific market replay UI. Project evidence should
appear as links and typed evidence references supplied by adapters.

## Run

Generate a snapshot:

```bash
cargo run -p harness-cli -- dashboard snapshot > .harness/dashboard-snapshot.json
```

Open `apps/agent-dashboard/web/index.html`, then load or paste the JSON.

For live local state, start the API and use the Dashboard's live URL controls:

```bash
cargo run -p harness-cli -- serve --addr 127.0.0.1:8787
```

## Develop

```bash
pnpm dashboard:dev
pnpm dashboard:build
pnpm check:dashboard
```

Build output is emitted to `apps/agent-dashboard/web/` so the static artifact
remains easy to open or archive.

## Current Surface

The Dashboard polls `GET /v1/snapshot` and still supports file or pasted
snapshots for offline review. Live mode stops when loading fails, and pasted or
file snapshots stop live polling.

The current Control Plane shows:

- selected goal scope;
- filtered task Kanban, teams, members, and warnings;
- task assignment proof, reports, evidence, sessions, proposals, reviews, and decisions;
- member inbox/outbox, runtime health, provider sessions, and child threads;
- raw object views for audit/debugging.
