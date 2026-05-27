# MVP

The MVP is the first evidence that Multi-Agent Harness can manage real work
through its own protocol. It is not accepted by having docs, schemas, or a
Dashboard alone. It is accepted when the harness can use those pieces together
to run a non-fake workflow.

## Required Pilots

The MVP has two pilots, in this priority order:

1. self-hosting development for this repository;
2. LetMeTry / Earning Engine strategy-matrix iteration through a project
   adapter.

Both pilots must use the same generic loop:

```text
Goal -> GoalDesign -> Task Graph -> Message -> AgentMember work
  -> Evidence -> Proposal -> Review -> Decision
  -> GoalEvaluation -> Follow-up Task or GoalCase
```

The pilots may use different domain tools and dashboards, but they must share
the same coordination objects, evidence rules, and decision trail.

Historical implementation notes from the current self-hosting path are kept as
a reusable case in
[self-hosting-mvp-runtime-hardening-20260527](../examples/goal-cases/self-hosting-mvp-runtime-hardening-20260527/README.md).

## Pilot 1: Self-Hosting Development

The harness must manage its own development.

```mermaid
flowchart TD
  U[User request] --> L[Leader Agent]
  L --> GD[GoalDesign]
  GD --> T[Task]
  T --> M[Message kind=task]
  M --> A[AgentMember]
  A --> Repo[This repository]
  Repo --> Checks[pnpm check / cargo test]
  Checks --> E[Evidence]
  E --> P[Proposal]
  P --> R[Critic / Review]
  R --> D[Decision]
  D --> GE[GoalEvaluation]
  GE --> Follow[Follow-up task or GoalCase]
```

Minimum capabilities:

- create a goal and goal-design evidence;
- create a task with owner, assignee, reviewer, dependencies, workspace or
  owned-path policy, and acceptance criteria;
- assign through `Message(kind=task)`;
- run or record provider-backed member work;
- attach evidence from checks, diffs, provider sessions, review, logs, or
  Dashboard snapshots;
- create a proposal from diff or explicit changed paths;
- run critic/review gate and record Leader decision;
- produce goal evaluation and follow-up tasks.

Acceptance:

- a real repository change can be designed, assigned, delivered, proposed,
  reviewed, decided, and shown in Dashboard state without relying on chat
  history as the only state;
- generated evidence points to files, commands, logs, provider sessions, or
  review notes;
- stale docs, schema drift, missing evidence, provider failures, or missing
  ownership become tasks, blockers, or warnings.

## Pilot 2: LetMeTry Strategy Matrix Iteration

The harness must coordinate a real strategy system through an adapter without
coupling strategy logic into the generic core.

```mermaid
flowchart TD
  Goal[Long-term strategy matrix goal] --> L[Leader Agent]
  L --> Audit[Matrix audit task]
  Audit --> Curator[Matrix Curator]
  Audit --> Research[Strategy Research]
  Curator --> Nodes[DAG / Manifest strategy nodes]
  Research --> Adapter[LetMeTry / Earning Engine Adapter]
  Adapter --> Tools[Backtest / Live artifacts / Dashboard / Logs]
  Tools --> Reviews[Execution / Data / Dashboard / Parity reviews]
  Nodes --> Reviews
  Reviews --> C[Critic / Risk]
  C --> D[Decision: refine / kill / promote diagnostic live / create infra task]
  D --> Next[Next matrix or infrastructure task]
```

Minimum adapter capabilities:

- expose project CLI/API/dashboard/artifact commands through tool descriptors;
- link to strategy dashboard pages and artifacts as evidence;
- encode permission boundaries for live, wallet, order, and secret-touching
  actions;
- distinguish diagnostic evidence from promotion evidence;
- preserve backtest/live differences instead of hiding execution gaps;
- reference strategy nodes, parameters, lineage, and run history from the
  project source of truth;
- classify strategy problems by layer: strategy logic, execution lifecycle,
  market-data freshness, dashboard visibility, backtest/live parity, wallet or
  order safety, or missing tooling.

Acceptance:

- a Leader Agent can create matrix-level tasks such as audit strategy family,
  compare variants, diagnose quiet strategies, review no-fill behavior, inspect
  exits, or propose a new strategy;
- role-specific agents can inspect the same strategy family from strategy,
  execution, data, dashboard, parity, live-ops, critic, and knowledge angles;
