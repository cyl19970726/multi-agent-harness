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
| [0012](0012-dashboard-is-control-plane.md) | scoped | Dashboard is the execution operator control plane; it was never the truth owner for the retired Company OS layer |
| [0013](0013-pr-merge-is-not-harness-acceptance.md) | active | PR merge is not Harness acceptance |
| [0014](0014-react-vite-agent-dashboard.md) | scoped | React/Vite frontend platform; earlier product IA is superseded |
| [0016](0016-tailwind-shadcn-adoption.md) | active | Tailwind v4 + shadcn/ui adoption |
| [0018](0018-exec-stream-primary-substrate.md) | superseded for Agent Team | Historical exec-stream decision; retained for bounded Workflow context |
| [0020](0020-codex-persistent-service-exploration.md) | active evidence | Codex persistent-service exploration; retain respawn model |
| [0021](0021-resident-daemon.md) | historical | Former resident CLI warm-child host; not the Agent Team lifecycle |
| [0022](0022-dynamic-workflow-runtime-json-ir.md) | partially superseded | Dynamic Workflow runtime; authoring details refined by 0023 |
| [0023](0023-starlark-workflow-frontend.md) | partially superseded | Hermetic Starlark authoring and later convergence notes |
| [0025](0025-agent-team-run-control-plane.md) | partially superseded | Agent Team runtime substrate remains; Wave attempt ownership is superseded by 0034 |
| [0026](0026-mission-wave-architecture.md) | superseded by DOC-108 | Historical Mission/Wave foundation; the whole Mission/Wave model is retired legacy history |
| [0027](0027-company-os-primary-model.md) | superseded by DOC-108 | The Company OS primary model is retired; the repository is the execution foundation |
| [0028](0028-retire-goal-phase-task-graph.md) | active | Retire the superseded coordination stack |
| [0029](0029-agent-programmable-document-runtime.md) | superseded by DOC-108 | Built-in Docs runtime retired with the Company OS layer |
| [0030](0030-provider-interaction-contract.md) | superseded by 0056 | Historical provider-interaction object and permission-routing contract |
| [0031](0031-interactive-provider-modes-and-version-drift.md) | active | Chat/steer/interrupt semantics and adapter version review gates |
| [0032](0032-provider-native-session-is-execution-truth.md) | active, implemented | Provider-native session owns transcript/tool activity/resume; Harness owns coordination, outcomes, refs and gates |
| [0033](0033-agent-team-workspace-contract.md) | active, implemented | Agent Team store, project, run execution, and member worktree roots are distinct and observable |
| [0034](0034-host-plan-waves-and-mission-teams.md) | superseded by DOC-108 | Historical Host-plan Wave/Mission-Team model; Teams are durable and Mission-free |
| [0035](0035-company-os-sql-read-model.md) | superseded by DOC-108 | The Company OS read-model plan retired with its layer |
| [0036](0036-agent-operated-docs-and-code-declared-pages.md) | superseded by DOC-108 | Built-in agent-operated Docs retired with the Company OS layer |
| [0037](0037-agent-member-autonomy-and-collaboration.md) | active; assignment ownership amended by 0050 | Members own end-to-end Work; TeamMessage is conversation; subagents remain member-internal |
| [0038](0038-provider-native-member-plan-negotiation.md) | superseded | Historical provider-native Plan Gate, replaced by ordinary correlated planning |
| [0039](0039-ordinary-member-planning-and-durable-mailbox-delivery.md) | active; kinds amended by 0050 | Planning is ordinary Host/Member conversation; Harness owns durable authored-message delivery |
| [0040](0040-native-host-inbox-delivery.md) | active | Host mail is scoped to an exact native task; Codex busy delivery uses a one-shot Stop continuation and unowned idle tasks remain safe-boundary pull |
| [0041](0041-provider-neutral-member-continuation.md) | active; responsibility ref amended by 0050 | Continuation separates durable Work from the provider-native execution driver and one top-level Workspace lease |
| [0042](0042-company-store-execution-space-project-binding.md) | partially superseded by DOC-108 | Execution Space vs Project Binding separation remains current; the legacy Company Store identity is retired |
| [0044](0044-durable-team-supervision-and-typed-mail.md) | active | One durable Supervisor lease owns Provider control; typed mail and atomic delivery claims make multi-client coordination safe |
| [0045](0045-company-owned-standing-agent-execution-relation.md) | superseded by DOC-108 | Company-owned execution relation retired; AgentMember/TeamMembership is the only identity authority |
| [0046](0046-supervised-agentos-self-hosting-loop.md) | partially superseded by DOC-108 and the Agent Firm Mental Model | Supervising Operator and Runtime Supervisor boundaries remain current; the legacy Company OS premise and the separate StandingAgent target are retired |
| [0047](0047-scoped-company-authority-broker.md) | superseded by DOC-108 | The Company authority broker plan retired with the Company layer |
| [0048](0048-human-rooted-company-constitution.md) | superseded by DOC-108 | The Company constitution plan retired with the Company layer |
| [0049](0049-member-coordination-and-runtime-lifecycle.md) | active, implemented | Member coordination and disposable adapter runtime have separate Close, Reopen, and Retire semantics |
| [0050](0050-agent-team-work-board-and-message-boundary.md) | accepted; flat-Team amendment implemented; historical Mission boundary retired (DOC-108) | Work is the scheduling primitive; the retired Company WorkItem separation stays superseded |
| [0051](0051-single-intent-spine.md) | superseded by DOC-108 | The Mission/Mission Log spine is retired; Work + Messages replaced it |
| [0052](0052-nested-agent-teams-are-the-agent-organization.md) | superseded by [mental model](../mental/agent-firm-mental-model.md) | Proposed a recursive AgentTeam topology; that proposal is superseded by flat Agent Teams (no nesting). See the Agent Firm Mental Model. |
| [0053](0053-finance-contract-layer-retirement.md) | accepted; staged retirement | Finance contract layer retired; Commitment/Payment code remains dormant until decommission |
| [0054](0054-ai-first-docs-page-model-and-storage.md) | superseded by DOC-108 | The AI-first Docs page model retired with the built-in Docs layer |
| [0055](0055-remote-node-fabric.md) | accepted; implemented | One Fabric Control Plane, outbound NodeGateway children, and FabricStore as the sole cross-Node route truth |
| [0056](0056-correlated-message-and-session-permission-cutover.md) | accepted; implemented | Provider questions are correlated Messages; permission is frozen at AgentSession start; the second interaction object is removed |

## Split Rule

Add a new ADR when a decision changes object relationships, source of truth,
provider boundaries, task/review flow, Dashboard control-plane responsibility,
or a hard-to-reverse contract.
