---
name: generic-agent-harness
description: "Use when operating or extending a generic multi-agent harness: agent members, message-first task/report flow, claims, blockers, permissions, provider sessions, tool descriptors, and Agent Dashboard evidence."
---

# Generic Agent Harness

Use this skill when the work is about the multi-agent product itself, or when a
Lead Agent needs to use the harness to make a project or business domain
agent-operable.

The harness is not only a record of agent work. It is the workflow that turns a
goal and its domain scenario into infrastructure, agent team design, task graph
execution, evidence, review, decisions, and follow-up requirements.

## First Step

Read only what the task needs:

- Product requirements and scenarios: `docs/prd.md`
- Architecture: `docs/architecture.md`
- Goal learning loop: `docs/goal-learning-loop.md`
- Operations and CI: `docs/operations.md`
- Schemas and minimal object contracts: `docs/schemas.md`
- Decisions: `docs/decisions.md`

## Rules

- The Lead Agent must start from the goal and scenario, not from an isolated
  task.
- Before assigning work, define the scenario workflow, missing infra, agent
  team, task graph, and acceptance gates.
- Do not complete domain work locally and then backfill task/message/evidence
  records as if agent members had driven the work.
- External coding subagents or chat helpers are not canonical execution. Their
  output counts only after a harness `AgentMember` records a report message and
  evidence refs.
- Treat project systems as tools behind adapters.
- Do not put domain logic in the generic core.
- Use `AgentMessage` for task assignment, reports, follow-up questions, and
  handoff.
- Materialize messages into `Task`, `Report`, `Claim`, `Blocker`, or
  `Decision` before using them for gates.
- Keep provider chat below message/report artifacts in the trust order.
- Require explicit permission grants for live, money-moving, destructive, or
  secret-touching actions.

## Lead Workflow

For every goal that enters harness management, run this sequence before
implementation:

```text
Goal
  -> GoalDesign
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
  -> follow-up tasks
```

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
- task messages from the Lead to the assignee before member reports and leader
  decisions;
- reports back from the assignee;
- evidence refs for claims;
- critic or reviewer output for non-trivial decisions;
- a Leader decision that records accept, revise, block, split, or follow up.

The Agent Dashboard should expose this chain without requiring raw JSON reads.

Reject the task as not harness-operated when any of these are true:

- missing assignment message;
- assignment message created after the report or decision;
- missing member report for a non-trivial worker claim;
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
```

Use `acceptance:mvp:quick` while iterating on non-static changes, and
`acceptance:mvp:live` when the claim includes real persistent Codex
AgentMember delivery. A passing quick run proves the object protocol, review
gate, dashboard read model, hook bridge, and adapter surface. It does not prove
trusted plugin activation or live provider delivery unless the live gate runs.
The live gate must include both a single-member smoke and a Worker/Critic
multi-member dogfood task before claiming the harness can use live
AgentMembers.

When the gate exposes a skipped or failed stage, create a follow-up task rather
than weakening the acceptance criteria.
