# Agent Operating Rules

This repository builds Star Harness itself. Product truth lives in canonical
docs, schemas, ADRs, and implemented stores. Execution claims must additionally
be reconstructable from the native runtime records of the executor used.

## Product We Are Building

Star Harness is an AI Company OS with two primary systems: a Notion-like Docs
system for company memory and operating structure, and a mixed Organization of
humans, Standing Agents, external collaborators, and services. Documents create
WorkItems and Approvals; accountable actors execute them; results, evidence,
metrics, and financial effects return to the originating records.

Mission/Wave, Agent Team, Dynamic Workflow, Host execution, providers, plugins,
and MCP are the shared execution foundation. Their native relations are:

```text
Mission -> ordered Host-plan Wave
Mission <-> independent AgentTeam
AgentTeamRun -> MemberRun -> provider-native session
```

`Mission` is durable intent and may link multiple reusable teams. `Wave` is a
lightweight, versioned Markdown record of the Host's current plan and judgment;
it is not an executor container or synchronization barrier. An AgentTeamRun may
span multiple Waves while its MemberRuns and native sessions continue.
Assignment-message correlation owns member work. Dynamic Workflow owns its
workflow steps; Host execution may use provider-native subagents as an
implementation detail, with optional hooks for honest observation. The target
contract allows thinking only as sanitized transient live state: it must not be
persisted, replayed, treated as evidence, or forwarded to peers. New Kimi
writes already drop thinking instead of persisting it; a transient live display
channel is still pending.

The shared substrate includes provider sessions/runtimes, capability snapshots,
permission and budget ceilings, messages, artifacts, events, plugins/MCP, and
Dashboard projections. It does not collapse WorkflowRun, AgentTeamRun,
Host-native subagents, or future Standing Agents into one universal object.
Provider capability claims are execution-mode and version specific. Run
`harness member providers --fail-on-review` after provider upgrades; an
unreviewed version is `review_required`, not silently compatible. Interactive
chat/steer/interrupt controls must be backed by the selected mode's real
protocol and terminal acknowledgements.

Provider release discovery is read-only and should run at most once per day by
default. Provider version maintenance is Agent-managed: a per-version Human
confirmation is not required. Change only one Provider at a time; record the
current version, candidate, install channel and rollback path; never hot-replace
the runtime of an active MemberRun or native session. After a change, keep the
adapter `review_required` until mode-specific deterministic checks and a
proportional live canary justify updating the reviewed-version set. Roll back
when installation, protocol probing, deterministic acceptance or the live
canary fails. Authentication, payment, license acceptance, new credentials and
other protected actions still require the appropriate Human or policy
approval.

Standing Agents + Docs are the current product direction. Their Company OS
contracts are additive and still being implemented; do not claim planned
objects or fields exist until schemas, stores, APIs, and acceptance checks prove
them. See `docs/company-os/README.md` and ADR 0027.

The first Company OS acceptance scenario is a governed Trademark Management
module whose filing WorkItem, human approval, ¥3,000 financial commitment,
participants, evidence, and source/result documents remain one linked truth.
Repository self-hosting remains the first execution-foundation scenario.
Project-specific logic belongs in modules, adapters, and scenario skills, not
in the generic core.

## Native Product And Execution Objects

For company operations, the native product objects are `Document`,
`BusinessModule`, `TypedRecord`, `Relation`, `ActorRef`, `HumanMember`,
`AgentMember`, `OrgUnit`, `WorkItem`, `Assignment`, `Approval`,
`FinancialRecord`, and `MetricObservation`. Some of these are currently design
contracts rather than implemented schemas; keep that distinction explicit.

`Mission` and `Wave` are the only native coordination objects for new work.
The superseded coordination stack is being removed under ADR 0028: do not load
it into normal planning context, create new records, use its commands, or add
new dependencies. Historical stores must be exported and verified before their
old ledgers or code are deleted.

For Agent Team execution, Harness owns the coordination records:
`AgentTeam`, Mission relation, `AgentTeamRun`, `MemberRun` plus its
native-session binding, `TeamMessage`, `PendingInteraction`, explicit outcome
and artifact/check references, and control acknowledgements. Assignment
ownership is proven by
`TeamMessage(kind=assignment)` plus `correlation_id`. The provider's native
session store is the sole execution truth for that member's transcript, tool
calls, commands, file events, and provider turn lifecycle; do not mirror those
streams into Harness ledgers.

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

