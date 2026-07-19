# Concept Model

This document defines the canonical object relationships for Star Harness. It
exists to prevent architecture drift: implementation may add fields, commands,
and views, but it must not change the meaning of the core objects without
updating this model first.

Source-of-truth rules and gate invariants live in
[data-model.md](data-model.md). This document owns product meaning,
relationship rules, compatibility vocabulary, and anti-drift invariants.

## Vision

The accepted product vision is:

```text
Turn a project objective into an agent-operable workflow:
Mission -> Scenario -> Infra -> Wave -> executor
  -> Message delivery / actions / artifacts
  -> lightweight Wave gate -> Mission outcome
```

The harness is the coordination and evidence system. Project-specific tools are
connected through adapters.

## Compatibility Terms

Mission/Wave is the canonical product vocabulary. Current code and stored data
still use older names in places. Docs must make that explicit.

| Canonical term | Current compatibility surface | Rule |
| --- | --- | --- |
| `Mission` | `Goal` object, `goal` CLI, Goal-facing dashboard/docs | Use `Mission` for product semantics. Mention `Goal` only when referring to current runtime/store compatibility. |
| `Wave` | `GoalPhase`, phase-local task joins, some Goal page contracts | A Wave replaces GoalPhase as the product term. `GoalPhase` remains a transitional runtime concept until migration lands. |
| `MissionEvaluation` | `GoalEvaluation` | Closeout semantics stay the same while names migrate. |

## Core Object Relationships

```mermaid
flowchart TD
  Vision[Product Vision]
  Mission[Mission / Goal compat]
  Wave[Wave / GoalPhase compat]
  TeamRun[AgentTeamRun]
  WorkflowRun[WorkflowRun]
  HostExec[Host execution]
  Task[Task compat / internal execution unit]
  Message[Message]
  TeamMessage[TeamMessage]
  Member[AgentMember or MemberRun]
  Provider[ProviderSession / execution session]
  Event[Durable event stream]
  Evidence[Artifacts / optional Evidence]
  Gate[Lightweight Wave gate]
  Outcome[Mission outcome]
  Proposal[Proposal]
  Review[Review / Critic]
  Decision[Decision]
  Eval[MissionEvaluation / GoalEvaluation compat]
  Case[GoalCase]

  Vision --> Mission
  Mission --> Wave
  Wave --> TeamRun
  Wave --> WorkflowRun
  Wave --> HostExec
  TeamRun --> TeamMessage
  TeamRun --> Member
  WorkflowRun --> Event
  HostExec --> Event
  Task --> Message
  Message --> Member
  TeamMessage --> Member
  Member --> Provider
  Provider --> Event
  Event --> Evidence
  Message --> Evidence
  TeamMessage --> Evidence
  Evidence --> Gate
  Gate --> Outcome
  Mission --> Outcome
  Outcome --> Eval
  Evidence -. repository governance .-> Proposal
  Proposal --> Review
  Review --> Decision
  Decision -. repository governance .-> Eval
  Eval --> Case
```

## Mission And Wave

A `Mission` is the durable objective. A `Wave` is the lightweight ordered unit
inside a Mission.

Rules:

- a Mission owns objective, success interpretation, priority, and closeout
  standard;
- a Wave owns objective, exit criteria, status, executor reference, outcome,
  and a lightweight gate;
- a Wave does not require or expose a Task Graph as a product concept;
- replanning happens between Waves as an explicit design/update step, not as a
  hidden side effect;
- a Mission is not complete because activity happened; it is complete only when
  decision and evaluation evidence support the outcome.

Failure mode this prevents: replacing a durable objective with a sequence of
convenient implementation steps and then claiming completion from activity
alone.

## Executors

A Wave chooses one executor kind:

- `agent_team`
- `dynamic_workflow`
- `host`

### `agent_team`

Agent Team is for living collaborators with persistent session state, explicit
assignment, handoff, review, and lane ownership inside the Wave.

The target proof is assignment-message correlation:

```text
TeamMessage(kind=assignment)
  -> correlation_id
  -> MemberAction / blocker / handoff / review_result / delegation
  -> artifacts, checks, summaries, explicit outcome
```

Current v0 automatic handoff preserves this correlation, but manually sent
progress, blocker, and review messages still receive a new correlation id.
Those messages must not be described as structurally linked until the send
surfaces accept an existing correlation/causation reference.

### `dynamic_workflow`

Dynamic Workflow is a one-shot structured executor. It may share runtime
infrastructure with other executors, but it is not an Agent Team and should not
be described with Agent Team semantics.

### `host`

The Host executor is direct work by the resident Host Agent. The host may use
provider-native subagents internally. Those subagents are optional observation
targets, not canonical child records unless the harness actually controls them.

## Tasks And Messages

`Task` and `Message` remain important current-runtime objects, but they are no
longer the canonical explanation of a Wave.

Rules:

- a `Task` is still the smallest assignable and reviewable unit in the current
  task system and in compatibility flows;
- `Message(kind=task)` remains current proof of assignment for classic task
  execution;
- task DAGs, `Task.phase_id`, and `GoalPhase` joins may continue internally
  while Mission/Wave migration is underway;
- not every Wave must expose a Task Graph to the user or planner;
- when Agent Team is the executor, TeamMessage assignment correlation explains
  ownership more directly than a task DAG does.

