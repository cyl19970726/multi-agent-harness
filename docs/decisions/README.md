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
| [0022](0022-dynamic-workflow-runtime-json-ir.md) | superseded | Historical Dynamic Workflow runtime decision; retired from current execution |
| [0023](0023-starlark-workflow-frontend.md) | superseded | Historical Starlark authoring decision; retired from current execution |
| [0025](0025-agent-team-run-control-plane.md) | partially superseded | Agent Team runtime substrate remains; Work graph clauses are amended by 0058 |
| [0026](0026-mission-wave-architecture.md) | superseded by DOC-108 | Historical Mission/Wave foundation; the whole Mission/Wave model is retired legacy history |
| [0027](0027-company-os-primary-model.md) | superseded by DOC-108 | The Company OS primary model is retired; the repository is the execution foundation |
| [0028](0028-retire-goal-phase-task-graph.md) | active | Retire the superseded coordination stack |
| [0029](0029-agent-programmable-document-runtime.md) | superseded by DOC-108 | Built-in Docs runtime retired with the Company OS layer |
| [0030](0030-provider-interaction-contract.md) | superseded by 0056 | Historical provider-interaction object and permission-routing contract |
| [0031](0031-interactive-provider-modes-and-version-drift.md) | active | Chat/steer/interrupt semantics and adapter version review gates |
| [0032](0032-provider-native-session-is-execution-truth.md) | active, implemented | Provider-native session owns transcript/tool activity/resume; Harness owns coordination, outcomes, refs and gates |
| [0033](0033-agent-team-workspace-contract.md) | active, implemented; graph clause amended by 0058 | Agent Team store, project, run execution, and member worktree roots are distinct and observable |
| [0034](0034-host-plan-waves-and-mission-teams.md) | superseded by DOC-108 | Historical Host-plan Wave/Mission-Team model; Teams are durable and Mission-free |
| [0035](0035-company-os-sql-read-model.md) | superseded by DOC-108 | The Company OS read-model plan retired with its layer |
| [0036](0036-agent-operated-docs-and-code-declared-pages.md) | superseded by DOC-108 | Built-in agent-operated Docs retired with the Company OS layer |
| [0037](0037-agent-member-autonomy-and-collaboration.md) | active; dependency clauses amended by 0058 | Members own end-to-end Work; Message is conversation; subagents remain member-internal |
| [0038](0038-provider-native-member-plan-negotiation.md) | superseded | Historical provider-native Plan Gate, replaced by ordinary correlated planning |
| [0039](0039-ordinary-member-planning-and-durable-mailbox-delivery.md) | active; dependency clauses amended by 0058 | Planning is ordinary Host/Member conversation; Work edges own execution ordering |
| [0040](0040-native-host-inbox-delivery.md) | active | Host mail is scoped to an exact native task; Codex busy delivery uses a one-shot Stop continuation and unowned idle tasks remain safe-boundary pull |
| [0041](0041-provider-neutral-member-continuation.md) | active; responsibility ref amended by 0050; `provider_driven` superseded by 0067 | Continuation separates durable Work from the provider-native execution driver and one top-level Workspace lease |
| [0042](0042-company-store-execution-space-project-binding.md) | partially superseded by DOC-108 | Execution Space vs Project Binding separation remains current; the legacy Company Store identity is retired |
| [0044](0044-durable-team-supervision-and-typed-mail.md) | active; graph clause amended by 0058; lease-scope clause amended in the core-5 review of 2026-09-15 | One durable Supervisor lease owns Provider control; Work DAG remains kernel-owned; expiry-replacement applies to the TeamSupervisorLease only, never to the NodeDaemonLease |
| [0045](0045-company-owned-standing-agent-execution-relation.md) | superseded by DOC-108 | Company-owned execution relation retired; AgentMember/TeamMembership is the only identity authority |
| [0046](0046-supervised-agentos-self-hosting-loop.md) | partially superseded by DOC-108 and the Agent Firm Mental Model | Supervising Operator and Runtime Supervisor boundaries remain current; the legacy Company OS premise and the separate StandingAgent target are retired |
| [0047](0047-scoped-company-authority-broker.md) | superseded by DOC-108 | The Company authority broker plan retired with the Company layer |
| [0048](0048-human-rooted-company-constitution.md) | superseded by DOC-108 | The Company constitution plan retired with the Company layer |
| [0049](0049-member-coordination-and-runtime-lifecycle.md) | active, implemented | Member coordination and disposable adapter runtime have separate Close, Reopen, and Retire semantics |
| [0050](0050-agent-team-work-board-and-message-boundary.md) | accepted; dependency and child clauses superseded by 0058 | Work is the responsibility primitive; Message and runtime planes remain separate |
| [0051](0051-single-intent-spine.md) | superseded by DOC-108 | The Mission/Mission Log spine is retired; Work + Messages replaced it |
| [0052](0052-nested-agent-teams-are-the-agent-organization.md) | superseded by [mental model](../mental/agent-firm-mental-model.md) | Proposed a recursive AgentTeam topology; that proposal is superseded by flat Agent Teams (no nesting). See the Agent Firm Mental Model. |
| [0053](0053-finance-contract-layer-retirement.md) | accepted; staged retirement | Finance contract layer retired; Commitment/Payment code remains dormant until decommission |
| [0054](0054-ai-first-docs-page-model-and-storage.md) | superseded by DOC-108 | The AI-first Docs page model retired with the built-in Docs layer |
| [0055](0055-remote-node-fabric.md) | accepted; implemented | One Fabric Control Plane, outbound NodeGateway children, and FabricStore as the sole cross-Node route truth |
| [0056](0056-correlated-message-and-session-permission-cutover.md) | accepted; implemented | Provider questions are correlated Messages; permission is frozen at AgentSession start; the second interaction object is removed |
| [0057](0057-host-is-an-agent-member.md) | accepted; implementation tracked by DEV-59 | Managed and external interactive Hosts share AgentMember identity and role authority; managed Hosts use the ordinary Team runtime and exact-session inbox path |
| [0058](0058-work-dependency-dag-and-kernel-boundary.md) | accepted; DEV-60 cutover | Flat Work dependency DAG, kernel/package boundary, constrained Member proposals, and closed Module scope |
| [0059](0059-trusted-development-full-access-and-explicit-cwd.md) | accepted; DEV-65 implementation | Managed coding Hosts/Members use frozen FullAccess and exact cwd; shared cwd is allowed, worktrees are optional, and agent coordination is CLI-only |
| [0060](0060-source-lifecycle-projection-folds.md) | accepted; DEV-74 implementation | Canonical WorkDelivery revisions and HostAttention source/lifecycle rows fold deterministically and fail closed on identity, version, or transition conflicts |
| [0061](0061-retire-harness-coordination-mcp.md) | accepted; DEV-100 implementation | `firm` CLI is the sole Agent-facing Harness coordination transport; the duplicate Harness MCP server is deleted without migration |
| [0062](0062-host-owned-work-peer-acceptance.md) | accepted; DEV-102 implementation | Member Work remains Host-reviewed; Host-owned Work requires one exact active non-owner peer in the same TeamRun, without a Reviewer ledger |
| [0063](0063-retire-plugin-package-and-in-repo-retirement-evidence.md) | accepted; 2026-09-05 cleanup | The plugin package, the in-repo Dynamic Workflow retirement register/archive, archived operator skills, superseded specs, and the skill evaluation workspace live only in git history (last tree `918e9002`); one retired-path gate replaces four path-policing gates and the installer publishes the binary only |
| [0064](0064-host-attention-is-a-delivery-ledger.md) | accepted; Owner 2026-09-05 (SPEC-ADAPTATION-REFACTOR-01 D-A) | Three authority planes stay; HostAttention is the Host-notification delivery ledger whose ACK is the Host-intake precondition of exactly one Work verb (retarget) and whose review-requested rows are terminal-Work provenance |
| [0065](0065-two-runtime-epochs.md) | accepted; Owner 2026-09-05 (SPEC-ADAPTATION-REFACTOR-01 D-B) | `MemberRun.runtime_generation` is the adapter-process epoch and fence authority; `AgentSession.runtime_generation` is the immutable provider-session epoch; deliberately independent, related only for the minting MemberRun; no schema change |
| [0066](0066-retire-the-work-ledger-delegation-stack.md) | accepted; W5a Work ledger slice | The Work-ledger WorkDelegation relation, its ledgers, gate and readers are retired; the Work model carries no cross-Team edge and the Remote Fabric's `WorkDelegationV1` is untouched |
| [0067](0067-retire-the-native-continuation-control-plane.md) | accepted; 2026-09-14 C1 cutover | The NativeContinuation control plane and the `provider_driven` driver are retired with zero persisted usage; the continuation activation projection and the quiesce step are retained as the drained-lane proof, and the three RuntimeCommand kinds are frozen rather than deleted |
| [0068](0068-retire-the-inject-and-interrupt-delivery-policies.md) | accepted; 2026-09-14 C2 cutover | `TeamDeliveryPolicy` keeps only `queue` and `manual_ack`; the mid-cycle Steer/inject path is retired end to end (intents, capabilities, adapter impls, HTTP route, delivery mode) and the two RuntimeCommand kinds are frozen; interrupt plus next-cycle mail are the two ways to change a member's course |
| [0069](0069-retire-the-agent-identity-projection.md) | accepted; 2026-09-14 B1 cleanup | The AgentIdentity projection is retired on every surface (type, Store projections, schema, RoleView field, Dashboard types) with the StartSession permission-ceiling fence rewritten onto AgentMember; the unreadable SubscriptionCursor and the writer-less MessageRouteJournal are deleted; five serde aliases and two persisted string formats are frozen so pre-cutover rows stay readable and replay-identical |
| [0070](0070-retire-waiting-session-status-and-rename-the-cycle-marker.md) | accepted; 2026-09-15 N1 cutover | `AgentSessionStatus::Waiting` is deleted with zero persisted usage (0 of 2,162 real `agent_session` projections), so an unknown value fails decoding rather than becoming a drivable lane; `current_turn_id` is renamed `current_cycle_marker` with a frozen serde alias because it is a Harness-synthesized open-cycle marker that never carried a provider turn id |
| [0071](0071-two-closes-team-close-quiesces-provider-stop-closes.md) | accepted; 2026-09-15 CORE4-N3 | Two operations are called Close: Team `close-member` ends one MemberRun generation and quiesces the machine-owned AgentSession to `idle`, while only a settled `StopSession` RuntimeCommand writes the terminal `closed`; a Team Host cannot issue that command because AgentSession authority is exact self or the exact machine NodeDaemon/Operator, and continuity across a provider Close is the native session id carried onto a newly minted AgentSession row |
| [0072](0072-one-authority-for-the-provider-native-session-pointer.md) | accepted; 2026-09-15 N2a slice (authority projection in the N2b follow-up) | `AgentSession.native_session_ref` is the one authority for the provider-native session pointer; `MemberRun.native_session` and the `member_runs.jsonl` copy are projections with pre-session `requested` semantics; one `NativeSessionRef` type, one `same_identity_as` plus one named asymmetric resume-seed admission, and one locator-kind table shared by every adapter and seeding path |
| [0073](0073-automatic-predecessor-recovery-and-settlement-records.md) | accepted; 2026-09-15 CORE5-E1a | A successor NodeDaemon recovers an unreleased predecessor automatically when the CLI's own proofs all hold, plus recycled-pid detection from the process start time; an alive, ambiguous or duplicated predecessor stays operator-gated and `daemon status` names the failing proof; every authority-loss path writes the self-stop phases and a per-session `settlement_incomplete` record that claims no settlement, which recovery reads, settles and clears |
| [0074](0074-authority-loss-issues-one-cooperative-interrupt.md) | accepted; 2026-09-15 CORE5-E1B | Losing machine authority or a TeamRun Supervisor lease hands every live provider turn in scope exactly one cooperative interrupt through the adapter's existing `interrupt_current_cycle` path — on the machine path before the drain's bounded cooperative wait and its SIGKILL, and on the Supervisor-lease path there is no drain, so that interrupt is what ends the turn; each latch first invalidates its own scope process-locally so a turn registering after the fan-out refuses instead of driving; the interrupt is deliberately a process-local action and not a RuntimeCommand, because nothing may be admitted under lost authority, and the interrupted terminal still hits the authority refusal so the admitted StartCycle settles nothing |
| [0075](0075-machine-lease-leaves-the-space-data-lock.md) | accepted; Owner 2026-09-15 (CORE5-E2) | Machine authority leaves the Execution Space data lock: one `<FIRM_HOME>/nodes/<node_id>/node-daemon-lease.json` per machine, written by atomic replace under its own leaf, non-nesting lease lock, renewed once per machine with a machine-wide monotonic generation; one resolver answers every fence and only the node file authorizes a provider effect, while the per-Space `node_daemon_leases.jsonl` rows stay readable legacy |

## Split Rule

Add a new ADR when a decision changes object relationships, source of truth,
provider boundaries, task/review flow, Dashboard control-plane responsibility,
or a hard-to-reverse contract.
