# Agent Operating Rules — Detailed Companion

```text
status: canonical operating detail
owner_role: lead-operations
canonical_for: full agent operating rules relocated from root AGENTS.md, plus the relocation map
```

Root [AGENTS.md](../AGENTS.md) states product identity, hard invariants,
repository execution rules, routing links, and proportional acceptance. This
companion carries the operating detail behind those invariants so the root file
stays slim. Where a canonical contract doc exists, it is linked and wins any
conflict; do not restate its contract here.

## Product Context

Firm is an AI Company OS with two primary systems: a Notion-like Docs
system for company memory and operating structure, and a mixed Organization of
humans, Standing Agents, external collaborators, and services. Documents create
WorkItems and Approvals; accountable actors execute them; results, evidence,
metrics, and financial effects return to the originating records
([prd.md](prd.md), [company-os/README.md](company-os/README.md)).

Mission/Wave, Agent Team, Dynamic Workflow, Host execution, providers, plugins,
and MCP are the shared execution foundation. `Mission` is durable intent and
may link multiple reusable teams. `Wave` is a lightweight, versioned Markdown
record of the Host's current plan and judgment; it is not an executor container
or synchronization barrier. An AgentTeamRun may span multiple Waves while its
MemberRuns and native sessions continue. ADR 0050 defines Work as member
responsibility and removes Assignment-message ownership. `WorkOperation`
atomically preserves the resulting Work, its `WorkEvent`, and delivery deltas;
`WorkDelivery` wakes or updates a Member runtime, and
`TeamMessage` is authored conversation only. Dynamic Workflow owns its workflow steps; Host execution may use
provider-native subagents as an implementation detail, with optional hooks for
honest observation. The target contract allows thinking only as sanitized
transient live state: it must not be persisted, replayed, treated as evidence,
or forwarded to peers. New Kimi writes already drop thinking instead of
persisting it; a transient live display channel is still pending.

The shared substrate includes provider sessions/runtimes, capability snapshots,
permission and budget ceilings, messages, artifacts, events, plugins/MCP, and
Dashboard projections. It does not collapse WorkflowRun, AgentTeamRun,
Host-native subagents, or future Standing Agents into one universal object.
Interactive chat/steer/interrupt controls must be backed by the selected mode's
real protocol and terminal acknowledgements.

Provider release discovery is read-only and should run at most once per day by
default. Provider version maintenance is Agent-managed: a per-version Human
confirmation is not required. Change only one Provider at a time; record the
current version, candidate, install channel and rollback path; never
hot-replace the runtime of an active MemberRun or native session. After a
change, keep the adapter `review_required` until mode-specific deterministic
checks and a proportional live canary justify updating the reviewed-version
set. Roll back when installation, protocol probing, deterministic acceptance or
the live canary fails. Authentication, payment, license acceptance, new
credentials and other protected actions still require the appropriate Human or
policy approval.

Docs plus recursive AgentTeam Organization is the accepted target direction.
AgentMember is the target durable agent identity and Work is the target shared
responsibility kernel. Current StandingAgent, Company WorkItem, OrgUnit, and
explicit execution-ref rows remain compatibility implementation truth until an
explicit verified cutover. Do not claim target objects or fields exist until
schemas, stores, APIs, and acceptance checks prove them. See
[company-os/README.md](company-os/README.md) and
[ADR 0052](decisions/0052-nested-agent-teams-are-the-agent-organization.md).

The first Company OS acceptance scenario is a governed Trademark Management
module whose filing WorkItem, human approval, ¥3,000 financial commitment,
participants, evidence, and source/result documents remain one linked truth.
Repository self-hosting remains the first execution-foundation scenario.
Project-specific logic belongs in modules, adapters, and scenario skills, not
in the generic core.

## Native Product And Execution Objects

For company operations, the native product objects are `Document`,
`BusinessModule`, `TypedRecord`, `Relation`, `ActorRef`, `HumanMember`,
`AgentMember`, recursive `AgentTeam`, `Work`, `Approval`,
`FinancialRecord`, and `MetricObservation`. Some of these are currently design
contracts rather than implemented schemas; current `OrgUnit`, `StandingAgent`,
`WorkItem`, and `Assignment` rows remain compatibility implementation truth.
Keep that distinction explicit. See
[concept-model.md](concept-model.md).

