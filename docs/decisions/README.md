# Architecture Decisions

This directory records durable architecture decisions that future agents should
not casually re-litigate. Each ADR should name the context, decision,
consequences, affected modules, and validation path.

## Index

| ADR | Decision |
| --- | --- |
| [0001](0001-rust-backend.md) | Rust backend |
| [0002](0002-message-first-task-system.md) | Message-first task system |
| [0003](0003-minimal-first-types.md) | Minimal first types |
| [0004](0004-file-store-before-database.md) | File store before database |
| [0005](0005-self-hosting-first.md) | Self-hosting first |
| [0006](0006-task-graph-before-workflow-dsl.md) | Task graph before workflow DSL |
| [0007](0007-kanban-dashboard-first.md) | Kanban Dashboard first |
| [0008](0008-persistent-codex-agent-runtime.md) | Persistent Codex Agent runtime |
| [0009](0009-task-graph-as-derived-view.md) | Task graph as derived view |
| [0010](0010-harness-store-is-canonical.md) | Harness store is canonical |
| [0011](0011-provider-neutral-runtime.md) | Provider-neutral runtime before provider implementations |
| [0012](0012-dashboard-is-control-plane.md) | Dashboard is control plane |
| [0013](0013-pr-merge-is-not-harness-acceptance.md) | PR merge is not harness acceptance |

## Split Rule

Add a new ADR when a decision changes object relationships, source of truth,
provider boundaries, task/review flow, Dashboard control-plane responsibility,
or a hard-to-reverse contract.
