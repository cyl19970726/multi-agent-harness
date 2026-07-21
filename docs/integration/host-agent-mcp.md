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
- Member execution: Kimi ACP, Codex batch (`codex_exec`), and Codex interactive
  (`codex_app_server`) are executable Team Member modes. Claude Team Member
  execution remains unsupported and fails honestly.
- `team_run_start` reserves the run and returns immediately while members run
  in the background.
- Every create/start/status/cancel/ACK result includes an exact TeamRun URL.
  When project identity is available it includes `project=<workspace-id>`.
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
registered MCP tools appear. The Dashboard is a separate long-running process:

```bash
target/debug/harness --project <workspace-id> serve --addr 127.0.0.1:8787
```

`project_id` is the technical Harness Workspace identity. It routes the
central store and repository execution root; it is not a Company OS Project
business object. Product copy should say **Workspace**.

## Host Journey

1. Call `mission_create` for durable intent.
2. Call `wave_create` with `executor_kind=agent_team` for the next lightweight
   outcome boundary.
3. Call `team_run_create` with role-specific supported provider members and
   disjoint owned paths. Keep the returned Assignment message ids and
   correlations.
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
records from a separately executed real Kimi run.
