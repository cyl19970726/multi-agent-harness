---
name: generic-agent-harness
description: "Use when operating or extending a generic multi-agent harness: agent members, message-first task/report flow, claims, blockers, permissions, provider sessions, tool descriptors, and Agent Dashboard evidence."
---

# Generic Agent Harness

Use this skill when the work is about the multi-agent product itself, or when a
Lead Agent needs to use the harness to make a project or business domain
agent-operable.

The harness is not only a record of agent work. It is the workflow that lets a
persistent team observe a project, propose goals, adjust a task graph, execute
through messages, produce evidence, review decisions, evaluate results, and
keep moving into the next goal.

## First Step

Read only what the task needs:

- Product requirements and scenarios: `docs/prd.md`
- Object relationships and anti-drift invariants: `docs/concept-model.md`
- Architecture: `docs/architecture.md`
- Core module purpose and boundaries: `docs/core-modules.md`
- Data model source-of-truth rules: `docs/data-model.md`
- Provider-neutral runtime: `docs/agent-runtime.md`
- Multi-project store and project switching: `docs/multi-project.md`
- Dashboard information architecture: `docs/dashboard.md`
- Git/PR workflow: `docs/workflow-git-pr.md`
- Goal learning loop: `docs/goal-learning-loop.md`
- Operations and CI: `docs/operations.md`
- Schemas and minimal object contracts: `docs/schemas.md`
- Decisions: `docs/decisions/README.md`

## Rules

- The Lead Agent must start from the goal and scenario, not from an isolated
  task.
- Before assigning work, define the scenario workflow, missing infra, agent
  team, task graph, and acceptance gates.
- Treat `AgentTeam` as a standing organization when the scenario is
  long-running. Do not design create-deliver-close job runners and call that
  autonomous teamwork.
- Include an Observer or equivalent role for long-running goals. Observer
  watches Dashboard warnings, CI, stale tasks, adapter evidence, prior cases,
  and repeated manual work, then proposes goals, blockers, graph changes, or
  follow-up tasks.
- Do not complete domain work locally and then backfill task/message/evidence
  records as if agent members had driven the work.
- External coding subagents or chat helpers are not canonical execution. Their
  output counts only after a harness `AgentMember` records a report message and
  evidence refs.
- Treat project systems as tools behind adapters.
- Do not put domain logic in the generic core.
- Use `AgentMessage` for task assignment, reports, follow-up questions, and
  handoff.
- A task assignment is not proven by directly setting `assignee_agent_id`.
  Create or reference the task, send `Message(kind=task)`, then treat task and
  member assignment state as projections of that delivered message.
- Tasks attach to a goal phase via `phase_id` — the single validated join key
  (the legacy free-text `Task.phase` field was retired). The dashboard groups
  tasks under Goal -> phase -> [Task Graph | Task Kanban], not a flat board.
