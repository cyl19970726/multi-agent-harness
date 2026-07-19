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
or forwarded to peers. Current v0 durable `thinking` actions are a migration
gap, not an approved product contract.

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

## Current Self-hosting Compatibility Objects

The runtime and store still use `Goal`, `GoalDesign`, `GoalPhase`, `Task`,
`Message`, `Evidence`, `Proposal`, `Decision`, and `GoalEvaluation`. They remain
the canonical coordination state for work in this repository until the staged,
non-destructive Mission/Wave migration in ADR 0026 is implemented and accepted.
The stricter chain below is repository governance; it is not a mandatory
product object graph for every Wave:

```text
Goal -> GoalDesign -> Task/Message -> Evidence -> Proposal
  -> Critic/Gate -> Decision -> GoalEvaluation
```

Current object responsibilities:

- `Goal`: the durable user or product objective. A goal is not complete until
  it has evidence-backed acceptance or an explicit blocker.
- `GoalDesign`: the Lead's plan for scenario, non-goals, permissions, infra,
  agent team, task graph, evidence, and evaluator gates.
- `AgentTeam`: the role composition for the goal. Teams should be designed from
  the scenario, not copied blindly from a template.
- `AgentMember`: a persistent or logically durable agent instance with id,
  name, role, prompt, skills, runtime state, current task, and provider session
  history.
- `GoalPhase`: a transitional sequential checkpoint inside a Goal. A phase chooses one
  primary executor: `task_graph` for durable Task/Message/AgentMember work, or
  `workflow` for direct WorkflowRun/WorkflowStep execution. It is not the same as
  the Starlark workflow `phase("...")` label and is not the future Wave model.
- `Task`: a unit of work owned by an agent, with dependencies, worktree/branch
  refs, owned paths, reviewer, and acceptance criteria.
- `Message`: the communication protocol. Assignment, handoff, review request,
  clarification, and report flow through messages so context is inspectable.
- `Evidence`: a file, command output, provider session, check, review note,
  screenshot, adapter artifact, or dashboard snapshot that supports a claim.
- `Proposal`: the implementation or decision candidate, usually backed by a
  diff, changed paths, checks, and evidence.
- `Decision`: the Leader/Gate outcome: accept, reject, split, block, kill,
  promote, or create follow-up work.
- `GoalEvaluation`: the evaluator's closeout explaining what worked, what
  failed, where the workflow helped, and what should become infra, docs, skill,
  schema, CLI, dashboard, adapter, or plugin work.
- `GoalCase`: a sanitized reusable example for future Lead Agents.

## Canonical Agent Members

For harness-managed work, the canonical agents are `AgentMember` records in the
harness store plus their `Message`, `Task`, `Evidence`, `Decision`, and
`ProviderSession` records.

External coding subagents or chat-side helpers may be used only as temporary
inputs. They do not count as harness execution unless their output is recorded
through a harness `AgentMember` report message and evidence refs.

Do not claim a task was run by the multi-agent harness unless the store shows:

- an assigned `Task`;
- a role-specific `AgentMember`;
- a `Message(kind=task)` from the Lead before implementation;
- a `Message(kind=report)` from the assignee;
- evidence refs for claims;
- critic/evaluator or review output before acceptance;
- a Leader `Decision`.

## How To Develop This Repository With The Harness

The Lead Agent should use this sequence for every non-trivial change:

1. Inspect current state:
   `target/debug/harness goal list`, `target/debug/harness task list`,
   `target/debug/harness agent list`, and relevant docs.
2. Define the Mission, its ordered Waves, each Wave's executor, and its gate.
   When `executor_kind=agent_team`, define only the roles, permissions, model
   tiers, depth, owned surfaces, and artifacts that Wave needs.
3. Until first-class Mission/Wave storage exists, record this intent through the
   current Goal design fields as compatibility data. Do not create GoalPhase or
   a Task Graph merely to describe the new product model.
