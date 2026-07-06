# Product Requirements

## Vision

Star Harness turns a project or business domain into an
agent-operable and agent-progressable system.

The product is not an agent runner and not a project-specific engine. It is a
coordination layer for persistent Agent Teams. A standing team can observe
project state, propose goals, prioritize work, manage a task graph, collaborate
through messages, produce evidence, make reviewed decisions, evaluate the
workflow, and continue into the next goal.

The core product loop is:

```text
persistent AgentTeam
  -> observe project state and prior goal cases
  -> propose / prioritize goals
  -> scenario and infra design
  -> task graph
  -> message-driven member collaboration
  -> evidence -> proposal -> review -> decision
  -> goal evaluation -> follow-up task or next goal
```

The smaller execution loop inside one accepted goal is:

```text
scenario -> missing infra -> agent team -> task graph -> message execution
  -> evidence -> review -> decision -> goal learning
```

The harness succeeds only when a human or future agent can reconstruct why work
existed, who owned it, how it was assigned, what evidence was produced, what
decision was made, and what should improve next.

## Product Thesis

Modern projects already expose valuable capability through CLI commands, APIs,
dashboards, artifacts, logs, tests, and domain-specific tools. Raw agents can
call those tools, but they often see too much unstructured context and too
little structured feedback.

The harness makes those capabilities usable by agents through:

- stable project adapters and tool descriptors;
- scenario-specific skills;
- durable agent teams, agent members, and task graphs;
- message-first assignment and reporting;
- peer-to-peer agent communication;
- evidence-backed proposals, reviews, and decisions;
- Dashboard visibility;
- goal evaluation, reusable cases, and agent-proposed follow-up goals.

The Lead Agent is therefore an organization and decision owner before it is an
executor. It accepts or rejects proposed goals, designs the workflow, delegates
through messages, and records decisions. It should not be the only source of
new work once a standing team is operating.

The default standing team should include an `Observer` role when the goal is
long-running. The Observer watches repository state, Dashboard warnings, CI
results, adapter outputs, prior GoalCases, and stale task graphs. Its output is
not a final decision; it produces proposed goals, blockers, graph-change
proposals, or follow-up tasks for the Lead and reviewers to accept, reject, or
prioritize.

For each goal, the Lead must decide:

- which scenario is being operated;
- which missing CLI, adapter, skill, Dashboard, or CI/CD surface would shorten
  future work;
- which agent members are needed and what each owns;
- which tasks can run in parallel and which require worktree or PR boundaries;
- what evidence proves the work happened through the harness;
- which critic, reviewer, or gate decides whether the result is acceptable.
- which agent-proposed follow-up goals should enter the backlog.

## Final Acceptance

The product is accepted when it can manage two real pilots with the same
generic workflow and a standing team that continues across multiple tasks:

1. self-hosting development of this repository;
2. LetMeTry / Earning Engine strategy-matrix iteration through an adapter.

Both pilots must use the same core chain:

```text
Standing AgentTeam -> Proposed Goal -> GoalDesign -> Task Graph
  -> Message -> AgentMember work
  -> Evidence -> Proposal -> Critic/Review -> Decision
  -> GoalEvaluation -> Follow-up Task or GoalCase
```

The self-hosting pilot has first priority. The product must prove it can
develop its own docs, schemas, CLI, CI/CD, provider integration, and Dashboard
before it can reliably coordinate another project's strategy work.

A create-deliver-close smoke test is not final acceptance. It can prove
provider transport, but it does not prove a team can keep working. Final
acceptance requires durable members that return to idle, receive more messages,
collaborate with peers, and produce or prioritize follow-up goals without being
recreated as job runners.

Detailed MVP gates are in [mvp.md](mvp.md).

## Critical Mechanisms

The product vision depends on these mechanisms working together:

| Mechanism | Key question | Canonical doc |
| --- | --- | --- |
| Goal and learning loop | Why does the work exist and how does the system learn from it? | [goal-learning-loop.md](goal-learning-loop.md) |
| Concept model | What do Goal, Task, Message, AgentMember, Evidence, Proposal, and Decision mean? | [concept-model.md](concept-model.md) |
| Data model | What is source of truth and what is only projection? | [data-model.md](data-model.md) |
| Core modules | Which modules exist because the vision would fail without them? | [core-modules.md](core-modules.md) |
| Agent runtime | How do persistent provider-backed members receive work and emit events? | [agent-runtime.md](agent-runtime.md) |
| Agent control plane | How are lifecycle, queues, peer messages, and reductions operated? | [agent-control-plane.md](agent-control-plane.md) |
| Dashboard | What must the user see to know the workflow really happened? | [dashboard.md](dashboard.md) |
| Git / PR workflow | How do worktrees, branches, PRs, proposals, reviews, and decisions relate? | [workflow-git-pr.md](workflow-git-pr.md) |
| Provider integration | How can Codex and future providers implement the same runtime contract? | [integration/README.md](integration/README.md) |
| Decisions | Which hard-to-reverse tradeoffs must future agents preserve? | [decisions/README.md](decisions/README.md) |

