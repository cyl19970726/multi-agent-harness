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

The execution Workbench uses these native safe-action families:

```text
POST /v1/missions
POST /v1/missions/{id}/close
POST /v1/waves
POST /v1/waves/{id}/gate
POST /v1/team-runs
POST /v1/team-runs/{id}/start
POST /v1/team-runs/{id}/messages
POST /v1/team-runs/{id}/messages/{message_id}/ack
POST /v1/team-runs/{id}/transition
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

The current execution surfaces show:

- Missions: ordered Waves, executor attempts, gate, retry, and closeout;
- Agent Teams: only Mission/Wave-linked runs;
- Team War Room: member presence, assignments, unified activity, messages,
  ACK, start, and attempt lifecycle;
- MemberRuns: run-scoped member detail;
- Workflows: WorkflowRun/WorkflowStep, result, artifacts, and diagnostics;
- the raw snapshot, read-only, behind the Debug boundary.

Company OS surfaces share the shell and expose Home, Docs, Work,
Organization, Approvals, Finance, and Governance from either authoritative
store projections or an explicitly labelled prototype fixture.
