# Concept Model

This document defines the canonical object relationships for Star Harness. It
exists to prevent architecture drift: implementation may add fields, commands,
and views, but it must not change the meaning of the core objects without
updating this model first.

Source-of-truth rules and gate invariants live in
[data-model.md](data-model.md). This document owns product meaning,
relationship rules, active vocabulary, and anti-drift invariants.

## Vision

The accepted product vision is:

```text
Turn a project objective into agent-operable work:
Mission -> ordered Host-plan Wave
Mission <-> independent AgentTeam -> TeamRun -> MemberRun
  -> Harness coordination / native session refs / artifacts
  -> explicit Host advance -> Mission outcome
```

The harness is the coordination and evidence system. Project-specific tools are
connected through adapters.

## Active Vocabulary

Mission/Wave is the only active coordination vocabulary and native contract
for new work. The superseded coordination stack is governed by
[ADR 0028](../../decisions/0028-retire-goal-phase-task-graph.md) and is not exposed
through product projections or authoring paths. Optional review and evaluation
records may strengthen a high-risk gate, but they do not replace Mission
closeout or become mandatory hierarchy levels.

## Core Object Relationships

```mermaid
flowchart TD
  Vision[Product Vision]
  Mission[Mission]
  Wave[Native Wave]
  TeamRun[AgentTeamRun]
  WorkflowRun[WorkflowRun]
  HostExec[Host execution]
  Message[Message]
  TeamMessage[TeamMessage]
  Member[AgentMember or MemberRun]
  Provider[NativeSessionRef / provider-owned execution]
  Event[Durable event stream]
  Evidence[Artifacts / optional Evidence]
  Gate[Host Wave advance]
  Outcome[Mission outcome]
  Proposal[Proposal]
  Review[Review / Critic]
  Decision[Decision]
  Eval[Optional evaluation]
  Case[reusable learning note]

  Vision --> Mission
  Mission --> Wave
  Mission --> TeamRun
  Wave -. plan explains .-> TeamRun
  Wave -. plan explains .-> WorkflowRun
  Wave -. plan explains .-> HostExec
  TeamRun --> TeamMessage
  TeamRun --> Member
  WorkflowRun --> Event
  HostExec --> Event
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
  Outcome -. optional governance .-> Eval
  Evidence -. repository governance .-> Proposal
  Proposal --> Review
  Review --> Decision
  Decision -. repository governance .-> Eval
  Eval --> Case
```

## Mission And Wave

A `Mission` is the durable objective and relation boundary for reusable teams.
A `Wave` is a lightweight, versioned Markdown record of the Host's current
plan and judgment.

Rules:

- a Mission owns objective, success interpretation, priority, and closeout
  standard;
- a Mission may link zero or more independent AgentTeams;
- a Wave records changed facts, Work/member composition changes, blockers,
  carry-over, evidence, and the Host's advance outcome;
- a Wave does not require or expose a legacy dependency graph as a product concept;
- a Wave is not an executor container, task graph, session boundary, or barrier;
- replanning is an explicit Wave update/advance, not a hidden side effect;
- a Mission is not complete because activity happened; it is complete when its
  Host decisions and explicit closeout summary support the desired outcome. Stricter
  evidence or evaluation may be layered on when the domain or risk requires it.

Failure mode this prevents: replacing a durable objective with a sequence of
convenient implementation steps and then claiming completion from activity
alone.

## Execution Capabilities

The Host may use an Agent Team, Dynamic Workflow, direct Host work, or a
combination. Wave context explains the choice without owning the runtime.

### `agent_team`

Agent Team is for living collaborators with persistent session state, explicit
Work ownership, review, and responsibility that may span Waves.

A Member is the accountable end-to-end lane owner. Provider-native subagents
are bounded internal helpers whose results, permissions, evidence, and review
responsibility return to that Member. A separate Member is required for an
independent mailbox, Workspace, session, or acceptance role.

The accepted target proof is Work responsibility:

```text
Work assignment/claim -> WorkOperation(WorkEvent + resulting Work + deliveries)
  -> WorkDelivery
  -> MemberRun + Workspace + NativeSessionRef
  -> Work block / submission / review / acceptance
  -> linked TeamMessage / PendingInteraction where needed
  -> explicit outcome + artifacts/check refs
```

Messages can link the same-run Work while preserving correlation/reply lineage.
They do not mutate owner or state. Cross-run, unknown, and mismatched Work or
message lineage is rejected before persistence.

Members may communicate directly with same-run peers without routine Host
approval. The Host can observe those messages and answers its own Inbox.
Minimal Work blockers compute readiness; no conditional message or general Task
Graph is introduced.

### `dynamic_workflow`

Dynamic Workflow is a one-shot structured engine. It may share runtime
infrastructure with other executors, but it is not an Agent Team and should not
be described with Agent Team semantics.

### `host`

Host execution is direct work by the resident Host Agent. The host may use
provider-native subagents internally. Those subagents are optional observation
targets, not canonical child records unless the harness actually controls them.

## Messages And Ownership

Messages remain runtime facts, but a Wave does not contain a task graph. Agent
Team ownership lives in Work; Dynamic Workflow owns its steps; Host execution
records its observable outcome. Residual Assignment-message fields are removal
debt and cannot define another ownership path.

## Agent Team Objects

`AgentTeamRun` is one standalone or Mission-scoped use of an independent team.
It is not a standing organization and is not owned by one Wave.

