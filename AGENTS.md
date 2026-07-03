# Agent Operating Rules

This repository builds Star Harness itself. Work in this repo must use
the harness objects as the canonical coordination state.

## Product We Are Building

Star Harness is a goal-task-multi-agent development system. Its purpose
is to turn a high-level goal into an executable, reviewable, reusable workflow:

```text
Goal -> GoalDesign -> AgentTeam -> TaskGraph -> Message -> AgentMember work
  -> Evidence -> Proposal -> Critic/Gate -> Decision -> GoalEvaluation
  -> GoalCase / Follow-up Task
```

The product is not just a wrapper around coding agents. It must help a Lead
Agent understand a project scenario, decide what infra is missing, design the
right team, assign work through durable messages, observe execution, verify
claims with evidence, and convert every useful lesson into future workflow
improvements.

For this repository, the first product scenario is self-hosting: the harness
must be able to develop, evaluate, and improve itself through its own objects.
The second scenario is project adaptation, starting with the LetMeTry / Earning
Engine strategy-matrix workflow. Project-specific logic belongs in adapters and
skills, not in the generic harness core.

## Core Objects And Responsibilities

- `Goal`: the durable user or product objective. A goal is not complete until
  it has evidence-backed acceptance or an explicit blocker.
- `GoalDesign`: the Lead's plan for scenario, non-goals, permissions, infra,
  agent team, task graph, evidence, and evaluator gates.
- `AgentTeam`: the role composition for the goal. Teams should be designed from
  the scenario, not copied blindly from a template.
- `AgentMember`: a persistent or logically durable agent instance with id,
  name, role, prompt, skills, runtime state, current task, and provider session
  history.
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

1. Load the required Lead skill:
   `.agents/skills/generic-agent-harness/SKILL.md`.
2. Inspect current state:
   `target/debug/harness goal list`, `target/debug/harness task list`,
   `target/debug/harness agent list`, and relevant docs.
3. Create or reuse a goal. If the goal is new, record `goal_design` evidence
   before assigning implementation tasks.
4. Design the team. At minimum, substantial work needs a Lead, Worker, and
   Critic/Gate. Add Dashboard, Schema, Provider, Adapter, or Docs agents only
   when the scenario needs those roles.
5. Create tasks with explicit owner, assignee, reviewer, dependencies, owned
   paths, workspace or worktree refs, and acceptance criteria.
6. Assign work through `task assign` or `agent send`; do not treat a private
   chat instruction as an assignment.
7. For concurrent code work, give each implementation task a separate worktree
   or clearly disjoint owned paths. Shared-file conflicts must be escalated to
   the Lead before merging.
8. When a claim depends on real provider behavior, use persistent Codex
   `AgentMember` runtimes and `agent deliver`; one-shot helper output is not
   enough.
9. Attach evidence with `evidence add`: checks, logs, provider sessions,
   dashboard snapshots, diffs, review notes, adapter artifacts, or screenshots.
10. Create a proposal from a diff or explicit changed paths before acceptance.
11. Run the review gate. Acceptance requires check evidence, worker or provider
    output, critic findings, valid evidence refs, and owned-path compliance
    unless an explicit waiver decision records the exception.
12. Record the Leader decision and update task status.
13. At goal close, add `goal_evaluation` evidence and create follow-up tasks or
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

## Required Skills For Lead

The Lead Agent must load `.agents/skills/generic-agent-harness/SKILL.md` before
planning or accepting non-trivial work in this repository. It is the operating
contract for the product itself: goal design, task graph design, message-first
assignment, provider sessions, evidence, critic review, decisions, goal
evaluation, and follow-up tasks.

Load `skills/bootstrap-project-workflow/SKILL.md` only when the goal is about
project bootstrapping or governance: docs and CI/CD design, directory reorg,
new requirement workflow design, adapter boundaries, skill design, task-system
design, or migrating a project into a harness-operable shape.

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

Every non-trivial goal must be accepted in stages:

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
