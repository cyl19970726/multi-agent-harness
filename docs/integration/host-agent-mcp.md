# Host Agent MCP Integration

## Product Contract

The Host Agent is the user's interactive Codex, Claude Code, Kimi Code, or
another long-lived coding agent. It is not an Agent Team member. Its default
control surface is MCP:

```text
Host Agent
  -> harness MCP (typed authoring and control)
  -> shared Rust application operations
  -> Mission / Wave / AgentTeamRun / Store
  -> provider member adapter

Dashboard <- HTTP + SSE projections of the same store
CLI       <- human, CI, diagnosis, and fallback surface
```

Skills may teach the Host when to form a team and how to gate a Wave, but they
do not own product truth or execute runtime operations. Commands and hooks are
optional conveniences. Provider-specific integration packs configure these
parts; they do not fork the core model.

## Current Executable Boundary

- Host: Codex can call the stdio MCP server after local registration below.
- Coordination: Mission, ordered Wave, and AgentTeamRun are native.
- Member execution: Kimi ACP, Codex batch (`codex_exec`), Codex interactive
  (`codex_app_server`), and Claude CLI (`claude_cli`) are registered executable
  Team Member modes. Any other provider or mode is rejected explicitly; Harness
  never silently substitutes Codex or invents a native session.
- `team_run_start` reserves the run and returns immediately while members run
  in the background.
- Every create/start/status/cancel/ACK result includes an exact TeamRun URL on
  the UI origin (`127.0.0.1:5173`), with `api=.` so API and SSE requests use the
  UI's same-origin `/v1` proxy. When project identity is available it includes
  `project=<workspace-id>`.
- Temporary development policy gives every Agent Team member full execution
  permission. Codex batch turns launch with `danger-full-access`; Kimi ACP tool
  approvals are resolved immediately by `policy`. `AskUserQuestion` and
  `PlanReview` still pause and route to Lead. Requests and resolutions remain
  durable coordination evidence; provider transcripts and thinking do not.
- Thinking is allowed only as sanitized transient live state. It is never
  persisted, replayed, forwarded to peers, or accepted as evidence.

## Codex Registration

Build the binary, initialize/select the Workspace, then register its absolute
path and explicit project identity:

```bash
cargo build -p harness-cli
target/debug/harness init
codex mcp add harness -- \
  /absolute/path/to/target/debug/harness \
  --project <workspace-id> mcp
codex mcp get harness
```

An existing Codex conversation may require a new session before the newly
registered MCP tools appear. The API and Dashboard UI are separate long-running
processes. Start the Vite UI with its same-origin proxy pointed at the API:

```bash
target/debug/harness --project <workspace-id> serve --addr 127.0.0.1:8787
HARNESS_CAPTURE_API_PROXY=http://127.0.0.1:8787 npm run dashboard:dev
```

The MCP URL opens `http://127.0.0.1:5173` and sets `api=.`. Port 8787 is an API
origin, not a human Dashboard URL.

`project_id` is the technical Harness Workspace identity. It routes the
central store and repository execution root; it is not a Company OS Project
business object. Product copy should say **Workspace**.

## Store root is not execution root

`store_root` contains Harness JSONL coordination ledgers. Provider processes do
not run there. Their cwd is selected in this order: member `worktree_ref`,
TeamRun `execution_root`, then selected Workspace `project_root`; the Host cwd
is only the creation default for an unrouted legacy raw-store invocation.
`team_run_create` exposes `execution_root` and `members[].worktree_ref` through
CLI (`--execution-root`, `--member-worktree name:path`), HTTP, and MCP. An
override must be the selected project root or a Git worktree sharing its Git
common directory, including external Codex worktrees.

That provider cwd controls project instruction and configuration discovery:
Codex walks `AGENTS.md` and its project/root skill/config locations from that
execution root; Claude and Kimi likewise load project-level instruction and
configuration files from the spawned project/worktree context. Moving the
central store must therefore never change provider cwd, and passing a store
path as an execution root is a routing defect.

## Host Journey

1. Call `mission_create` for durable intent.
2. Call `wave_create` with `executor_kind=agent_team` for the next lightweight
   outcome boundary.
3. Call `team_run_create` with role-specific supported provider members,
   disjoint owned paths, and workspace overrides only when needed. Keep the
   returned execution/member roots, Assignment message ids, and correlations.
4. Call `team_run_start`; immediately give the user its `dashboard_url`.
5. Follow `team_run_status` or `team_run_events(after_seq=...)`. The browser
   receives durable Harness coordination plus transient/on-demand activity
   projected from provider-native sessions through SSE/API.
6. When a provider pauses for input, inspect its `PendingInteraction` and call
   `team_run_resolve_interaction` with the exact option id and authorized actor.
   Do not treat provider `completed` as proof of semantic approval or answer.
7. For a running `codex_app_server` member, use `team_run_steer_member` to
   inject input into the same turn. Use `team_run_interrupt_member` for either
   Codex app-server or Kimi ACP when cooperative cancellation is intended.
   Other messages use `team_run_send_message` and are delivered next round.
8. Acknowledge delivered handoffs with `team_message_acknowledge`.
9. Check outcomes and artifacts, then call `wave_gate` with
   `accepted | revise | blocked`. Acceptance names the completed attempt.

## Experience Acceptance

The integration is usable only when a user can start from a Codex prompt and
reconstruct the result from native state:

- Mission and ordered Wave exist;
- the TeamRun is linked to both;
- actual MemberRuns have Assignment messages and correlations;
- start returns without blocking the Host conversation;
- the exact URL opens the correct Workspace and selected TeamRun;
- handoffs and ACKs appear in the event stream;
- provider interactions preserve route, resolution actor, exact option id, and
  distinct transport/semantic status;
- outcome, useful artifacts/checks, and the Wave gate explain acceptance;
- no durable thinking rows are created.

Run the deterministic product gate with:

```bash
npx pnpm@9.15.4 acceptance:mission-wave
```

This gate is not proof of a real provider call. Live claims require the native
records from a separately executed run in the claimed provider mode.