- evidence includes DAG or manifest nodes, parameters, backtest/live artifacts,
  dashboard links, logs, screenshots, review summaries, or command outputs;
- the Leader can decide whether to refine, kill, promote bounded live, or
  create an infrastructure task based on evidence;
- strategy-specific logic stays in the LetMeTry project or adapter.

## Shared MVP Surfaces

| Surface | MVP role |
| --- | --- |
| Rust core | Defines first stable objects and state transitions. |
| File store | Persists goals, teams, members, runtimes, tasks, messages, events, proposals, evidence, provider sessions, and decisions locally. |
| CLI/API | Creates, reads, validates, and records the workflow objects. |
| Provider runtime | Backs persistent Agent Members and records delivery/evidence. |
| Skills | Teach agents how to operate the harness and project adapters. |
| Tool descriptors | Expose project capabilities without importing project code. |
| CI/CD | Verifies docs, schemas, fixtures, Rust checks, skill metadata, and stable workflow gates. |
| Agent Dashboard | Shows teams, member state, message delivery, runtime events, task Kanban, proposal state, evidence, review, decisions, and warnings. |

## Acceptance Gates

The MVP is accepted only when the repository can prove both the object protocol
and one self-hosted work loop.

| Gate | Accepted when | Does not pass |
| --- | --- | --- |
| Object contracts | Rust types, JSON schemas, fixtures, and docs agree for core objects. | A field exists only in code, docs, or a Dashboard view. |
| Goal design | GoalDesign exists before implementation assignment. | Retrospective chat explanation only. |
| Message delivery | A task message becomes delivered or failed with provider-session refs or explicit failure reason. | Success inferred from stdout or an assignee field. |
| Persistent member | `agent create`, `agent health`, `agent send`, `agent deliver`, and `agent close` operate on a durable member. | Only `codex exec`, dry-run delivery, or pid/socket checks. |
| Provider events | Provider notifications or fixtures become `AgentEvent` and report/evidence candidates. | Provider output remains raw transcript only. |
| Review gate | Accepted work includes proposal evidence, check evidence, critic findings, worker/provider output, path validation, and Leader decision. | Missing evidence ids, stale failed sessions, or unchecked path changes. |
| Dashboard read model | Dashboard/API shows tasks, members, runtimes, messages, provider sessions, proposals, evidence, decisions, and warnings. | A static page that cannot explain assignment, blockers, or evidence. |
| Goal learning | Goal evaluation and follow-up tasks or cases are produced when useful. | Final chat summary only. |
| Self-hosting dogfood | One real repo change passes through the full workflow. | Lead manually edits everything and only documents the intended flow. |
| Adapter pilot | Earning Engine adapter can drive one strategy-matrix decision or infrastructure task. | Adapter only lists commands with no evidence-backed decision path. |

Executable gates:

```bash
npx pnpm@9.15.4 acceptance:mvp
npx pnpm@9.15.4 acceptance:mvp:live
```

`acceptance:mvp` proves deterministic object protocol, review gate, Dashboard
API, hook bridge, and adapter surface. `acceptance:mvp:live` adds real
persistent Codex delivery plus Worker/Critic multi-member dogfood and is the
gate for claims that the harness is using live AgentMembers.

## Current Build Order

```text
object contracts
  -> file store and CLI
  -> provider runtime and delivery
  -> evidence / proposal / review gate
  -> Dashboard read model and warnings
  -> self-hosting dogfood
  -> Earning Engine adapter pilot
  -> goal evaluation and reusable cases
```

## Non-Goals For MVP

- No full workflow DSL.
- No generic strategy engine.
- No plugin before CLI/API/schema contracts stabilize.
- No live trading automation without explicit permission gates.
- No replacement for LetMeTry's strategy dashboard or backtest engine.

## Completion Criteria

The MVP is complete when the same harness can:

1. manage a real change to `multi-agent-harness` through goal, task, message,
   provider/session evidence, proposal, review, decision, and goal evaluation;
2. use the LetMeTry / Earning Engine adapter to drive strategy-matrix
   iteration from long-term goal to evidence-backed strategy or infrastructure
   decision;
3. show both flows in the Agent Dashboard or equivalent structured read model;
4. run CI gates that verify the contracts used by both flows;
5. produce follow-up tasks from missing evidence, failed checks, rejected
   proposals, or strategy findings.
