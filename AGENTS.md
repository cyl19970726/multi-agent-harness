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
and MCP are the shared execution foundation. Their native hierarchy is:

```text
Mission -> ordered Wave -> executor
  executor = agent_team | dynamic_workflow | host
```

`Mission` is durable intent. `Wave` is a lightweight ordered unit with an
objective, executor, outcome, artifacts, and gate. Agent Team uses
assignment-message correlation for member
ownership; Dynamic Workflow owns its workflow steps; Host execution may use
provider-native subagents as an implementation detail, with optional hooks for
honest observation. The target contract allows thinking only as sanitized
transient live state: it must not be persisted, replayed, treated as evidence,
or forwarded to peers. New Kimi writes already drop thinking instead of
persisting it; a transient live display channel is still pending.

The shared substrate includes provider sessions/runtimes, capability snapshots,
permission and budget ceilings, messages, artifacts, events, plugins/MCP, and
Dashboard projections. It does not collapse WorkflowRun, AgentTeamRun,
Host-native subagents, or future Standing Agents into one universal object.

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

For `executor_kind=agent_team`, the canonical execution records are
`AgentTeamRun`, `MemberRun`, `TeamMessage`, explicit `MemberAction` summaries,
artifacts, and the Wave gate. Assignment ownership is proven by
`TeamMessage(kind=assignment)` plus `correlation_id`.

Provider-native or chat-side subagents are implementation details of the Host
or member that invoked them. Optional hooks may record honest attribution, but
the harness must not claim lifecycle control it does not have.

Do not claim that an Agent Team Wave was accepted unless the store shows:

- a native Mission and native `Wave(executor_kind=agent_team)`;
- one or more linked `AgentTeamRun` attempts;
- role-specific MemberRuns and assignment messages for actual members;
- correlation-backed blocker, handoff, or review messages where those events
  occurred;
- an explicit outcome, plus artifact/check references when they are useful;
- a Wave gate naming the accepted completed attempt.

For `dynamic_workflow`, WorkflowRun/WorkflowStep and its result/artifacts are
the execution truth. For `host`, record the observable outcome and artifacts
without inventing controlled child objects.

## How To Develop This Repository With The Harness

The Lead Agent should use this sequence for non-trivial new work:

1. Inspect relevant code/docs and native state with `harness mission list`,
   `harness wave list`, and the Agent Team/Dynamic Workflow surfaces needed by
   the selected executor.
2. Create or select the Mission, define its ordered Waves, each Wave's executor,
   and its lightweight gate.
   When `executor_kind=agent_team`, define only the roles, permissions, model
   tiers, depth, owned surfaces, and artifacts that Wave needs.
3. Let the selected executor own its internal plan; a Wave remains a lightweight
   outcome, executor, artifact, and gate boundary.
4. For Agent Team work, create the linked TeamRun, then use its Assignment
   messages and correlations for lane ownership. Give concurrent members
   disjoint owned paths or worktrees and surface shared-file conflicts to the
   Host.
5. Keep explicit actions, checks, artifacts, blockers, handoffs, reviews, and
   outcomes durable. Do not persist provider thinking.
6. Apply review proportional to risk. A reviewer member or stricter repository
   governance may be added when useful, but Proposal/Decision/outcome evaluation is
   not a universal product chain.
7. Gate the Wave as `accepted`, `revise`, or `blocked`. A retry creates another
   executor run; it never mutates away the earlier attempt.
8. Re-plan the next Wave from plan-vs-actual deviation and close the Mission
   with an explicit outcome summary when Mission closeout support is available.

## Project Selection (Multi-Project)

One `serve` / dashboard manages many projects. Each has a centralized
`store_root` (`~/.harness/projects/<id>/`, the JSONL ledgers) and a `project_root`
(the git repo where `CLAUDE.md` / `AGENTS.md` / worktrees live); a spawned
worker's cwd derives from `project_root`, not the harness process cwd.

- Select the project explicitly (`--project <id|path>`, `HARNESS_PROJECT`, or
  `harness project switch`) before spawning workers; do not rely on cwd.
- `--store` / `HARNESS_ROOT` still win as back-compat overrides but are
  deprecation-warned — prefer `harness init` / `harness project switch`.
- The reserved GLOBAL `_global` (`~/`) project is non-git: read-only work runs
  there, but `writable` / `isolation="worktree"` nodes are rejected with an
  actionable message (and have no diff evidence).
- Centralize a legacy repo-local `.harness` with `harness project migrate` (copies
  with no data loss; marks the old store). Full reference:
  [docs/multi-project.md](docs/multi-project.md).

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
target/debug/harness mission create --title <title> --objective <objective>
target/debug/harness wave create --mission-id <mission> --title <title> \
  --objective <objective> --executor-kind agent_team
target/debug/harness team-run create --mission-id <mission> --wave-id <wave> \
  --objective <objective> --member name:role:provider
target/debug/harness wave gate --id <wave> --status accepted \
  --run-id <completed-run> --accepted-by <actor> --outcome <summary>
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
- Any feature claim about Agent Team behavior must be backed by linked run,
  member, assignment/correlation, explicit action/outcome, and Wave-gate state.
- When the current workflow feels slow or manual, record a follow-up Wave or
  issue instead of normalizing hidden local reasoning.
- Prefer the progression `doc -> skill -> schema -> CLI/API -> dashboard ->
  plugin`. A plugin is justified only after the object contracts and commands
  are stable enough to reduce variance.
- The Agent Dashboard is the operator view for harness state. Product dashboards
  for adapted projects remain separate.

## Staged Acceptance

Every non-trivial native Wave is accepted in four small stages:

1. Context: Mission, Wave objective, executor kind, exit criteria, permissions,
   and risk are clear.
2. Execution: the selected executor owns its internal plan and emits its honest
   run records. Agent Team lanes start from assignment messages.
3. Outcome: explicit checks, artifacts, blockers, handoffs, and review results
   needed for this Wave are recorded. Review depth is proportional to risk.
4. Gate: the Host records `accepted | revise | blocked`; acceptance names one
   completed attempt and preserves all earlier attempts.

Company-level acceptance is separate: a WorkItem must preserve source/result
provenance and responsibility, sensitive actions must satisfy their Approval
policy, and durable effects must update their related document and typed
records. An accepted Wave alone does not approve a payment, legal submission,
permission change, or organization change.

## What Counts As Done

A native Mission/Wave slice is done only when the store can explain:

- why the work existed;
- which Wave and executor were selected;
- which run attempts occurred and which one was accepted;
- which TeamMessages assigned or handed off Agent Team lanes;
- which explicit outcomes, checks, and artifacts support acceptance;
- what the Wave gate accepted, revised, or blocked;
- what should be reused, improved, split, or followed up next.

If a future agent cannot reconstruct the answer from repository files and
native harness state, the work is not fully accepted.