Failure mode this prevents: forcing every orchestration mode through the legacy
task-graph story even when the real proof is message-driven collaboration or a
one-shot workflow run.

## Agent Team Objects

`AgentTeamRun` is one wave-scoped execution owned by the `agent_team`
executor kind. It is not a standing organization.

| Object | Meaning | Rule |
| --- | --- | --- |
| `AgentTeamRun` | One team execution attempt for one Wave. | One Wave may have multiple attempts; every terminal attempt becomes read-only history. |
| `MemberRun` | One member instance inside a run: role, provider, model, status, worktree, owned paths. | Exists only for that run; it is not a durable standing employee record. |
| `TeamMessage` | Run-scoped communication envelope with delivery records. | Assignment, handoff, blocker, review, and control messages live here. |
| `MemberAction` | Normalized explicit work/action record: plan update, file change, command, test, review, delegation, waiting, blocked, completed, error. | Stores explicit action facts, not private reasoning. |
| `DelegationRun` | Attribution record for observed or orchestrated delegation. | Parent permissions, paths, and budgets bound the child. |
| `TeamRunEvent` | Ordered durable event log for one run. | Payloads are sanitized before storage. |

Relationship rules:

- a Wave using `agent_team` may instantiate one or more `AgentTeamRun` attempts
  and records which attempt its gate accepted;
- ownership inside the Wave is explained by `TeamMessage(kind=assignment)` plus
  `correlation_id`;
- `TeamMessage` and `MemberAction` may reference artifacts or `Evidence`; the
  Wave gate needs an explicit outcome and acceptance note but does not require
  Proposal/Review/Decision objects;
- current runtime code may still reference Goal/GoalPhase/task fields during
  migration, but those fields are compatibility seams, not the product model.

## Generic Object Model

The learning and governance layer remains domain-neutral.

| Object | Rule |
| --- | --- |
| `Review` | Structured evaluator or critic output. Evidence for a Decision, never the Decision itself. |
| `Gap` | Defect/risk ledger row. `category=bug` is a bug; there is no separate Bug object. |
| `GoalDesign` | Current compatibility object for the Mission design/plan. |
| `GoalEvaluation` | Current compatibility object for Mission closeout and learning. |
| `GoalCase` | Reusable teaching artifact distilled from a closed Mission/Goal. |
| `Vision` | Long-lived target that Missions/Goals advance toward. |

## Agent Runtime And Provider Session

`AgentRuntime` and `ProviderSession` connect durable members, Wave executors,
and host tools to external providers such as Codex, Claude, or Kimi.

Rules:

- the harness owns assignment, artifacts, evidence, review, and decisions;
- the provider owns model execution and transcript details;
- provider output becomes useful only after it is reduced into explicit actions,
  artifacts, evidence, or outcomes;
- hooks are observation inputs, not the canonical message bus;
- runtime health is represented as lifecycle state, not inferred only from raw
  provider output.

Failure mode this prevents: the provider transcript becoming the hidden source
of truth for ownership, status, or acceptance.

## Thinking Policy

The target contract makes thinking transient live-only state.

- It may be shown live when a provider exposes it.
- It is bounded and sanitized.
- It is never persisted in canonical harness history.
- It is never replayable state.
- It is never execution evidence.
- It is never forwarded into another member's context.

Persist explicit actions, artifacts, summaries, blockers, and outcomes instead.

Current v0 Kimi execution still writes a bounded durable `thinking` action.
That row is a migration gap, is not evidence, and must disappear from new writes
after the transient channel is implemented.

## Closeout Gates

The product contract and this repository's current self-hosting governance are
deliberately different:

- a Wave product gate records `accepted | revise | blocked`, the accepted run
  attempt, actor/time, outcome summary, a short note, and useful artifact refs;
- a Mission outcome is based on its Wave gates and an explicit Mission-level
  closeout summary;
- while this repository migrates, harness-managed development still requires
  the stricter current `Decision` + evidence + `GoalEvaluation` closeout (or an
  explicit waiver with evidence and a follow-up task).

The legacy governance chain must not leak into every Agent Team product Wave as
a mandatory object graph.

## Open-Enum Vocabularies

Useful but workflow-flavored taxonomies remain open enums: harness defines a
canonical starter set in Rust, JSON keeps the field as `string`, and adapters
may add values without a schema bump.

| Field | Object | Canonical values |
| --- | --- | --- |
| `review_kind` | Review | `acceptance`, `correctness`, `safety`, `design`, `data_flow`, `docs`, `other` |
| `verdict` | Review | `pass`, `fail`, `blocked`, `needs_changes` |
| `decision` | Decision | `accept`, `reject`, `revise`, `split`, `block`, `promote`, `waive`, `follow_up`, `stop_approved`, `continue_required` |
| `decision_kind` | Decision | `verdict`, `gate`, `stop_gate`, `waiver`, `closeout`, `promotion`, `other` |
| `evidence_kind` | Evidence | `check`, `log`, `session`, `diff`, `review_note`, `screenshot`, `artifact`, `snapshot`, `goal_design`, `goal_evaluation`, `other` |
| `category` | Gap | `ux`, `data`, `observability`, `parity`, `tooling`, `workflow`, `docs`, `bug`, `other` |
| `outcome` | GoalEvaluation | `success`, `partial`, `failed`, `blocked` |

Only truly closed, harness-owned sets should use hard JSON enums.
