# Agent Operating Rules

This repository builds Star Harness itself. Work in this repo must use
the harness objects as the canonical coordination state.

## Product We Are Building

Star Harness gives resident Host Agents such as Codex, Claude Code, and Kimi
Code provider-neutral tools for structured execution and collaboration. The
canonical product hierarchy is:

```text
Mission -> ordered Wave -> executor
  executor = agent_team | dynamic_workflow | host
```

`Mission` is durable intent. `Wave` is a lightweight ordered unit with an
objective, executor, outcome, artifacts, and gate. A Wave does not own or
require a Task Graph. Agent Team uses assignment-message correlation for member
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

Future Standing Agents + Docs will build long-lived business operations on the
same tools. They are not part of the current Mission/Wave implementation slice.

For this repository, the first product scenario is self-hosting: the harness
must be able to develop, evaluate, and improve itself through its own objects.
The second scenario is project adaptation, starting with the LetMeTry / Earning
Engine strategy-matrix workflow. Project-specific logic belongs in adapters and
skills, not in the generic harness core.

## Native And Compatibility Objects

`Mission` and `Wave` are the native coordination objects for new work. Existing
`Goal`, `GoalDesign`, `GoalPhase`, `Task`, `Message`, `Evidence`, `Proposal`,
`Decision`, and `GoalEvaluation` ledgers remain readable compatibility and
optional governance surfaces; they are not prerequisites for a new Wave.

Compatibility reads are intentionally asymmetric:

- an existing Goal may appear as a provenance-marked, read-only Mission
  projection;
- a GoalPhase is never converted into a Wave, because its Task Graph and gate
  semantics are different;
- old JSONL is not rewritten by native Mission/Wave operations.

For `executor_kind=agent_team`, the canonical execution records are
`AgentTeamRun`, `MemberRun`, `TeamMessage`, explicit `MemberAction` summaries,
artifacts, and the Wave gate. Assignment ownership is proven by
`TeamMessage(kind=assignment)` plus `correlation_id`, not by a Task Graph.

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
3. Do not create GoalPhase or a Task Graph merely to describe a Mission/Wave or
   Agent Team. An executor may use its own internal plan.
4. For Agent Team work, create the linked TeamRun, then use its Assignment
   messages and correlations for lane ownership. Give concurrent members
   disjoint owned paths or worktrees and surface shared-file conflicts to the
   Host.
5. Keep explicit actions, checks, artifacts, blockers, handoffs, reviews, and
   outcomes durable. Do not persist provider thinking.
6. Apply review proportional to risk. A reviewer member or stricter repository
   governance may be added when useful, but Proposal/Decision/GoalEvaluation is
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

The retired `generic-agent-harness`, `star-goal`, and `star-planner` skills must
not be used to plan new work. Goal/GoalPhase commands are legacy compatibility
surfaces and must not be used as the default for a new Mission.

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
npx pnpm@9.15.4 acceptance:mvp
npx pnpm@9.15.4 acceptance:mvp:live
```

`acceptance:mvp` proves deterministic object protocol, review gate, dashboard
read model, hook bridge, and adapter surface. `acceptance:mvp:live` is required
before claiming real live Codex AgentMember usage; it must include both the
single-member smoke and the Worker/Critic live dogfood gate.

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

Legacy Goal learning/evidence gates may still be used by compatibility flows or
special governance, but they are not prerequisites for native Wave acceptance.

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
