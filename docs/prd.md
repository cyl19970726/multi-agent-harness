# Product Requirements

## Product Mission

Star Harness gives resident Host Agents such as Codex, Claude Code, and Kimi
Code provider-neutral tools for structured execution and collaboration.

The canonical hierarchy is:

```text
Mission -> ordered Wave -> executor
  executor = agent_team | dynamic_workflow | host
```

- `Mission` is durable intent and the desired-outcome container.
- `Wave` is a lightweight ordered unit with an objective, executor, outcome,
  artifacts, and a gate.
- The executor owns its internal planning. A Wave never requires a Task Graph.

Current `Goal`, `GoalPhase`, Task, Evidence, Proposal, Decision, and
GoalEvaluation objects remain self-hosting compatibility surfaces while the
non-destructive migration in [ADR 0026](decisions/0026-mission-wave-architecture.md)
lands. They are not the exported product model.

## Product Thesis

Resident agents already have provider-native execution and subagents. Star
Harness adds the tools they lack when work needs a durable outer structure:

- Mission/Wave outcome planning and gates;
- living Agent Team collaborators with messages, assignments, handoffs, and
  reviews;
- one-shot Dynamic Workflows with fan-out, retry, artifacts, and typed results;
- provider-neutral sessions, capabilities, permission ceilings, and budgets;
- plugins, MCP, CLI, hooks, and a shared Dashboard read model;
- explicit artifacts and outcome summaries instead of hidden transcript state.

This is not one universal Agent or Run abstraction. `WorkflowStep`,
`MemberRun`, Host-native subagent, and future Standing Agent have different
ownership and lifecycle semantics even when they reuse common infrastructure.

## Primary Product Loop

```text
Host receives durable intent
  -> define Mission
  -> choose the next Wave objective and gate
  -> select executor
  -> run / observe / collect artifacts
  -> accept, revise, or block the Wave
  -> re-plan the next Wave
  -> close the Mission with an outcome summary
```

Executor-specific truth stays local:

- Agent Team: assignment message -> correlated collaboration -> handoff /
  optional review -> Wave gate;
- Dynamic Workflow: program -> WorkflowRun/WorkflowStep -> artifacts/result ->
  Wave gate;
- Host: direct work and optional provider-native subagents -> observable
  artifacts/outcome -> Wave gate.

## Near-Term Scope

The current product slice prioritizes:

1. the minimal Mission/Wave contracts and non-destructive compatibility read;
2. Agent Team as a real cross-provider collaborative executor;
3. Dynamic Workflow as an independent one-shot executor;
4. shared provider/session, capability, permission, artifact, event, plugin,
   and Dashboard infrastructure;
5. honest live observation, including a transient-only thinking channel.

Standing Agents + Docs are a later layer for long-lived business operation.
They should reuse these tools, but must not distort the current Mission/Wave or
Agent Team implementation into a premature organization model.

## Required Capabilities

### Mission And Wave

- A Mission has durable intent, desired outcome, status, Waves, and closeout.
- Waves are ordered but remain small: objective, optional exit criteria,
  executor kind, attempts, accepted attempt, artifacts, outcome, and gate.
- A Wave may retry with a new executor run; the accepted attempt is explicit.
- Replanning occurs between Waves and records plan-vs-actual deviation.
- No Mission/Wave API or UI requires a Task Graph.

### Agent Team

- `AgentTeamRun` is one attempt for one Wave, not the Wave itself and not a
  standing organization.
- Each Wave defines only the members it needs: role, provider/model tier,
  permissions, owned surfaces, budget, and depth.
- A `TeamMessage(kind=assignment)` is the lane's work identity; progress,
  blocker, handoff, review, and delegation target the assignment correlation.
- Message delivery/ACK state is explicit and separate from message semantics.
- Members may use provider-native subagents as their own capability. Harness
  records only honestly observable attribution and does not pretend to control
  children owned by the provider.
- External changes such as deploy, remote deletion, protected merge, or paid
  decisions remain user authorization gates.

The v0 correlation send path is incomplete: automatic handoff preserves the
assignment correlation, while manual sends currently create a new one. The
target contract is not accepted until existing correlation/causation inputs are
supported and exercised.

### Dynamic Workflow

- Dynamic Workflow remains standalone and independently useful.
- Workflow programs own their steps, parallelism, retry, gates, patches,
  artifacts, and structured result.
- A Mission/Wave may point to a WorkflowRun without rebuilding it as a Task
  Graph or Agent Team.

### Shared Infrastructure

- Provider adapters normalize sessions and observable actions without erasing
  provider-specific capability.
- Capability snapshots, permission ceilings, budgets, artifact references,
  event transport, hooks, and Dashboard projections are reusable across
  executors.
- Plugins are thin host-native distribution/call surfaces; runtime truth stays
  in the resident service and store.
- Project-specific business logic remains in project adapters and tools.

### Thinking Boundary

Thinking is optional transient live state only: sanitize, truncate, rate-limit,
overwrite/expire, and never persist, replay, forward to peers, or treat as
evidence. Explicit plans, actions, artifacts, blockers, handoffs, and outcomes
are durable.

Current v0 Kimi execution still writes a bounded durable `thinking` action.
That is a known migration defect, not an accepted capability.

## Product Scenarios

### Resident Host Uses Agent Team

A Host turns one Mission Wave into a role-specific cross-provider team, keeps
only run/member/assignment pointers in its own context, intervenes on blockers
or approvals, and accepts a handoff-backed Wave outcome.

### Resident Host Uses Dynamic Workflow

A Host authors or selects a structured one-shot workflow, observes the run,
reviews its artifacts or patch, and attaches its typed result to the Wave gate.

### Self-Hosting Migration

This repository uses its stricter compatibility governance chain while it adds
Mission/Wave schemas, joins, runtime routing, and Dashboard surfaces. The
stricter chain proves the migration without becoming a requirement for every
external product Wave.

### Project Adapter Operation

An external project supplies its CLI/API, artifacts, dashboards, permission
rules, and domain evaluation through an adapter. The harness owns orchestration
and shared execution infrastructure, not project business logic.

### Standing Agents + Docs (Future)

Long-lived business agents use Mission/Wave, Agent Team, Dynamic Workflow, and
shared Docs/knowledge infrastructure. This is an architectural consumer of the
current tools, not current MVP acceptance.

## Non-Goals

- No mandatory Task Graph inside Mission, Wave, or Agent Team.
- No rename-only mapping from GoalPhase to Wave.
- No universal Agent/Run object that erases executor semantics.
- No claim that provider-native subagents are harness-controlled without a real
  control/observation path.
- No durable private reasoning or raw transcript as product evidence.
- No project-specific business logic in the generic core.
- No Standing Agent organization UI in the current Mission/Wave slice.
- No plugin treated as architecture authority; schemas, code, ADRs, and current
  product docs own the contract.

## Acceptance Summary

The product direction is accepted when:

- a Host can define a Mission and ordered Waves without a Task Graph;
- each Wave selects Agent Team, Dynamic Workflow, or Host execution;
- Agent Team ownership is assignment/message-based and retries are distinct run
  attempts;
- artifacts and a lightweight gate explain the accepted Wave outcome;
- shared infrastructure works across provider instances without collapsing
  their semantics;
- compatibility Goal/GoalPhase data remains readable during migration;
- thinking is live-only for new writes;
- the Dashboard and host plugins expose the same truthful read model.

Detailed implementation gates are in [mvp.md](mvp.md). The architecture map is
[architecture-map.md](architecture-map.md).