## Scenarios

### Persistent Autonomous Team Operation

The central scenario is a long-lived team that can keep moving after one task
or one user turn ends.

```mermaid
flowchart TD
  Team[Standing AgentTeam] --> Observe[Observe repo, adapter, dashboard, prior cases]
  Observe --> Gap[Gap / opportunity / blocker]
  Gap --> Proposed[Proposed Goal]
  Proposed --> Lead[Lead prioritizes / accepts / rejects]
  Lead --> Design[GoalDesign]
  Design --> Graph[Task Graph]
  Graph --> Msg[Task and peer messages]
  Msg --> Members[Persistent AgentMembers]
  Members --> Evidence[Reports and evidence]
  Evidence --> Review[Critic / Review]
  Review --> Decision[Leader Decision]
  Decision --> Eval[GoalEvaluation]
  Eval --> Next[Follow-up task or next goal]
  Next --> Observe
```

This is what distinguishes the product from provider subagents. The team has a
stable identity, shared task graph, durable mailboxes, persistent members, and
an evaluation loop that generates the next work.

### Self-Hosting Development

The harness must develop itself through its own protocol.

```mermaid
flowchart TD
  U[User request] --> L[Leader Agent]
  L --> GD[GoalDesign]
  GD --> TG[Task Graph]
  TG --> M[Message kind=task]
  M --> A[AgentMember]
  A --> Repo[Repository / CLI / Tests]
  Repo --> E[Evidence]
  E --> P[Proposal]
  P --> R[Critic / Review]
  R --> D[Decision]
  D --> GE[GoalEvaluation]
  GE --> F[Follow-up Task or GoalCase]
```

This scenario proves that the harness is not just documentation. It must use
durable agent members, message delivery, provider sessions, proposals, review
gates, decisions, and Dashboard visibility for real repository work.

### Project Adapter Operation

The harness must operate external projects without importing their domain logic
into the generic core.

```mermaid
flowchart TD
  G[Project goal] --> L[Leader Agent]
  L --> S[Scenario workflow]
  S --> Adapter[Project adapter]
  Adapter --> CLI[CLI / API / Dashboard / Artifacts]
  CLI --> E[Evidence]
  E --> Review[Domain review / Critic]
  Review --> D[Decision]
  D --> Next[Strategy or infrastructure task]
```

The first adapter pilot is LetMeTry / Earning Engine strategy-matrix work.
Strategy-specific logic, wallet/order safety, backtests, live artifacts, and
domain dashboards stay in the project or adapter. The generic harness owns
coordination, evidence, decisions, and follow-up work.

### Self-Improving Workflow

Every significant goal should produce learning.

```mermaid
flowchart TD
  D[Decision] --> Eval[GoalEvaluation]
  Eval --> Gap[Gap or reusable lesson]
  Gap --> Task[Follow-up Task]
  Gap --> Case[GoalCase]
  Task --> Better[Better CLI / skill / schema / dashboard / CI]
  Better --> Next[Next goal]
```

Repeated manual effort should become infrastructure. Repeated confusion should
become docs, ADR, schema, skill, Dashboard visibility, or CI/CD.

## Non-Goals

- Do not build project-specific business logic into the generic core.
- Do not make a large workflow DSL before the task/message/evidence loop works.
- Do not treat provider chat or transcripts as canonical state.
- Do not claim multi-agent execution from one-shot helper output unless it is
  recorded through harness messages, evidence, and decisions.
- Do not claim autonomous team acceptance from create-agent, deliver-one-turn,
  close-agent smoke tests.
- Do not make the Agent Dashboard replace project-specific dashboards.
- Do not use docs as the source of truth for stable fields, commands, runtime
  state, or checks.

## Acceptance Summary

The product is useful when:

- a Lead Agent can turn a goal into scenario workflow, infra gaps, agent team,
  task graph, evidence plan, and reviewer gates;
- standing teams can propose useful goals or follow-up tasks from observed
  gaps, evaluations, or dashboard warnings;
- an Observer or equivalent role continuously scans project state and turns
  drift, stale warnings, repeated manual work, or new opportunities into
  reviewable proposals;
- work is assigned through `Message(kind=task)`, not hidden chat or field-only
  mutation;
- persistent Agent Members can receive multiple messages over time, report,
  return to idle, and be observed;
- agents can communicate peer-to-peer through durable messages when work needs
  clarification, handoff, or critique;
- proposals are backed by evidence and reviewed before Leader decisions;
- Dashboard views show goals, tasks, teams, messages, runtime health, evidence,
  proposals, reviews, decisions, and goal-learning warnings;
- completed goals produce evaluations and reusable cases or follow-up tasks;
- stable commitments are verified by schema, CLI/API, Dashboard, CI/CD, or
  skills rather than prose alone.
