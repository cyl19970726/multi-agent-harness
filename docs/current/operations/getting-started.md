# Getting started

Star Harness provides durable Agent Teams, Work, identity-first Messages,
provider-native sessions, and fenced RuntimeCommands. Dynamic Workflow and the
`star-workflow` authoring/runtime surface are retired; do not install or invoke
them as a current execution path.

## Prerequisites

- Rust stable for the `firm` binary.
- Node and pnpm for the Dashboard and documentation checks.
- An authenticated supported provider: Codex app-server, Claude Agent SDK
  streaming, or Kimi ACP.

## 1. Build and initialize

```bash
cargo build -p firm-cli
./target/debug/firm init
./target/debug/firm space list
./target/debug/firm project list
```

An Execution Space selects coordination storage. A Project Binding separately
selects provider cwd, instructions, Skills, Git/worktree, and permission
boundaries. Select them explicitly with `--space` / `HARNESS_SPACE` and
`--project` / `HARNESS_PROJECT`, or the corresponding `firm space switch` and
`firm project switch` commands.

## 2. Start the service and Dashboard

```bash
./target/debug/firm serve --addr 127.0.0.1:8787
pnpm install
pnpm dashboard:dev
```

Use the same Execution Space for CLI, service, and Dashboard coordination.
Project selection may differ per TeamRun when the Host deliberately chooses a
different binding. One machine-scoped NodeDaemon and the current Supervisor
generation own provider effects and live controls; other surfaces route through
that authority and fail explicitly when it is unavailable.

## 3. Create and operate an Agent Team

Create the Host AgentMember through the canonical member-trust operation, then
create the Node and flat AgentTeam:

```bash
./target/debug/firm member-trust mutate \
  --actor-kind human --actor-id operator \
  --idempotency-key create-builder --expected-version 0 \
  --json '<create_agent_member payload>'
./target/debug/firm node init
./target/debug/firm team create \
  --name builders --description "Persistent builders" \
  --host-agent-id builder-codex --node-id <node-uuid> --member builder-codex
```

Use Work for durable responsibility and identity-first Messages for
conversation. New Team members use only persistent bidirectional modes:
`codex_app_server`, `claude_agent_sdk`, or `kimi_acp`. Interrupt stops one turn;
Close ends the current runtime generation; Reopen resumes the same verified
provider-native session with a new generation; Retire is permanent. TeamRun
completion never implies Close or Host acceptance.

## Bounded local orchestration

A Host or provider may use plans, loops, or native subagents internally while
performing assigned Work. Those are implementation details: they do not create
a second task ledger, WorkflowRun, Harness transcript, or acceptance state.

## Historical Dynamic Workflow data

Historical Workflow records are not current coordination state. Access them
only through the lossless legacy archive export, verification, and restore-read
path. No current CLI, HTTP, MCP, Dashboard, plugin, migration, test helper, or
background service may write, resume, or project them as live state. Retirement
does not assert that historical rows were deleted or that storage owners and
administrators cannot edit underlying storage.