`Mission` and `Wave` are the only native coordination objects for new work. The
superseded coordination stack is being removed under
[ADR 0028](decisions/0028-retire-goal-phase-task-graph.md): do not load it into
normal planning context, create new records, use its commands, or add new
dependencies. Historical stores must be exported and verified before their old
ledgers or code are deleted.

For Agent Team execution, Harness owns the coordination records: `AgentTeam`,
Mission relation, `AgentTeamRun`, `MemberRun` plus its native-session binding,
`Work`, `WorkEvent`, `WorkDelivery`, `TeamMessage`, `PendingInteraction`,
explicit outcome and artifact/check references, and control acknowledgements.
Work owner and state prove responsibility; TeamMessage is authored conversation.
There is no Assignment Message compatibility path; active stores using the old
ownership model must be reset or explicitly migrated rather than dual-read.
The provider's native session store is the sole execution truth for that member's transcript, tool
calls, commands, file events, and provider turn lifecycle; do not mirror those
streams into Harness ledgers
([ADR 0032](decisions/0032-provider-native-session-is-execution-truth.md),
[integration/native-session-storage.md](integration/native-session-storage.md)).

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

## Agent Team Member Lifecycle And Control

New Agent Team members use only their persistent bidirectional mode:
`codex_app_server`, `kimi_acp`, or `claude_agent_sdk`. Bounded
`codex_exec`/`claude_cli` paths belong to Dynamic Workflow and historical
reads; they are not Team fallbacks. The Host explicitly creates, messages,
inspects, interrupts, closes, and resumes members. Interrupt stops one current
turn; Close ends the member runtime; Wave or TeamRun completion never implies
Close. Physical live control handles remain process-local to the Firm
service that started them. A durable Team Supervisor lease is the cross-process
control authority and contains a loopback service locator. Dashboard, CLI, and
MCP clients route controls to that owner; the owner revalidates supervisor id,
generation, status, and expiry immediately before driving its handle. After a
crash, a new Supervisor generation reattaches the recorded native sessions;
uncertain claimed deliveries require explicit reconciliation, never blind
replay.

Harness has no Plan Mode or Plan Gate. When the Host wants a plan first, it
asks through an ordinary correlated Markdown message; the Member replies, the
Host argues or approves in the same chain, and provider-native plan/goal
features remain internal execution aids.

Work is durable responsibility; a provider-native Goal is only one possible
continuation mechanism for executing it. Each active MemberRun/native
session/writable Workspace must have exactly one top-level execution driver:
either Harness starts the next provider cycle (`host_driven`) or an observed
provider-native continuation loop does (`provider_driven`). Never activate a
native goal and also issue an ordinary Harness start for the same work. A
provider-driven member may complete many native cycles without creating a new
MemberRun, but provider satisfaction never implies Host acceptance. Providers
without a reviewed native continuation capability remain first-class
host-driven members. See
[member-continuation-model.md](member-continuation-model.md) and
[ADR 0041](decisions/0041-provider-neutral-member-continuation.md).

Provider-native or chat-side subagents are implementation details of the Host
or member that invoked them. Optional hooks may record honest attribution, but
the firm must not claim lifecycle control it does not have.

## Acceptance Evidence For Mission-Scoped Agent Team Work

Do not claim that Mission-scoped Agent Team work was accepted unless the store
shows:

- a native Mission, its linked `AgentTeam`, and the relevant Host-plan Wave;
- one or more Mission-scoped `AgentTeamRun` records;
- role-specific MemberRuns and owned or claimed Works for actual members;
- ordered WorkOperations preserving append-only WorkEvents, resulting Work
  projections, and WorkDelivery facts for allocation, execution, blocking,
  submission, recovery, and acceptance;
- Work-linked conversation where questions, explanation, or peer coordination
  occurred;
- submitted results and explicit Host acceptance, plus artifact/check refs;
- an explicit Host Wave advance decision. Active unrelated Works may
  continue into the next Wave.

Execution claims must also resolve to the provider-native session when the
member used a provider. Missing or incompatible native sessions are reported
honestly; Harness coordination history does not impersonate a backup
transcript. Resume must use the provider-native session id and verified
provider operation, never a replay assembled from Harness events.

For `dynamic_workflow`, WorkflowRun/WorkflowStep and its result/artifacts are
the execution truth. For `host`, record the observable outcome and artifacts
without inventing controlled child objects.

## Developing This Repository With The Harness

The Lead Agent should use this sequence for non-trivial new work:

1. Inspect relevant code/docs and native state with `firm mission list`,
   `firm wave list`, and the Agent Team/Dynamic Workflow surfaces needed.
