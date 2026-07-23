# Architecture Decisions

This directory records durable architecture decisions that future agents should
not casually re-litigate. Each ADR should name the context, decision,
consequences, affected modules, and validation path.

## Index

| ADR | State | Decision |
| --- | --- | --- |
| [0001](0001-rust-backend.md) | active | Rust backend |
| [0004](0004-file-store-before-database.md) | active | File store before database |
| [0005](0005-self-hosting-first.md) | active | Self-hosting first |
| [0008](0008-persistent-codex-agent-runtime.md) | amended | Persistent Codex Agent runtime; provider lifecycle refined by 0018, 0020 and 0021 |
| [0010](0010-harness-store-is-canonical.md) | active | Harness store is canonical for execution records |
| [0011](0011-provider-neutral-runtime.md) | active | Provider-neutral runtime before provider implementations |
| [0012](0012-dashboard-is-control-plane.md) | scoped | Dashboard is the execution operator control plane, not the Company OS truth owner |
| [0013](0013-pr-merge-is-not-harness-acceptance.md) | active | PR merge is not Harness acceptance |
| [0014](0014-react-vite-agent-dashboard.md) | scoped | React/Vite frontend platform; earlier product IA is superseded |
| [0016](0016-tailwind-shadcn-adoption.md) | active | Tailwind v4 + shadcn/ui adoption |
| [0018](0018-exec-stream-primary-substrate.md) | amended | Headless exec-stream substrate, amended by resident-host support |
| [0020](0020-codex-persistent-service-exploration.md) | active evidence | Codex persistent-service exploration; retain respawn model |
| [0021](0021-resident-daemon.md) | active | Resident-daemon warm-child host |
| [0022](0022-dynamic-workflow-runtime-json-ir.md) | partially superseded | Dynamic Workflow runtime; authoring details refined by 0023 |
| [0023](0023-starlark-workflow-frontend.md) | partially superseded | Hermetic Starlark authoring and later convergence notes |
| [0025](0025-agent-team-run-control-plane.md) | partially superseded | Agent Team runtime substrate remains; Wave attempt ownership is superseded by 0034 |
| [0026](0026-mission-wave-architecture.md) | partially superseded | Mission/Wave names and transient-thinking policy remain; Wave-as-executor hierarchy is superseded by 0034 |
| [0027](0027-company-os-primary-model.md) | active | Docs + mixed Organization product cores and WorkItem/Approval bridge |
| [0028](0028-retire-goal-phase-task-graph.md) | active | Retire the superseded coordination stack |
| [0029](0029-agent-programmable-document-runtime.md) | active, staged | Basic docs, structured views and governed custom pages |
| [0030](0030-provider-interaction-contract.md) | active | Execution-mode profiles, durable PendingInteraction routing, and provider-versus-semantic truth |
| [0031](0031-interactive-provider-modes-and-version-drift.md) | active | Chat/steer/interrupt semantics and adapter version review gates |
| [0032](0032-provider-native-session-is-execution-truth.md) | active, implemented | Provider-native session owns transcript/tool activity/resume; Harness owns coordination, outcomes, refs and gates |
| [0033](0033-agent-team-workspace-contract.md) | active, implemented | Agent Team store, project, run execution, and member worktree roots are distinct and observable |
| [0034](0034-host-plan-waves-and-mission-teams.md) | active | Wave is the Host's versioned operational memo; Missions link independent long-lived Agent Teams |

## Split Rule

Add a new ADR when a decision changes object relationships, source of truth,
provider boundaries, task/review flow, Dashboard control-plane responsibility,
or a hard-to-reverse contract.
