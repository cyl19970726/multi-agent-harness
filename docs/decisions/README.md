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
| [0018](0018-exec-stream-primary-substrate.md) | superseded for Agent Team | Historical exec-stream decision; retained for bounded Workflow context |
| [0020](0020-codex-persistent-service-exploration.md) | active evidence | Codex persistent-service exploration; retain respawn model |
| [0021](0021-resident-daemon.md) | historical | Former resident CLI warm-child host; not the Agent Team lifecycle |
| [0022](0022-dynamic-workflow-runtime-json-ir.md) | partially superseded | Dynamic Workflow runtime; authoring details refined by 0023 |
| [0023](0023-starlark-workflow-frontend.md) | partially superseded | Hermetic Starlark authoring and later convergence notes |
| [0025](0025-agent-team-run-control-plane.md) | partially superseded | Agent Team runtime substrate remains; Wave attempt ownership is superseded by 0034 |
| [0026](0026-mission-wave-architecture.md) | partially superseded | Mission/Wave names and transient-thinking policy remain; Wave-as-executor hierarchy is superseded by 0034 |
| [0027](0027-company-os-primary-model.md) | amended by 0051 | Docs + mixed Organization product cores remain; separate agent scheduling hierarchy is superseded |
| [0028](0028-retire-goal-phase-task-graph.md) | active | Retire the superseded coordination stack |
| [0029](0029-agent-programmable-document-runtime.md) | active, staged | Basic docs, structured views and governed custom pages |
| [0030](0030-provider-interaction-contract.md) | active | Execution-mode profiles, durable PendingInteraction routing, and provider-versus-semantic truth |
| [0031](0031-interactive-provider-modes-and-version-drift.md) | active | Chat/steer/interrupt semantics and adapter version review gates |
| [0032](0032-provider-native-session-is-execution-truth.md) | active, implemented | Provider-native session owns transcript/tool activity/resume; Harness owns coordination, outcomes, refs and gates |
| [0033](0033-agent-team-workspace-contract.md) | active, implemented | Agent Team store, project, run execution, and member worktree roots are distinct and observable |
| [0034](0034-host-plan-waves-and-mission-teams.md) | active | Wave is the Host's versioned operational memo; Missions link independent long-lived Agent Teams |
| [0035](0035-company-os-sql-read-model.md) | active | SQL is a derived Company OS read/query/index layer, not the current canonical Store |
| [0036](0036-agent-operated-docs-and-code-declared-pages.md) | active | Docs is an Agent-operated substrate with code-declared custom business pages |
| [0037](0037-agent-member-autonomy-and-collaboration.md) | active; assignment ownership amended by 0050 | Members own end-to-end Work; TeamMessage is conversation; subagents remain member-internal |
| [0038](0038-provider-native-member-plan-negotiation.md) | superseded | Historical provider-native Plan Gate, replaced by ordinary correlated planning |
| [0039](0039-ordinary-member-planning-and-durable-mailbox-delivery.md) | active; kinds amended by 0050 | Planning is ordinary Host/Member conversation; Harness owns durable authored-message delivery |
| [0040](0040-native-host-inbox-delivery.md) | active | Host mail is scoped to an exact native task; Codex busy delivery uses a one-shot Stop continuation and unowned idle tasks remain safe-boundary pull |
| [0041](0041-provider-neutral-member-continuation.md) | active; responsibility ref amended by 0050 | Continuation separates durable Work from the provider-native execution driver and one top-level Workspace lease |
| [0042](0042-company-store-execution-space-project-binding.md) | active | Company Store, Execution Space, and Project Binding are distinct identities |
| [0044](0044-durable-team-supervision-and-typed-mail.md) | active | One durable Supervisor lease owns Provider control; typed mail and atomic delivery claims make multi-client coordination safe |
| [0045](0045-company-owned-standing-agent-execution-relation.md) | historical implementation; target superseded by 0051 | Current StandingAgent-to-AgentMember compatibility join; not the target identity model |
| [0046](0046-supervised-agentos-self-hosting-loop.md) | amended by 0051 | Supervising Operator and Runtime Supervisor boundaries remain; separate StandingAgent target is superseded |
| [0047](0047-scoped-company-authority-broker.md) | scoped target; implementation pending | Sensitive Company effects only; routine Team scheduling follows 0051 topology |
| [0048](0048-human-rooted-company-constitution.md) | scoped target; implementation pending | Sensitive or root Company effects only; routine Team scheduling follows 0051 topology |
| [0049](0049-member-coordination-and-runtime-lifecycle.md) | active, implemented | Member coordination and disposable adapter runtime have separate Close, Reopen, and Retire semantics |
| [0050](0050-agent-team-work-board-and-message-boundary.md) | amended by 0051; implementation pending | Work is the scheduling primitive; permanent Company WorkItem separation is superseded |
| [0051](0051-nested-agent-teams-are-the-agent-organization.md) | accepted target; implementation pending | AgentMember is the durable agent identity; Organization is recursive AgentTeam topology over one Work kernel |
| [0051](0051-single-intent-spine.md) | accepted; staged breaking cutover | Mission absorbs Wave as an append-only Mission Log; provider-native goal/plan are derived drive cache, never truth |

## Split Rule

Add a new ADR when a decision changes object relationships, source of truth,
provider boundaries, task/review flow, Dashboard control-plane responsibility,
or a hard-to-reverse contract.