| Object | Meaning | Rule |
| --- | --- | --- |
| `AgentTeamRun` | One standalone or Mission-scoped team execution. | May span Waves; every terminal run remains read-only history. |
| `MemberRun` | One member instance inside a run: role, provider, model, status, worktree, owned paths. | Exists only for that run; it is not a durable standing employee record. |
| `Work` | TeamRun-scoped responsibility, owner, readiness, state, criteria and result. | Assignment, claim, block, submission and acceptance are Work operations governed by ADR 0050. |
| `WorkOperation` | Crash-atomic Store replay row containing one WorkEvent, its complete resulting Work, and delivery creates/updates. | It prevents an event and its projection from becoming independently visible; Hosts still act on Work, not WorkOperation. |
| `WorkDelivery` | Reliable delivery of one Work version to a Member runtime. | It reuses delivery machinery but is not authored conversation or Work ownership. |
| `TeamMessage` | Run-scoped authored conversation envelope with delivery records and optional Work link. | Questions, answers, planning and coordination live here; it is not task state or a fake live-control protocol. |
| `TeamSupervisorLease` | Latest-wins cross-process authority for one active TeamRun generation. | Owns provider transports, delivery claims, reconnect, and real Steer/Interrupt/Close routing; it is not a provider transcript. |
| `AgentMessageRoute` | Stable bridge from a reusable Agent Inbox message to one active MemberRun/TeamMessage. | Makes external Agent-addressed mail explicit and idempotent without collapsing Agent identity into MemberRun identity. |
| `MemberAction` | Transitional Harness action row. Target use is limited to Harness-owned coordination/control facts. | Provider tool, command, file, chat, turn, and reasoning streams stay solely in the native provider session. |
| `DelegationRun` | Attribution record for observed or orchestrated delegation. | Parent permissions, paths, and budgets bound the child. |
| `TeamRunEvent` | Transitional ordered event projection for Harness-owned run lifecycle. | It must not become a mirror of provider-native activity. |

Relationship rules:

- a Mission may link multiple independent teams and create multiple TeamRuns;
- ownership is explained by the latest Work projection and the ordered
  WorkOperations that preserve its WorkEvent audit;
- every message carries typed sender and recipient provenance; UI or MCP callers
  cannot impersonate a Member unless they are explicitly bound to that
  MemberRun;
- the current Supervisor atomically claims delivery only after its provider
  transport is healthy and records the native receipt. TeamMessage ACK is a
  separate idempotent intake state; WorkDelivery has no ACK state and a Work
  claim/start records responsibility acknowledgement;
- `Work`, `TeamMessage`, explicit outcomes, and Harness control facts may reference
  artifacts or `Evidence`; the
  Host Wave advance needs an explicit outcome but does not require
  Proposal/Review/Decision objects;
- residual task-named runtime fields are removal debt, not the product model or
  a supported ownership path.

## Generic Object Model

The learning and governance layer remains domain-neutral.

| Object | Rule |
| --- | --- |
| `Review` | Structured evaluator or critic output. Evidence for a Decision, never the Decision itself. |
| `Gap` | Defect/risk ledger row. `category=bug` is a bug; there is no separate Bug object. |
| `Evaluation` | Optional structured assessment layered on a high-risk outcome. |
| `LearningNote` | Reusable teaching artifact distilled from a closed Mission. |
| `Vision` | Long-lived target that Missions advance toward. |

## Agent Runtime And Native Session

`AgentRuntime` and `NativeSessionRef` connect durable members, independent
execution runs, and Host tools to external providers such as Codex, Claude, or
Kimi.

Rules:

- Harness owns Work responsibility, interaction routing, explicit
  outcomes, artifact/check references, and gates;
- the provider-native store owns model execution, transcript, tool/command/file
  activity, provider turns, and resume state;
- provider output can support an execution claim through its native session
  reference without being copied into Harness;
- hooks are observation inputs, not the canonical message bus;
- runtime health is represented as lifecycle state, not inferred only from raw
  provider output.

Failure modes this prevents: a provider transcript becoming the hidden source
of truth for ownership or acceptance, and a Harness mirror becoming a divergent
second transcript.

## Thinking Policy

The target contract makes thinking transient live-only state.

- It may be shown live when a provider exposes it.
- It is bounded and sanitized.
- It is never persisted in canonical harness history.
- It is never replayable state.
- It is never execution evidence.
- It is never forwarded into another member's context.

Persist only Harness-owned coordination, artifact/check references, blockers,
Work submission/acceptance, control acknowledgements, and explicit outcomes instead.

No provider thinking may enter a Harness ledger. Provider-derived action rows
are excluded by the implemented ADR 0032 boundary; historical rows do not
define current product state and are not projected as active activity.

## Closeout Gates

The product contract and this repository's current self-hosting governance are
deliberately different:

- a Wave decision records the Host's `accepted | revise | blocked` judgment,
  actor/time, outcome summary, a short note, and useful artifact refs;
- new AgentTeamRun and WorkflowRun records remain independent; a Wave does not
  need or own an `accepted_run_id`. That field is legacy direct-executor
  compatibility only;
- a Mission outcome is based on its Wave gates and an explicit Mission-level
  closeout summary;
- this repository may layer review, evidence, or evaluation on high-risk Waves,
  but those objects are not mandatory for every self-hosting change.

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
| `evidence_kind` | Evidence | `check`, `log`, `session`, `diff`, `review_note`, `screenshot`, `artifact`, `snapshot`, `historical work design`, `outcome evaluation`, `other` |
| `category` | Gap | `ux`, `data`, `observability`, `parity`, `tooling`, `workflow`, `docs`, `bug`, `other` |
| `outcome` | outcome evaluation | `success`, `partial`, `failed`, `blocked` |

Only truly closed, harness-owned sets should use hard JSON enums.
