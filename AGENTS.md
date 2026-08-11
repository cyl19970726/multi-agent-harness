# Agent Operating Rules

This repository builds Star Harness itself. Product truth lives in canonical
docs, schemas, ADRs, and implemented stores. Execution claims must additionally
be reconstructable from the native runtime records of the executor used.

This root file states product identity, hard invariants, repository execution
rules, routing links, and proportional acceptance. The full operating detail
lives in [docs/current/product/agent-operating-rules.md](docs/current/product/agent-operating-rules.md);
canonical contracts live in the docs linked under [Routing](#routing). Where
this file and a canonical doc conflict, the canonical doc wins — fix this file.

## AgentFirm CompanyOS Authority

AgentFirm's current product mental model and development control plane live in
Notion. Repository files remain the versioned implementation truth for code,
tests, CI, executable contracts, and shipped behavior.

- Start at [AgentFirm Home](https://app.notion.com/p/3b849a4fa3798115939cca2b0b9e6f2d)
  and verify its current authority notice before any Notion mutation.
- Use the [Development System](https://app.notion.com/p/21e49a4fa37982a5b9f781cf04584034)
  for Work, immutable Delivery Runs, and typed Development Documents.
- Follow the [Development Playbook](https://app.notion.com/p/3b849a4fa37981a990a5cf0059dcfa4a)
  for claim, execution, exact-candidate review, merge, and closeout.
- Never claim or update work in the Notion area labeled Legacy Production /
  READ ONLY / DO NOT CLAIM. Page location and relations never grant authority;
  authority follows the current AgentFirm Home notice.

Notion owns current product intent and operating state. The repository owns
what is actually implemented. When they diverge, record the gap in the
Implementation Crosswalk and Development Work rather than silently rewriting
either side.

## Product Identity

Star Harness is an AI Company OS with two primary systems: a Notion-like Docs
system for company memory and operating structure, and a mixed Organization of
humans, durable AgentMembers arranged in flat AgentTeams, external
collaborators, and services. Documents create or relate Work and Approvals;
accountable actors execute them; results, evidence, metrics, and financial
effects return to the originating records. See
[docs/current/product/prd.md](docs/current/product/prd.md) and
[docs/current/company-os/README.md](docs/current/company-os/README.md).

Mission/Wave, Agent Team, Dynamic Workflow, Host execution, providers, plugins,
and MCP are the shared execution foundation. Their native relations are:

```text
Mission -> ordered Host-plan Wave
Mission <-> exactly one flat AgentTeam
AgentTeam -> immutable node_id -> one machine-scoped NodeDaemon
AgentTeamRun -> MemberRun -> provider-native session
```

`Mission` is durable intent; `Wave` is a lightweight, versioned Markdown record
of the Host's current plan and judgment — not an executor container or
synchronization barrier. An AgentTeamRun may span multiple Waves while its
MemberRuns and native sessions continue. Every Team belongs to exactly one Mission,
one Mission owns exactly one Team, and a Team never spans machines.
No two AgentTeams may reference the same Mission. `NodeDaemonLease` is machine-scoped authority for all local Teams across registered Execution Spaces;
each machine has one machine-scoped NodeDaemon and the lease is never scoped to one Execution Space.
Cross-Team responsibility uses explicit WorkDelegation rather than parent/child
Team topology. Docs plus flat AgentTeam Organization is the accepted product
direction; ADR 0052 is superseded historical evidence. AgentMember is the one
durable organization-agent identity. Company Work is a
read-only aggregate over authoritative TeamWork and must never regain a second
task ledger or mutation path. Repository self-hosting remains the first
execution-foundation scenario.

## Hard Invariants

These rules bind every agent working in this repository. The linked canonical
doc carries the contract behind each rule.

1. **Native-session truth.** The provider's native session store is the sole
   execution truth for a member's transcript, tool calls, commands, file
   events, and provider turn lifecycle; do not mirror those
   streams into Harness ledgers. A provider `completed` status is not by
   itself proof of semantic success, answer, or approval.
   Resume must use the provider-native session id and verified provider
   operation, never a replay assembled from Harness events (ADR 0032,
   [docs/current/integration/native-session-storage.md](docs/current/integration/native-session-storage.md)).
2. **Harness owns only coordination records.** For Agent Team execution:
   `AgentTeam`, Mission relation, `AgentTeamRun`, `MemberRun` plus its
   native-session binding, `TeamMessage`, `PendingInteraction`, explicit
   outcome and artifact/check references, and control acknowledgements.
   Agent Team responsibility is proven by the latest `Work` rebuilt from
   ordered `WorkOperation` rows, each preserving its append-only `WorkEvent`
   and delivery deltas; `TeamMessage` is authored conversation only
   and may link a `work_id`. There is no Assignment Message compatibility
   path. For `dynamic_workflow`, WorkflowRun/WorkflowStep and its
   result/artifacts are the execution truth; for `host`, record the observable
   outcome and artifacts without inventing controlled child objects.
3. **Mission and Wave are the only native coordination objects** for new work.
   The superseded coordination stack is being removed under ADR 0028: do not
   load it into normal planning context, create new records, use its commands,
   or add new dependencies. Historical stores must be exported and verified
   before their old ledgers or code are deleted.
4. **Company/Execution/Project separation.** Execution Spaces own Mission/Wave,
   Agent Team, and Workflow coordination; Project Bindings identify the
   repository where providers execute and discover instructions, Skills,
   plugins, and MCP configuration. Selecting `--project` never switches the
   coordination store. Provider cwd resolves the attached
   `MemberWorkspaceBinding.canonical_root` > TeamRun `execution_root` > binding
   `project_root` — never an Execution Space,
   Company Store, or compatibility store root. Select space and project
   explicitly; never silently migrate or dual-write
   ([docs/current/operations/multi-project.md](docs/current/operations/multi-project.md), ADR 0033, ADR 0042).
5. **Provider upgrade gates.** Provider capability claims are execution-mode
   and version specific. Run `harness member providers --fail-on-review` after
   provider upgrades; an unreviewed version is `review_required`, not silently
   compatible. Change only one provider at a time; record the current version,
   candidate, install channel, and rollback path; never hot-replace the runtime
   of an active MemberRun or native session; roll back when installation,
   protocol probing, deterministic acceptance, or the live canary fails.
6. **Safety.** Authentication, payment, license acceptance, new credentials,
   and other protected actions require the appropriate Human or policy
   approval. The trusted-development Team policy that gives Codex, Claude, and
   Kimi members full execution access is a product policy — not a Provider
   capability and not approval for protected external effects. That permission
   policy is separate from execution-roster selection, which is scenario
   policy rather than a repository invariant (see Repository Execution Rules).
7. **Member lifecycle and control honesty.** New Agent Team members use only
   their persistent bidirectional mode: `codex_app_server`, `kimi_acp`, or
   `claude_agent_sdk`. Interrupt stops one current turn; Close ends the member
   runtime; Wave or TeamRun completion never implies Close. Cross-process
   control routes through the durable Team Supervisor lease, revalidated
   immediately before every drive; uncertain claimed deliveries require
   explicit reconciliation, never blind replay. Replacing a runtime drains or
   interrupts active turns first and never lets two runtime generations drive
   the same writable Workspace; resume the same native session under a higher
   Supervisor generation only when the reviewed contract allows, otherwise
   record the reason and start a new session, retaining the old one as
   history.
8. **One execution driver.** Each active MemberRun/native session/writable
   Workspace has exactly one top-level execution driver: `host_driven` or
   `provider_driven`. Never activate a provider-native goal and also issue an
   ordinary Harness start for the same work. Provider satisfaction never
   implies Host acceptance
   ([docs/current/architecture/member-continuation-model.md](docs/current/architecture/member-continuation-model.md),
   ADR 0041).
9. **No Plan Gate.** When the Host wants a plan first, it asks through an
   ordinary correlated Markdown message; the member replies, and the Host
   argues or approves in the same chain. Provider-native plan/goal features
   remain internal execution aids. Provider-native or chat-side subagents are
   implementation details of whoever invoked them; the harness must not claim
   lifecycle control it does not have.
10. **Honest capability claims.** Company OS contracts are additive and still
    being implemented; do not claim planned objects or fields exist until
    schemas, stores, APIs, and acceptance checks prove them. Keep the
    design-contract vs implemented-schema distinction explicit. In particular,
    AgentTeam authority is flat, Mission-Team identity is one-to-one, and every
    Team has immutable `node_id` placement under one machine-scoped NodeDaemon.
    The unified Work kernel is the shipped authority: do not
    create a second organization-agent identity or recreate a
    Company task ledger, migration fallback, or dual-write Work path.
11. **Skill optionality.** Skills are optional capabilities, never the
    authority for product architecture or Lead behavior. Do not load a skill
    merely because you are working in this repository; canonical docs, schemas,
    code, and ADRs win any conflict. Retired planning skills must not be
    installed, loaded, or referenced from active repository instructions. The
    generic harness core stays domain-neutral.

## Repository Execution Rules

- Repository development follows the Notion-led delivery contract in
  [docs/current/operations/workflow-git-pr.md](docs/current/operations/workflow-git-pr.md):
  frozen Spec -> umbrella Issue -> Primary Codex claim -> clean worktree -> one
  branch/PR -> final-SHA self-review and CI -> narrow Host Gate -> merge ->
  Notion closeout. Ordinary changes do not require a second reviewer queue.
- Harness Member execution is suspended for repository repair until the
  explicit dogfood admission standard passes. A Primary Codex Session may use
  bounded temporary Sub-Agents internally, but must not label that as Harness
  Member or Agent Team execution. Product TeamWork acceptance, Evidence,
  Finding, Failure, Acceptor, Gate, and Decision contracts remain intact and
  separate from developer self-review.
- A native Mission/Agent Team run is required when the claim under test is the
  native runtime itself or when a Spec explicitly selects it. Such a run is
  product evidence, not a prerequisite that silently replaces the repository
  development record. A small typo or single-line doc fix may be owner-local,
  but the final summary must identify the proportional exception.
- Harness dogfood runs follow
  [docs/current/product/agent-team-dogfood-loop.md](docs/current/product/agent-team-dogfood-loop.md):
  classify defects, repair on a clean lane, rerun the original scenario, then
  expand the pressure matrix. Never weaken a scenario or edit store evidence to
  make a run appear green.
- Scenario execution rosters and research budgets (for example the current
  dogfood roster) are scenario policy, not repository-wide invariants: they
  live in [docs/current/operations/operations.md](docs/current/operations/operations.md) and the owning scenario
  skill ([skills/dogfood-company-os/SKILL.md](skills/dogfood-company-os/SKILL.md))
  and must not be broadened into root instructions.
- Prefer the progression `doc -> skill -> schema -> CLI/API -> dashboard ->
  plugin`. The Agent Dashboard is the operator view for harness state; product
  dashboards for adapted projects remain separate.
- Local gates and commands: [docs/current/operations/operations.md](docs/current/operations/operations.md).
  `acceptance:mission-wave` proves the deterministic Mission/Wave, Agent Team,
  MCP, Kimi ACP adapter, and Dashboard contracts; a real-provider claim still
  requires a separately recorded native live run.

## Routing

- Product requirements: [docs/current/product/prd.md](docs/current/product/prd.md); Company OS product entry:
  [docs/current/company-os/README.md](docs/current/company-os/README.md); architecture:
  [docs/current/architecture/architecture-map.md](docs/current/architecture/architecture-map.md); concept model:
  [docs/current/architecture/concept-model.md](docs/current/architecture/concept-model.md)
- Detailed operating rules and the relocation map for this slimming:
  [docs/current/product/agent-operating-rules.md](docs/current/product/agent-operating-rules.md)
- Execution Spaces and Project Bindings:
  [docs/current/operations/multi-project.md](docs/current/operations/multi-project.md)
- Member continuation and execution drivers:
  [docs/current/architecture/member-continuation-model.md](docs/current/architecture/member-continuation-model.md)
- Provider runtime substrate: [docs/current/architecture/agent-runtime.md](docs/current/architecture/agent-runtime.md);
  integration model:
  [docs/current/architecture/agent-integration-model.md](docs/current/architecture/agent-integration-model.md)
- Operations gates and commands: [docs/current/operations/operations.md](docs/current/operations/operations.md)
- Dogfood method:
  [docs/current/product/agent-team-dogfood-loop.md](docs/current/product/agent-team-dogfood-loop.md)
- Documentation governance:
  [docs/current/documentation-governance.md](docs/current/documentation-governance.md)
- ADRs: [docs/decisions/README.md](docs/decisions/README.md) — especially 0026
  (Mission/Wave), 0027 (Company OS primary model), 0028 (retired coordination
  stack), 0032 (provider-native session truth), 0033 (member workspace), 0041
  (member continuation), 0042 (Execution Spaces and Project Bindings)

## Proportional Acceptance

Every non-trivial native Wave advances in four small stages: **Context**
(Mission intent, Wave plan, permissions, risk, Works, and decision
boundary are clear), **Execution** (the selected Host, Team, or Workflow owns
its internal plan and emits honest native records), **Outcome** (explicit Work
submissions, checks, artifacts, blockers, and review results are recorded), and
**Advance** (the Host records the outcome and next judgment; unrelated active
Works may carry forward unchanged). Review depth is proportional to risk;
Proposal/Decision/outcome evaluation is not a universal product chain.

Each MemberRun snapshots its concrete `ProviderIntegrationProfile`; platform
capability, execution-mode capability, adapter coverage, and product permission
are separate claims. Provider-native questions, approvals, or plan reviews that
actually pause a turn must be routed as PendingInteraction records. Ordinary
Host/Member planning remains correlated TeamMessage conversation. A provider
`completed` status is not by itself proof of semantic success, answer, or
approval.

The current trusted-development Team policy gives Codex, Claude, and Kimi
members full execution access so unattended work is not blocked by ordinary
tool authorization. This is a product policy, not a Provider capability and not
approval for protected external effects. Members decide when isolation is
useful and may create their own same-repository Git worktree; the Host declares
owned/conflicting paths and acceptance boundaries, not Git mechanics. Members
must report the actual worktree, branch, commit, checks, and conflicts.

New Agent Team members use only their persistent bidirectional mode:
`codex_app_server`, `kimi_acp`, or `claude_agent_sdk`. Bounded
`codex_exec`/`claude_cli` paths belong to Dynamic Workflow and historical
reads; they are not Team fallbacks. The one declared exception is
`external_interactive`: a user's own already-open interactive provider CLI
session may join a run as a non-driven member that Harness never spawns or
drives — it polls its inbox and replies over the trusted loopback CLI/MCP,
and it has no provider-native session record (evidence claims about its work
cannot resolve to one). The Host explicitly creates, messages,
inspects, interrupts, closes, reopens, and retires members. Interrupt stops one
current turn. Close releases the managed runtime and freezes the mailbox while
retaining the same MemberRun and provider-native session; Reopen increments its
runtime generation and resumes that exact session. Deactivate/Retire is the
permanent coordination end. Wave or TeamRun completion never implies Close.
Physical live control handles remain process-local to the Harness
service that started them. A durable Team Supervisor lease is the cross-process
control authority and contains a loopback service locator. Dashboard, CLI, and
MCP clients route controls to that owner; the owner revalidates supervisor id,
generation, status, and expiry immediately before driving its handle. After a
crash, a new Supervisor generation reattaches the recorded native sessions;
uncertain claimed deliveries require explicit reconciliation, never blind
replay.

Harness has no Plan Mode or Plan Gate. When the Host wants a plan first, it asks
through an ordinary correlated Markdown message; the Member replies, the Host
argues or approves in the same chain, and provider-native plan/goal features
remain internal execution aids.

Work is durable responsibility; a provider-native Goal is only one
possible continuation mechanism for executing it. Each active MemberRun/native
session/writable Workspace must have exactly one top-level execution driver:
either Harness starts the next provider cycle (`host_driven`) or an observed
provider-native continuation loop does (`provider_driven`). Never activate a
native goal and also issue an ordinary Harness start for the same work. A
provider-driven member may complete many native cycles without creating a new
MemberRun, but provider satisfaction never implies Host acceptance. Providers
without a reviewed native continuation capability remain first-class
host-driven members. See `docs/current/architecture/member-continuation-model.md` and ADR 0041.

Provider-native or chat-side subagents are implementation details of the Host
or member that invoked them. Optional hooks may record honest attribution, but
the harness must not claim lifecycle control it does not have.

Do not claim that Mission-scoped Agent Team work was accepted unless the store
shows the native Mission, its linked `AgentTeam`, the relevant Host-plan Wave,
Mission-scoped `AgentTeamRun` records, role-specific MemberRuns with owned or
claimed Works, versioned WorkEvents and delivery facts, Work-linked messages
where conversation occurred, explicit submitted results and Host acceptance,
and an explicit
Host Wave advance decision — with execution claims resolvable to the
provider-native session.

Company-level acceptance is separate: a Work must preserve source/result
provenance and responsibility, sensitive actions must satisfy their Approval
policy, and durable effects must update their related document and typed
records. An accepted Wave alone does not approve a payment, legal submission,
permission change, or organization change.

A native Mission/Wave slice is done only when the store can explain why the
work existed, how the Host's Wave context and judgment changed, which
teams/runs and Works were used, which WorkEvents allocated, blocked, submitted,
or accepted responsibility, which Messages explained coordination, and which
outcomes/checks/artifacts and provider-native sessions support
acceptance, why the Host advanced each Wave and closed the Mission, and what
should be reused, improved, split, or followed up next. If a future agent
cannot reconstruct the answer from repository files and native harness state,
the work is not fully accepted.