2. Create or select the Mission, link any independent teams the Host may use,
   and write the current ordered Wave as Markdown plan and judgment.
3. Let each executor own its internal plan. A Wave records what changed, what
   the Host decided, which work carries forward, and why it can advance.
4. For Agent Team work, create one Mission-scoped TeamRun and put every lane on
   its shared Works board. Assign bounded responsibility directly or expose
   eligible unassigned Work for atomic claim. Give concurrent members
   disjoint owned paths or explicit conflict boundaries. Let each Member decide
   whether to create its own same-repository worktree and surface shared-file
   conflicts to the Host. Do not pass a Wave id on the primary path.
5. Keep Harness-owned WorkOperations/WorkEvents, WorkDelivery, checks, artifact references,
   blockers, submissions, Host acceptance, control acknowledgements, and
   outcomes durable. Keep provider chat, tool,
   command, file, turn, and reasoning streams in the provider-native session;
   do not persist a duplicate in Harness.
6. Apply review proportional to risk. A reviewer member or stricter repository
   governance may be added when useful, but Proposal/Decision/outcome
   evaluation is not a universal product chain.
7. Advance the Wave from an explicit Host outcome. Do not wait for unrelated
   member work; carry its same Work, MemberRun, and native session into
   the next Wave.
8. Re-plan the next Wave from plan-vs-actual deviation and close the Mission
   with an explicit outcome summary. Closing never archives or deletes a team.

When the work is a Harness dogfood run, follow
[product/agent-team-dogfood-loop.md](product/agent-team-dogfood-loop.md). A
discovered defect is not the end of dogfood: the Host classifies it, opens a
Repair Wave or tracked issue, fixes it on a clean lane, reruns the original
scenario, and only then expands the pressure matrix. Do not weaken the scenario
or manually edit store evidence to make a run appear green.

Useful local commands:

```bash
target/debug/firm init
target/debug/firm mission create --title <title> --objective <objective> \
  --context <mission-markdown>
target/debug/firm mission create-team --id <mission> --name <team> \
  --description <purpose> --lead host --member <agent-member-id>
target/debug/firm wave create --mission-id <mission> --title <title> \
  --objective <objective> --context <wave-markdown>
target/debug/firm team-run create --mission-id <mission> \
  --agent-team-id <team> --objective <objective>
target/debug/firm team-run work create --team-run-id <team-run> \
  --title <title> --context <markdown> \
  --completion-criteria <criteria> --owner-member-run-id <member-run>
target/debug/firm team-run work list --team-run-id <team-run>
target/debug/firm wave advance --id <wave> --advanced-by <actor> \
  --outcome <summary>
target/debug/firm dashboard snapshot
target/debug/firm serve --addr 127.0.0.1:8787
npx pnpm@9.15.4 acceptance:mission-wave
```

`acceptance:mission-wave` proves the deterministic Mission/Wave, Agent Team,
MCP, Kimi ACP adapter, and Dashboard contracts. A real-provider claim still
requires a separately recorded native live run; the deterministic gate is not
live-provider evidence.

## Execution Space And Project Binding

Canonical contract: [multi-project.md](multi-project.md),
[ADR 0033](decisions/0033-agent-team-workspace-contract.md),
[ADR 0042](decisions/0042-company-store-execution-space-project-binding.md).
The operator rules are:

One `serve` / dashboard manages independent Execution Spaces and Project
Bindings. Execution Spaces under `~/.firm/execution-spaces/<id>/` own
Mission/Wave, Agent Team, and Workflow coordination. Project Bindings identify
the registered Git repository/directory where providers execute and discover
instructions, Skills, plugins, and MCP configuration. Selecting `--project`
never switches the coordination store.

Agent Team provider cwd resolves as member `worktree_ref` > TeamRun
`execution_root` > Project Binding `project_root`, never an Execution Space,
Company Store, or compatibility store root. Overrides must be the binding root
or a Git worktree sharing its Git common directory; external Codex worktrees
are valid. Treat cwd as an explicit execution and permission boundary.

- Select the Execution Space explicitly (`--space <id>`, `FIRM_SPACE`, or
  `firm space switch`) before writing coordination records.
- Select the Project Binding explicitly (`--project <id|path>`,
  `FIRM_PROJECT`, or `firm project switch`) before spawning workers.
- `AgentTeamRun.project_binding_id` and `WorkflowRun.project_binding_id` pin
  the execution resource; later selector changes must not retarget them.