An Assignment is durable work ownership; a provider-native Goal is only one
possible continuation mechanism for executing it. Each active MemberRun/native
session/writable Workspace must have exactly one top-level execution driver:
either Harness starts the next provider cycle (`host_driven`) or an observed
provider-native continuation loop does (`provider_driven`). Never activate a
native goal and also issue an ordinary Harness start for the same work. A
provider-driven member may complete many native cycles without creating a new
MemberRun, but provider satisfaction never implies Host acceptance. Providers
without a reviewed native continuation capability remain first-class
host-driven members. See `docs/member-continuation-model.md` and ADR 0041.

Provider-native or chat-side subagents are implementation details of the Host
or member that invoked them. Optional hooks may record honest attribution, but
the harness must not claim lifecycle control it does not have.

Do not claim that Mission-scoped Agent Team work was accepted unless the store
shows:

- a native Mission, its linked `AgentTeam`, and the relevant Host-plan Wave;
- one or more Mission-scoped `AgentTeamRun` records;
- role-specific MemberRuns and assignment messages for actual members;
- correlation-backed blocker, handoff, or review messages where those events
  occurred;
- an explicit outcome, plus artifact/check references when they are useful;
- an explicit Host Wave advance decision. Active unrelated assignments may
  continue into the next Wave.

Execution claims must also resolve to the provider-native session when the
member used a provider. Missing or incompatible native sessions are reported
honestly; Harness coordination history does not impersonate a backup
transcript. Resume must use the provider-native session id and verified
provider operation, never a replay assembled from Harness events.

For `dynamic_workflow`, WorkflowRun/WorkflowStep and its result/artifacts are
the execution truth. For `host`, record the observable outcome and artifacts
without inventing controlled child objects.

## How To Develop This Repository With The Harness

The Lead Agent should use this sequence for non-trivial new work:

1. Inspect relevant code/docs and native state with `harness mission list`,
   `harness wave list`, and the Agent Team/Dynamic Workflow surfaces needed.
2. Create or select the Mission, link any independent teams the Host may use,
   and write the current ordered Wave as Markdown plan and judgment.
3. Let each executor own its internal plan. A Wave records what changed, what
   the Host decided, which work carries forward, and why it can advance.
4. For Agent Team work, create one Mission-scoped TeamRun and use Assignment
   messages and correlations for lane ownership. Give concurrent members
   disjoint owned paths or explicit conflict boundaries. Let each Member decide
   whether to create its own same-repository worktree and surface shared-file
   conflicts to the Host. Do not pass a Wave id on the primary path.
5. Keep Harness-owned checks, artifact references, blockers, handoffs, reviews,
   control acknowledgements, and outcomes durable. Keep provider chat, tool,
   command, file, turn, and reasoning streams in the provider-native session;
   do not persist a duplicate in Harness.
6. Apply review proportional to risk. A reviewer member or stricter repository
   governance may be added when useful, but Proposal/Decision/outcome evaluation is
   not a universal product chain.
7. Advance the Wave from an explicit Host outcome. Do not wait for unrelated
   member work; carry its same assignment, MemberRun, and native session into
   the next Wave.
8. Re-plan the next Wave from plan-vs-actual deviation and close the Mission
   with an explicit outcome summary. Closing never archives or deletes a team.

When the work is a Harness dogfood run, follow
`docs/product/agent-team-dogfood-loop.md`. A discovered defect is not the end of
dogfood: the Host classifies it, opens a Repair Wave or tracked issue, fixes it
on a clean lane, reruns the original scenario, and only then expands the
pressure matrix. Do not weaken the scenario or manually edit store evidence to
make a run appear green.

## Execution Space And Project Binding

One `serve` / dashboard manages independent Execution Spaces and Project
Bindings. Execution Spaces under `~/.harness/execution-spaces/<id>/` own
Mission/Wave, Agent Team, and Workflow coordination. Project Bindings identify
the registered Git repository/directory where providers execute and discover
instructions, Skills, plugins, and MCP configuration. Selecting `--project`
never switches the coordination store.

Agent Team provider cwd resolves as member `worktree_ref` > TeamRun
`execution_root` > Project Binding `project_root`, never an Execution Space,
Company Store, or compatibility store root. Overrides must be the binding root
or a Git worktree sharing its Git common directory; external Codex worktrees
are valid. Treat cwd as an explicit execution and permission boundary. See ADR
0033, ADR 0042, and [docs/multi-project.md](docs/multi-project.md).

- Select the Execution Space explicitly (`--space <id>`, `HARNESS_SPACE`, or
  `harness space switch`) before writing coordination records.
