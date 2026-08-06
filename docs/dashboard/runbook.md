# Agent Workbench

The Agent Workbench is the operational UI for the generic firm. Legacy
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
cargo run -p firm-cli -- dashboard snapshot > .firm/dashboard-snapshot.json
```

The web UI no longer loads pasted or file snapshots; it reads the live API.
The raw snapshot behind the UI is viewable read-only via the top-bar Debug
toggle.

For live local state, start the API and point the Workbench's top-bar API URL
control at it:

```bash
cargo run -p firm-cli -- serve --addr 127.0.0.1:8787
```

The Workbench fetches `GET /v1/snapshot`, subscribes to the `/v1/events` SSE
stream for deltas, and offers opt-in interval polling from the top bar. A
multi-project serve is multiplexed with `?project=<id>`; the top-bar project
picker lists `GET /v1/projects` and switches via `POST /v1/projects/switch`.

The execution Workbench uses these native safe-action families:

```text
POST /v1/missions
POST /v1/missions/{id}/context
POST /v1/missions/{id}/link-team
POST /v1/missions/{id}/close
POST /v1/waves
POST /v1/waves/{id}/context
POST /v1/waves/{id}/advance
POST /v1/waves/{id}/gate
POST /v1/team-runs
POST /v1/team-runs/{id}/start
POST /v1/team-runs/{id}/messages
POST /v1/team-runs/{id}/messages/{message_id}/ack
POST /v1/team-runs/{id}/messages/{message_id}/reconcile-delivery
POST /v1/team-runs/{id}/members/{member_run_id}/steer
POST /v1/team-runs/{id}/members/{member_run_id}/interrupt
POST /v1/team-runs/{id}/members/{member_run_id}/close
POST /v1/team-runs/{id}/transition
```

These actions route through the same Rust CLI value paths as operator commands
and return an updated snapshot for the Workbench. They are not local UI-only
state changes.

## Provenance

Second occurrence of "the panel showed something other than Store truth"
(issue #307; first was fixture impersonation, PR #291) was a dashboard served
from a stale, pre-`TeamWorksBoard` commit while the Store/API had Works the
whole time — caught by the user via screenshot, not by the Host. Every surface
now carries enough provenance to answer "is this the truth?" without reading
server logs:

- `GET /v1/meta` returns `{ git_rev, built_at, store_root, latest_op_seq,
  server_version }`. `git_rev`/`built_at` are embedded at **compile time** by
  `crates/firm-cli/build.rs` (a `git rev-parse --short HEAD` build-script
  call, never shelled out per-request); `latest_op_seq` is a monotonic cursor
  over the store's `work_operations.jsonl` append log.
- The Workbench's persistent footer shows that server `git_rev` +
  `latest_op_seq` next to this frontend bundle's OWN build rev (injected by
  `vite.config.ts` via `import.meta.env.VITE_DASHBOARD_GIT_REV`, the same
  `git rev-parse` mechanism run at dev-server/build start). A screenshot of
  any surface carries this strip, so a stale worktree is visible without
  asking. A prominent banner replaces the quiet strip only when the two revs
  disagree, or `/v1/meta` is unreachable — otherwise it stays out of the way.
- `firm dashboard doctor --team-run-id <id> --api <base-url>
  [--expected-git-rev <rev>]` is the operator/CI-facing check: it fetches
  `/v1/meta` and the same `GET /v1/team-runs/{id}/snapshot` the Workbench
  fetches, compares works/members/messages counts and `git_rev` against this
  process's own direct store reads (no HTTP), prints a pass/fail table, and
  exits non-zero on any mismatch. Read-only; performs no writes.

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

- Missions: durable Markdown context, linked Teams, ordered Host-plan Wave
  revisions, explicit judgments, and closeout;
- Agent Teams: independent and Mission-scoped long-lived runs, including
  historical direct-Wave compatibility rows;
- Team War Room: member presence, assignments, unified activity, messages,
  typed authors/recipients, claim/provider receipt/ACK, current Supervisor,
  reconnect state, start, and attempt lifecycle;
- MemberRuns: run-scoped member detail;
- Workflows: WorkflowRun/WorkflowStep, result, artifacts, and diagnostics;
- the raw snapshot, read-only, behind the Debug boundary.

Company OS surfaces share the shell and expose Home, Docs, Work,
Organization, Approvals, Finance, and Governance from either authoritative
store projections or an explicitly labelled prototype fixture.

If the current service does not own a live Team's process handles, the
Workbench still shows durable state and routes real controls through the
current `TeamSupervisorLease.owner_locator`. A missing or stale Supervisor
disables Steer/Interrupt/Close with an explicit reason; the UI never changes
status locally to imply provider work stopped. Member Focus renders runtime
status separately from `active | closed | retired` coordination. Reopen keeps
the same MemberRun, increments its runtime generation, and resumes the recorded
native session; Retire is permanent.