- `--store` / `FIRM_ROOT` still win as back-compat overrides but are
  deprecation-warned — prefer `firm init` / `firm space switch`.
- The reserved GLOBAL `_global` (`~/`) project is non-git: read-only work runs
  there, but `writable` / `isolation="worktree"` nodes are rejected with an
  actionable message (and have no diff evidence).
- Copy project-derived execution history with explicit
  `firm space migrate-from-project`; the source is retained and verified.
  Centralize a repo-local `.harness` first with `firm project migrate` when
  needed. Never silently migrate or dual-write.

`ProjectContext` is compatibility infrastructure. Do not infer that a Git
repository owns execution or Company OS truth. An Agent Company Workspace /
Company Store may contain multiple operating areas while external repositories
remain Project Bindings or source/delivery mappings. Mission/Wave, Agent Team,
Dynamic Workflow, and Host execution remain usable with no Company Store.

## Skills Are Optional Capabilities

Repository skills are implementation and distribution artifacts, not the
authority for product architecture or Lead behavior. Agents must not load a
skill merely because they are working in this repository. Use a retained skill
only when the user requests it or the current task explicitly needs that
capability, and prefer canonical architecture, schemas, code, and ADRs when a
skill conflicts with them.

Retired planning skills must not be installed, loaded, or referenced from
active repository instructions. Skills are optional capabilities, never the
authority for product architecture.

Do not make Earning Engine or other domain skills mandatory for this
repository. Domain workflows enter through adapters and scenario-specific
skills; the generic firm core must stay domain-neutral.

## Self-Hosting Rules

This repository should dogfood native Mission/Wave and the executor it is
changing once that slice is capable of running the work. A bootstrap change
that creates or repairs the native path may use the current host/subagent
mechanism, but must say so and add focused acceptance for the path it creates.

- For meaningful product, schema, CLI, dashboard, provider, adapter, or skill
  changes, prefer a native Mission/Wave run when the needed executor path
  works.
- A small typo or single-line doc fix may be Lead-local, but the final summary
  must say that it was a Lead-local exception.
- Any feature claim about Agent Team behavior must be backed by linked
  team/run, member/native-session binding, owned or claimed Work, WorkEvent and
  WorkDelivery lineage, explicit submission/Host acceptance, useful
  artifact/check references, Host Wave decisions, and
  resolvable native provider records for claims about the member's own
  execution.
- When the current workflow feels slow or manual, record a follow-up Wave or
  issue instead of normalizing hidden local reasoning.
- Prefer the progression `doc -> skill -> schema -> CLI/API -> dashboard ->
  plugin`. A plugin is justified only after the object contracts and commands
  are stable enough to reduce variance.
- The Agent Dashboard is the operator view for firm state. Product
  dashboards for adapted projects remain separate.

## Runtime Replacement And Rolling Reconciliation

Scenario execution rosters and research budgets are scenario policy, not
repository-wide invariants. The current dogfood roster and per-member research
budget live in [operations.md](operations.md) and
[../skills/dogfood-company-os/SKILL.md](../skills/dogfood-company-os/SKILL.md);
do not mirror quota-specific rosters here — update those scenario carriers
instead.

The provider-neutral principle behind rolling reconciliation binds every
runtime replacement (Firm binary, adapter, protocol, permission,
model-control, Plugin, or Skill contract change):

1. classify whether only UI/Docs projection changed or a runtime contract
   changed; projection-only changes need no restart;
2. drain or interrupt active turns before replacing an incompatible runtime;
3. install/sync canonical artifacts from the new version before starting the
   next generation;
4. never let two runtime generations drive the same writable Workspace;
5. resume the same MemberRun and provider-native Session under a higher
   Supervisor generation when the reviewed contract allows it; when
   compatibility cannot be proven, record the reason and start a new native
   Session, retaining the old Session as history;
6. reconcile queued/claimed mail, permissions, model controls, cwd/Skill
   roots, and the single writable-Workspace driver before resuming; and
7. prove the new generation lane by lane: the latest ready WorkDelivery reaches
   the existing MemberRun, the same native Session can answer linked
   conversation and submit Work, and the Host can explicitly accept or request
   changes.

When a scenario runs members in Git worktrees, reconciliation also rebases
each member worktree onto the new base or recreates a clean same-repository
worktree when rebase is unsafe, and the reconciliation itself is tracked work
— link the triggering merge commit, the Supervisor generations, and each
resume-or-new-session decision to the governing record.

