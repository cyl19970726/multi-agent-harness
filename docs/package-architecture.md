# Package Architecture

The backend should be Rust-first. TypeScript should be used for the Agent
Dashboard and lightweight documentation tooling, not the core runtime.

## Rust Crates

```text
crates/
  harness-core      # AgentMember, Task, Message, Evidence, Decision
  harness-task      # task list, assignment, status transitions
  harness-store     # append-only file store, later SQLite/Postgres
  harness-adapter   # provider and project tool adapter traits
  harness-api       # HTTP/WebSocket API
  harness-cli       # agent-harness CLI
```

First version focuses on:

```text
Task -> Message -> Evidence -> Decision
```

## Frontend

```text
apps/
  agent-dashboard
```

The dashboard reads structured objects from the API. It should not implement
project-specific business views; those come from adapters as evidence links.

## Dependency Direction

```text
harness-cli -> harness-store -> harness-core
harness-api -> harness-store -> harness-core
harness-api -> harness-adapter -> harness-core
agent-dashboard -> harness-api
project adapter -> harness-adapter
harness-core -> no project dependencies
```

## First Implementation Slice

1. `harness-core`: minimal types.
2. `harness-store`: append-only file-backed task/message/evidence store.
3. `harness-cli`: create/list task, send/list message, attach evidence.
4. `harness-adapter`: tool descriptor and invocation trait.
5. `harness-api`: read model for dashboard.
