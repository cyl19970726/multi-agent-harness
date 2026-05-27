# Decisions

This file records early product decisions. Split into ADR files only when this
file becomes too long or a decision needs separate review.

## 0001: Rust Backend

Use Rust for the backend because the core is an event system, state machine, and
audit ledger. Rust is a good fit for append-only storage, concurrent agent
writes, permission gates, and typed lifecycle transitions.

## 0002: Message-First Task System

Task assignment and task reports should flow through `Message`. A message can
later materialize into `Task`, `Evidence`, or `Decision`.

## 0003: Minimal First Types

The first version should center on:

```text
Goal
AgentMember
Task
Message
Evidence
Decision
ProviderSession
```

Other concepts such as Skill, ToolAdapter, and Dashboard can start as
configuration or views until the core loop works.

## 0004: File Store Before Database

Start with append-only file-backed storage. Move to SQLite/Postgres after the
object model and query patterns are stable.

## 0005: Self-Hosting First

The first MVP pilot is this repository managing its own development. Earning
Engine is the first project adapter pilot, but it should follow the harness's
self-hosting proof rather than become the only source of product requirements.

## 0006: Task Graph Before Workflow DSL

Use a simple task DAG with dependencies, parent tasks, workspace refs, branch
refs, PR refs, owned paths, reviewers, messages, evidence, and decisions before
introducing a larger workflow DSL. Parallel development is expressed as
separate tasks with separate worktrees and branches, then integrated through PRs
or equivalent review artifacts.

## 0007: Kanban Dashboard First

The first Agent Dashboard should be a Kanban-style operating view over goals,
tasks, messages, evidence, blockers, workspaces, reviewers, and decisions. It
links to project dashboards for domain charts instead of replacing them.

## 0008: Persistent Codex Agent Runtime

The first provider integration is Codex, and the target MVP runtime is
persistent Agent Members backed by `codex app-server`, not only one-shot
`codex exec`.

Use one Codex app-server process per Agent Member in V1. Each member gets its
own prompt, worktree, provider thread, runtime state, and event stream.

`codex exec` and `codex review` remain fallback paths for one-shot work, CI
smoke tests, and PR review. They are not the primary source of persistent agent
state.

Skills teach Codex how to operate in this workflow. App-server notifications
and hooks feed `AgentEvent`, `Proposal`, `Evidence`, messages, and Dashboard
updates. Plugins are deferred until CLI/API/schema contracts are stable and
should package skills/hooks/MCP helpers rather than replace the runtime.

The integration boundary is in [integration/codex.md](integration/codex.md);
the runtime details are in [codex-agent-runtime.md](codex-agent-runtime.md).