4. When repository self-hosting governance needs durable ownership, create the
   smallest necessary compatibility Tasks with explicit assignee, reviewer,
   owned paths, and acceptance. Agent Team product ownership itself is expressed
   through assignment-message correlation.
5. Assign work through `task assign` or `agent send`; do not treat a private
   chat instruction as an assignment.
6. For concurrent code work, give each implementation task a separate worktree
   or clearly disjoint owned paths. Shared-file conflicts must be escalated to
   the Lead before merging.
7. When a claim depends on real provider behavior, use persistent Codex
   `AgentMember` runtimes and `agent deliver`; one-shot helper output is not
   enough.
8. Attach evidence with `evidence add`: checks, logs, provider sessions,
   dashboard snapshots, diffs, review notes, adapter artifacts, or screenshots.
9. Create a proposal from a diff or explicit changed paths before acceptance.
10. Run the review gate. Acceptance requires check evidence, worker or provider
    output, critic findings, valid evidence refs, and owned-path compliance
    unless an explicit waiver decision records the exception.
11. Record the Leader decision and update task status.
12. At compatibility Goal close, add `goal_evaluation` evidence and create follow-up tasks or
    a reusable GoalCase when the run teaches a reusable workflow pattern.

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
not be used to plan new work. Current Goal/GoalPhase commands remain temporary
runtime compatibility surfaces until the Mission/Wave migration is complete.

Do not make Earning Engine or other domain skills mandatory for this
repository. Domain workflows enter through adapters and scenario-specific
skills; the generic harness core must stay domain-neutral.

Useful local commands:

```bash
target/debug/harness init
target/debug/harness goal learning-status --id <goal> --strict --require-evaluation
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

This repository must dogfood the workflow it is building.

- Do not bypass the harness for meaningful product, schema, CLI, dashboard,
  provider, adapter, or skill changes.
- A small typo or single-line doc fix may be Lead-local, but the final summary
  must say that it was a Lead-local exception.
- Any feature claim about multi-agent behavior must be backed by store-visible
  tasks, messages, provider sessions or reports, critic evidence, and a
  decision.
- When the current workflow feels slow or manual, create an infra task instead
  of normalizing hidden local reasoning.
- Prefer the progression `doc -> skill -> schema -> CLI/API -> dashboard ->
  plugin`. A plugin is justified only after the object contracts and commands
  are stable enough to reduce variance.
- The Agent Dashboard is the operator view for harness state. Product dashboards
  for adapted projects remain separate.

## Staged Acceptance

Every non-trivial goal managed through the current repository self-hosting
runtime must be accepted in stages:

1. GoalDesign acceptance: scenario, non-goals, infra gaps, team, task graph,
   evidence plan, evaluator plan, and risks are recorded before implementation.
2. Assignment acceptance: tasks are split by owner, dependencies, and owned
   paths; task messages are sent before work starts.
3. Implementation acceptance: each worker report has evidence and checks.
4. Review/Gate acceptance: critic or evaluator output exists before the Leader
   decision.
5. GoalEvaluation acceptance: goal close records what worked, what failed,
   missing infra, event-order health, follow-up tasks, and whether a GoalCase is
   needed.

Skipping a stage requires an explicit waiver decision with rationale, evidence,
owner, and follow-up task.

## Goal Learning Gate

For goals managed by the harness:

- `goal_design` evidence must exist before implementation tasks move forward;
- `goal_evaluation` evidence must exist before final close, or an explicit
  waiver decision must explain why not;
- final chat summaries are not durable evidence;
- Dashboard visibility is useful, but CLI/review-gate checks remain the source
  of acceptance truth.

## What Counts As Done

A goal or substantial task is done only when the harness store can explain:

- why the work existed;
- which scenario and workflow the Lead designed;
- which agents were responsible for which tasks;
- which messages assigned or handed off the work;
- what evidence supports each claim;
- what the Critic/Gate accepted, rejected, or questioned;
- what decision was made;
- what should be reused, improved, split, or followed up next.

If a future agent cannot reconstruct the answer from repository files and
harness state, the work is not fully accepted.