- Goals, tasks, and stores are project-scoped: each project has its own store
  under `~/.harness/projects/<id>/` (the `store_root`, distinct from the
  project's `project_root` working tree). Use `harness project add|list|switch|
  migrate` to register and select projects rather than juggling `--store`.
- Materialize messages into `Task`, `Report`, `Claim`, `Blocker`, or
  `Decision` before using them for gates.
- Keep provider chat below message/report artifacts in the trust order.
- Require explicit permission grants for live, money-moving, destructive, or
  secret-touching actions.
- Closing an `AgentMember` is retirement, handoff, or cleanup. It is not the
  normal successful end of one task.

## Lead Workflow

For every goal that enters harness management, run this sequence before
implementation:

```text
standing team observes project state
  -> proposed goal / blocker / graph change
  -> Lead accepts, rejects, prioritizes, or requests evidence
Goal
  -> design_md (synthesized from the knowledge[] ledger) + acceptance_md
  -> scenario understanding
  -> scenario workflow
  -> infra gaps: CLI + skill + adapter + dashboard + CI/CD
  -> agent team design
  -> task graph design
  -> message-driven assignment
  -> report + evidence
  -> critic / gate
  -> leader decision
  -> GoalEvaluation
  -> GoalCase when reusable
  -> follow-up tasks or proposed next goals
```

A `Goal` now carries `design_md` (the synthesized design — key problems first,
then approach — built from the append-only `knowledge[]` ledger) and
`acceptance_md` (the real acceptance, written BEFORE work). Together these
absorb the legacy `GoalDesign` field soup; `GoalDesign` survives only as a
back-compat typed record, not the authoring surface.

Write down the result in tasks, messages, evidence, or decisions. Do not rely
on hidden chat context.

The event order matters. The normal path is:

```text
task created
  -> task message assigned
  -> member report message
  -> evidence refs attached
  -> critic / review output
  -> leader decision
```

If a direct store update bypasses this order, record it as a workflow defect
and create a follow-up task to move the behavior behind CLI/API validation.

### Phased goal execution

A goal is executed as agent-planned, SEQUENTIAL `phases[]`, each owning a task
DAG, gated by the phase's `acceptance`. The append-only `knowledge[]` ledger is
the source of truth for progress; `goal design-synthesize` rebuilds `design_md`
from it; and once `phases[]` is non-empty the goal's `stage` is a DERIVED
projection of the phases (forward-only), not a field you set by hand.

The command seam:

- `harness goal plan <goal>` — a planner agent decomposes `design_md` +
  `acceptance_md` into the phase/task DAG (capped replan loop).
- `harness goal run-phases <goal>` — execute the phases in order; `--resume`
  re-enters a `Running` checkpoint and reuses succeeded leaves, and
  `--max-phase-retries <n>` bounds per-phase retries.
- `harness goal reconcile-phase` — true up a phase whose work landed out of
  band.
- `harness goal finalize [--force]` — close the goal; the last phase/task
  finishing already auto-finalizes (the derivation runs on every completion
  seam), so `finalize` is the explicit/forced path.

Phased execution is workflow-backed: `goal run-phases` compiles each phase's
task DAG to a `.star` program (`compile_phase_to_starlark`) and runs it on the
SAME runtime the `star-workflow` skill documents — and a passing phase's
writable diffs LAND on the branch (per-phase landing commit). See the
`star-workflow` skill for the runtime, flags, and structured-output contract.

If the Lead must do a blocking step locally, record it as a `leader-local
exception` with the reason, evidence, and follow-up task that should turn the
manual step into CLI, skill, adapter, dashboard, or CI.

Skipping GoalDesign, assignment, review, or GoalEvaluation requires an explicit
waiver decision. A valid first-version waiver is selected with
`--waiver-decision <id>`, has resolving evidence ids, is attached to an owned
task in the same goal, and names a real follow-up task. A bare
`--allow-*` flag is not enough.

At goal close, require an evaluator or critic pass. The evaluator checks what
worked, what failed, whether the event order was real, which infra was missing,
and whether the goal should produce a reusable case under `examples/goal-cases`.
In the first version this is represented as `goal_evaluation` evidence; after
the fields stabilize it should become a review-gate requirement.

## Agent Team Design

For each goal, define only the members the scenario needs. Each member needs:

- role and responsibility;
- allowed tools and forbidden actions;
- expected evidence;
- owned paths or project surfaces;
- reviewer or critic relationship.

For standing teams, define the ongoing role mix as well as task-specific
assignees. A useful self-hosting team normally has:

- Lead: prioritizes and records decisions;
- Observer: proposes goals, blockers, and graph changes from system state;
- Implementer: changes code or docs in owned paths;
- Critic/Reviewer: challenges evidence and acceptance;
- Dashboard/Runtime/Domain specialists when the scenario needs them.

If the work changes files concurrently, split tasks by owned paths and assign
separate worktrees or PR boundaries.

## Infra Design

The Lead must ask which repeated work should become infrastructure:

- CLI for shortest executable paths and JSON outputs;
- skill for agent operating procedure;
- adapter for project-specific commands, permissions, and evidence policy;
- dashboard for shared state and review visibility;
- CI/CD for stable commitments and regressions.

If a task repeatedly requires manual log reading, ad hoc screenshots, or hidden
local reasoning, create an infra-improvement task instead of treating the
manual step as normal.

## Acceptance

A task is not proof that the harness was used. To prove harness usage, require:

- a goal or parent task;
- role-specific agent members or an explicit leader-local exception;
- for autonomous-team claims, a standing team with durable members reused
  across multiple messages, tasks, or goals;
- Observer or equivalent proposals for new goals, blockers, graph changes, or
  follow-up tasks when the work is long-running;
- task messages from the Lead to the assignee before member reports and leader
  decisions;
- peer messages when clarification, critique, or handoff happens between
  members;
- reports back from the assignee;
- evidence refs for claims;
- critic or reviewer output for non-trivial decisions;
- a Leader decision that records accept, revise, block, split, or follow up.

The Agent Dashboard should expose this chain without requiring raw JSON reads.

Reject the task as not harness-operated when any of these are true:

- missing assignment message;
- assignment message created after the report or decision;
- missing member report for a non-trivial worker claim;
- create-deliver-close smoke is presented as autonomous AgentTeam acceptance;
- only Lead-local evidence supports the conclusion;
- provider chat is the only source of truth;
- critic/reviewer output is missing for promotion, live, money-moving,
  destructive, or cross-module decisions;
- domain logic is moved into the generic core instead of an adapter.

## Progressive MVP Gate

For self-hosting development, prefer the executable gate before claiming the
MVP is working:

```bash
npx pnpm@9.15.4 acceptance:mvp
npx pnpm@9.15.4 acceptance:autonomous-team
```

Use `acceptance:mvp:quick` while iterating on non-static changes, and
`acceptance:mvp:live` when the claim includes real Codex provider delivery. A
passing quick run proves the object protocol, review gate, dashboard read
model, hook bridge, and adapter surface. The current live gate proves provider
transport smoke only. Do not claim autonomous persistent team acceptance until
the gate also proves durable member reuse, idle-to-next-message delivery,
peer-to-peer messages, Observer-generated proposals, and a Lead decision over
those proposals.

Use `acceptance:autonomous-team` when claiming the standing team loop works.
That gate must prove: durable team members, same-member multiple message
delivery, idle return, peer messages, GoalEvaluation, Observer next-round
proposal, Lead disposition, follow-up goal/task creation, and Dashboard
`autonomous_proposals` visibility. It must also execute an accepted generated
next-round task and create another accepted follow-up proposal; merely creating
one next-round task is not enough to prove self-evolution.

For runner/scheduler work, require the full lifecycle:
`Vision -> Goal -> GoalDesign -> TaskGraph -> GoalEvaluation/final acceptance
-> GoalClose -> Vision comparison -> NextGoalProposal -> Lead disposition ->
New GoalDesign/TaskGraph`. The runner must not schedule from a lone task
report. It should close only a goal whose task graph is done, whose strict goal
learning status is clean, and whose next-round proposal can cite the vision
context it is trying to advance.

When the gate exposes a skipped or failed stage, create a follow-up task rather
than weakening the acceptance criteria.
