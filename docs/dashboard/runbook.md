# Agent Workbench

The Agent Workbench is the operational UI for the generic harness. Legacy
commands and package paths still use `dashboard`.

Product-level design and acceptance are in
[../dashboard.md](../dashboard.md). Frontend architecture is in
[frontend-architecture.md](frontend-architecture.md). UI/UX principles are in
[design-principles.md](../company-os/frontend-information-architecture.md). Frontend design is in
[frontend-design.md](frontend-design.md). Frontend acceptance is in
[acceptance.md](../company-os/frontend-information-architecture.md). The read model is in [read-model.md](../company-os/frontend-information-architecture.md).
The React/Vite decision is in
[../decisions/0014-react-vite-agent-dashboard.md](../decisions/0014-react-vite-agent-dashboard.md).

It must not become a project-specific market replay UI. Project evidence should
appear as links and typed evidence references supplied by adapters.

## Run

Generate a snapshot for CLI/audit use:

```bash
cargo run -p harness-cli -- dashboard snapshot > .harness/dashboard-snapshot.json
```

The web UI no longer loads pasted or file snapshots; it reads the live API.
The raw snapshot behind the UI is viewable read-only via the top-bar Debug
toggle.

For live local state, start the API and point the Workbench's top-bar API URL
control at it:

```bash
cargo run -p harness-cli -- serve --addr 127.0.0.1:8787
```

The Workbench fetches `GET /v1/snapshot`, subscribes to the `/v1/events` SSE
stream for deltas, and offers opt-in interval polling from the top bar. A
multi-project serve is multiplexed with `?project=<id>`; the top-bar project
picker lists `GET /v1/projects` and switches via `POST /v1/projects/switch`.

The live API also accepts the safe actions used by the Workbench:

```text
POST /v1/messages
POST /v1/teams
POST /v1/agents
POST /v1/goals
POST /v1/gateway/tick
POST /v1/agents/{id}/deliver
POST /v1/agents/{id}/retry-delivery
POST /v1/agents/{id}/reconcile-session
POST /v1/agents/{id}/close
POST /v1/tasks/{id}/assign
POST /v1/tasks/{id}/reviewer
POST /v1/tasks/{id}/request-review
```

These actions route through the same Rust CLI value paths as operator commands
and return an updated snapshot for the Workbench. They are not local UI-only
state changes.

## Develop

```bash
pnpm dashboard:dev
pnpm dashboard:build
pnpm check:dashboard
```

Build output is emitted to `apps/agent-dashboard/web/` so the static artifact
remains easy to open or archive.

## Current Surface

The Workbench is live-only: SSE deltas are merged in-memory, a reconnect
resyncs the full snapshot from `/v1/snapshot`, and a failed load shows an
empty offline workspace (write actions disabled).

The current surfaces show:

- Agents: roster plus a URL-addressable agent detail page (`?agent=<id>`) with
  conversation timeline, tasks, and config tabs;
- Vision: goal collection grouped by state plus autonomous proposals;
- Work: the goal-collection board; a goal-scoped task lane view remains as the
  drill-in (the flat global task board is retired);
- Goal: phase spine with per-phase task DAG, workflow runs, landed commits,
  and gate evidence; Task: assignment/report/evidence/review/decision proof;
- Workflows: workflow runs and steps (codex/claude/kimi), including
  `goal run-phases` orchestration linked to its goal/phase;
- Docs: registry-backed project docs via `GET /v1/docs`;
- safe actions for send message, create team/agent/goal, deliver, retry
  delivery, reconcile session, close member, assign task, set reviewer, and
  request review;
- the raw snapshot, read-only, behind the top-bar Debug toggle.
