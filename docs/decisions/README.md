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
| [0014](0014-react-vite-agent-dashboard.md) | React/Vite Agent Dashboard frontend |
| [0015](0015-autonomous-proposals-use-evidence-message-decision.md) | Autonomous proposals use Evidence, Message, and Decision first |
| [0016](0016-tailwind-shadcn-adoption.md) | Tailwind v4 + shadcn/ui adoption for Agent Workbench |
| [0017](0017-generic-object-model.md) | Generic object model — additive-optional schema evolution |
| [0018](0018-exec-stream-primary-substrate.md) | Headless exec-stream as primary provider substrate |
| [0019](0019-vision-goal-task-workbench-redesign.md) | Vision → Goal → Task Workbench redesign (board + detail + derived graph) |
| [0020](0020-codex-persistent-service-exploration.md) | Codex persistent-service exploration — keep the respawn model (Claude resident is separate) |
| [0021](0021-resident-daemon.md) | Resident-daemon warm-child host (amends 0018) — internal Unix-socket host keeps exec-stream children warm across deliveries |
| [0022](0022-dynamic-workflow-runtime-json-ir.md) | Dynamic Workflow Runtime — skill + CLI entry, JSON-IR spec (not embedded JS), new `harness-workflow` crate |
| [0023](0023-starlark-workflow-frontend.md) | Starlark program front-end — third authoring surface (loops/conditionals/data-driven fan-out) via a hermetic interpreter; reuses the 0022 backend |
| [0024](0024-goal-phase-execution-modes.md) | GoalPhase execution modes — each phase chooses task_graph or workflow, never both |

## Split Rule

Add a new ADR when a decision changes object relationships, source of truth,
provider boundaries, task/review flow, Dashboard control-plane responsibility,
or a hard-to-reverse contract.