- Select the Project Binding explicitly (`--project <id|path>`,
  `HARNESS_PROJECT`, or `harness project switch`) before spawning workers.
- `AgentTeamRun.project_binding_id` and `WorkflowRun.project_binding_id` pin the
  execution resource; later selector changes must not retarget them.
- `--store` / `HARNESS_ROOT` still win as back-compat overrides but are
  deprecation-warned — prefer `harness init` / `harness space switch`.
- The reserved GLOBAL `_global` (`~/`) project is non-git: read-only work runs
  there, but `writable` / `isolation="worktree"` nodes are rejected with an
  actionable message (and have no diff evidence).
- Copy project-derived execution history with explicit
  `harness space migrate-from-project`; the source is retained and verified.
  Centralize a repo-local `.harness` first with `harness project migrate` when
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
skills; the generic harness core must stay domain-neutral.

Useful local commands:

```bash
target/debug/harness init
target/debug/harness mission create --title <title> --objective <objective> \
  --context <mission-markdown>
target/debug/harness mission create-team --id <mission> --name <team> \
  --description <purpose> --lead host --member <agent-member-id>
target/debug/harness wave create --mission-id <mission> --title <title> \
  --objective <objective> --context <wave-markdown>
target/debug/harness team-run create --mission-id <mission> \
  --agent-team-id <team> --objective <objective>
target/debug/harness wave advance --id <wave> --advanced-by <actor> \
  --outcome <summary>
target/debug/harness dashboard snapshot
target/debug/harness serve --addr 127.0.0.1:8787
npx pnpm@9.15.4 acceptance:mission-wave
```

`acceptance:mission-wave` proves the deterministic Mission/Wave, Agent Team,
MCP, Kimi ACP adapter, and Dashboard contracts. A real-provider claim still
requires a separately recorded native live run; the deterministic gate is not
live-provider evidence.

## Self-Hosting Rules

This repository should dogfood native Mission/Wave and the executor it is
changing once that slice is capable of running the work. A bootstrap change
that creates or repairs the native path may use the current host/subagent
mechanism, but must say so and add focused acceptance for the path it creates.

- For meaningful product, schema, CLI, dashboard, provider, adapter, or skill
  changes, prefer a native Mission/Wave run when the needed executor path works.
- A small typo or single-line doc fix may be Lead-local, but the final summary
  must say that it was a Lead-local exception.
- Any feature claim about Agent Team behavior must be backed by linked team/run,
  member/native-session binding, assignment/correlation, explicit outcome and
  useful artifact/check references, Host Wave decisions, and resolvable native
  provider records for claims about the member's own execution.
- When the current workflow feels slow or manual, record a follow-up Wave or
  issue instead of normalizing hidden local reasoning.
- Prefer the progression `doc -> skill -> schema -> CLI/API -> dashboard ->
  plugin`. A plugin is justified only after the object contracts and commands
  are stable enough to reduce variance.
- The Agent Dashboard is the operator view for harness state. Product dashboards
  for adapted projects remain separate.

## Staged Acceptance

Every non-trivial native Wave advances in four small stages:

1. Context: Mission intent, Wave Markdown plan, permissions, risk, assignments,
   and intended decision boundary are clear.
2. Execution: the selected Host, Team, or Workflow owns its internal plan and
   emits honest native records. Agent Team lanes start from assignment messages.
3. Outcome: explicit checks, artifacts, blockers, handoffs, and review results
   needed for this Wave are recorded. Review depth is proportional to risk.
4. Advance: the Host records the outcome and next judgment. Unrelated active
   assignments may carry forward without changing MemberRun or native session.

Company-level acceptance is separate: a WorkItem must preserve source/result
provenance and responsibility, sensitive actions must satisfy their Approval
policy, and durable effects must update their related document and typed
records. An accepted Wave alone does not approve a payment, legal submission,
permission change, or organization change.

## What Counts As Done

A native Mission/Wave slice is done only when the store can explain:

- why the work existed;
- how the Host's Wave context and judgment changed;
- which independent teams/runs were used and which assignments carried forward;
- which TeamMessages assigned or handed off Agent Team lanes;
- which explicit outcomes, checks, and artifacts support acceptance and which
  provider-native session supports claims about the member's execution;
- why the Host advanced each Wave and closed the Mission;
- what should be reused, improved, split, or followed up next.

If a future agent cannot reconstruct the answer from repository files and
native harness state, the work is not fully accepted.