## Relocation Map

How the pre-slimming root `AGENTS.md` (337 lines) maps to the current layout.
"Root" means the current slim [AGENTS.md](../AGENTS.md); "here" means this
companion.

| Former AGENTS.md content | Now lives in | Notes |
| --- | --- | --- |
| Product We Are Building — two primary systems | Root §Product Identity | Canonical: [prd.md](prd.md), [company-os/README.md](company-os/README.md) |
| Product We Are Building — Mission/Wave relations diagram and semantics | Root §Product Identity; here §Product Context | Full prose kept here |
| Product We Are Building — Work responsibility, Work-linked conversation, thinking-as-transient contract | Here §Product Context | Thinking stays non-persisted; live display channel still pending |
| Product We Are Building — shared substrate, capability claims, interactive controls | Root invariant 5 (gate); here §Product Context | Substrate contract: [agent-runtime.md](agent-runtime.md) |
| Product We Are Building — provider release discovery and version maintenance | Root invariant 5; here §Product Context | Full procedure kept here |
| Product We Are Building — recursive AgentTeams + Docs direction, honesty about planned objects | Root invariant 10; here §Product Context | ADR 0052 |
| Product We Are Building — Trademark scenario, module placement | Here §Product Context | Canonical scenario: [prd.md](prd.md) |
| Native Product And Execution Objects — object inventory | Here §Native Product And Execution Objects | [concept-model.md](concept-model.md) |
| Native Product And Execution Objects — Mission/Wave only, ADR 0028 retirement | Root invariant 3; here §Native Product And Execution Objects | — |
| Agent Team execution records, Work responsibility/delivery proof, native-session boundary | Root invariants 1–2; here §Native Product And Execution Objects and §Acceptance Evidence | ADR 0032; the exact phrases `provider's native`, `streams into Harness ledgers`, and `Resume must use the provider-native session id` must stay in root AGENTS.md — `scripts/check-native-session-boundary.mjs` greps for them |
| MemberRun ProviderIntegrationProfile, PendingInteraction, `completed` ≠ success | Root invariants 1–2; here §Native Product And Execution Objects | — |
| Trusted-development Team policy and worktree norms | Root invariant 6; here §Native Product And Execution Objects | — |
| Member modes, interrupt/close, Team Supervisor lease, reconciliation | Root invariant 7; here §Agent Team Member Lifecycle And Control | — |
| No Plan Mode / Plan Gate | Root invariant 9; here §Agent Team Member Lifecycle And Control | — |
| Assignment vs provider-native Goal, single execution driver | Root invariant 8; here §Agent Team Member Lifecycle And Control | [member-continuation-model.md](member-continuation-model.md), ADR 0041 |
| Subagents as implementation details | Root invariant 9; here §Agent Team Member Lifecycle And Control | — |
| Acceptance checklist for Mission-scoped Team work; workflow/host truth | Root §Proportional Acceptance (condensed); here §Acceptance Evidence (full) | — |
| How To Develop This Repository — 8-step Lead sequence | Root §Repository Execution Rules (condensed); here §Developing This Repository (full) | — |
| Dogfood defect/repair paragraph | Root §Repository Execution Rules; here §Developing This Repository | Canonical method: [product/agent-team-dogfood-loop.md](product/agent-team-dogfood-loop.md) |
| Useful local commands block | Here §Developing This Repository | Gates: [operations.md](operations.md) |
| Execution Space And Project Binding section | Root invariant 4; here §Execution Space And Project Binding | Canonical contract: [multi-project.md](multi-project.md), ADR 0033, ADR 0042 |
| Skills Are Optional Capabilities | Root invariant 11; here §Skills Are Optional Capabilities | — |
| Self-Hosting Rules | Root §Repository Execution Rules (condensed); here §Self-Hosting Rules (full) | — |
| Staged Acceptance | Root §Proportional Acceptance | Not duplicated here |
| What Counts As Done | Root §Proportional Acceptance | Not duplicated here |
| Provider-neutral rolling reconciliation principle (aligned post-slimming, not relocated) | Root invariant 7; here §Runtime Replacement And Rolling Reconciliation | Scenario rosters and research budgets stay in scenario policy — [operations.md](operations.md) and [../skills/dogfood-company-os/SKILL.md](../skills/dogfood-company-os/SKILL.md) — and are deliberately not mirrored in root instructions |
