# Agent Dashboard

The Agent Dashboard is the operational UI for the generic harness.

Frontend architecture and framework decisions are documented in
[ARCHITECTURE.md](ARCHITECTURE.md).

It should show:

- overview;
- Agent Member roster;
- Agent Team roster;
- message threads;
- task board;
- Kanban columns for task status;
- workspace, branch, and PR refs on task cards;
- member cards with prompt refs, skill refs, runtime status, current task, current proposal, and latest event;
- message delivery state for queued, delivered, acknowledged, and failed messages;
- event timeline from Codex app-server notifications and hooks;
- proposal board for draft, submitted, accepted, rejected, and superseded proposals;
- report index;
- claim ledger;
- blocker board;
- permission queue;
- provider/subagent sessions;
- decision timeline;
- evidence links.

It must not become a project-specific market replay UI. Project evidence should
appear as links and typed evidence references supplied by adapters.

## Static MVP

The first implemented surface supports both snapshot import and a local live
API:

The Dashboard source is built with React, TypeScript, and Vite. Build output is
emitted to `apps/agent-dashboard/web/` so the static artifact remains easy to
open or archive.

Generate data with:

```bash
cargo run -p harness-cli -- dashboard snapshot > .harness/dashboard-snapshot.json
```

Then open the HTML file and load or paste that JSON. This keeps the first UI
decoupled from any backend server while the runtime/API contracts stabilize.
This is an audit entry point, not sufficient proof that persistent agents are
working.

For live local state, start the API and use the dashboard's live URL controls:

```bash
cargo run -p harness-cli -- serve --addr 127.0.0.1:8787
```

Develop and build the frontend with:

```bash
pnpm dashboard:dev
pnpm dashboard:build
```

The dashboard polls `GET /v1/snapshot` and still supports file or pasted
snapshots for offline review. Live mode is not a replacement for evidence
gates; it is a faster way to inspect queued messages, failed deliveries,
provider sessions, and member runtime state.

## Next Dashboard Targets

The next steps are:

- stream new events through SSE or WebSocket after the polling API is stable;
- add task and member detail drawers for source refs, checks, and proposals;
- keep snapshot import for offline review and CI artifacts.
