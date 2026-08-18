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
AgentTeam -> TeamMembership -> AgentMember
AgentTeam -> AgentTeamRun -> MemberRun
  -> Harness coordination / native session refs / artifacts
  -> explicit Work submission, review, and Host acceptance
```

The harness is the coordination and evidence system. Project-specific tools are
connected through adapters.

## Active Vocabulary

Durable AgentTeam, Team-run Work, and identity-first Message delivery are the
active coordination vocabulary and native contract for new work. Mission,
Mission Log, and Wave are retired by DOC-108: pre-cutover rows remain
read/export-only historical context, and no writer exists on any surface.
The older superseded coordination stack is governed by
[ADR 0028](../../decisions/0028-retire-goal-phase-task-graph.md) and is not exposed
through product projections or authoring paths. Optional review and evaluation
records may strengthen a high-risk gate, but they do not replace Work
acceptance or become mandatory hierarchy levels.

## Core Object Relationships

```mermaid
flowchart TD
  Vision[Product Vision]
  Team[AgentTeam]
  TeamRun[AgentTeamRun]
  WorkflowRun[WorkflowRun]
  HostExec[Host execution]
  Message[Message]
  Delivery[CanonicalMessageDelivery]
  Member[AgentMember or MemberRun]
  Provider[NativeSessionRef / provider-owned execution]
  Event[Durable event stream]
  Evidence[Artifacts / optional Evidence]
  Judgment[Host judgment / acceptance]
  Work[Work]
  Proposal[Proposal]
  Review[Review / Critic]
  Decision[Decision]
  Eval[Optional evaluation]
  Case[reusable learning note]

  Vision --> Team
  Team --> TeamRun
  TeamRun --> Work
  TeamRun --> Member
  WorkflowRun --> Event
  HostExec --> Event
  Message --> Delivery
  Delivery --> Member
  Member --> Provider
  Provider --> Event
  Event --> Evidence
  Message --> Evidence
  Work --> Judgment
  Evidence --> Judgment
  Judgment -. optional governance .-> Eval
  Evidence -. repository governance .-> Proposal
  Proposal --> Review
  Review --> Decision
  Decision -. repository governance .-> Eval
  Eval --> Case
```

## Legacy Mission And Wave (retired, DOC-108)

`Mission`, `MissionLogEntry`, and `LegacyWave` survive only as historical
read/export compatibility rows. Their schemas stay validated so old rows can
be read and exported without data loss; no current surface writes them, and
`AgentTeam.legacy_mission_id` is optional read-only provenance, never Team
identity authority.

## Execution Capabilities

The Host may use an Agent Team, Dynamic Workflow, direct Host work, or a
combination. Work context and linked Messages explain the choice without
owning the runtime.

### `agent_team`

Agent Team is for living collaborators with persistent session state, explicit
Work ownership, review, and responsibility that spans runs of the same Team.

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
  -> linked correlated Message/reply where needed
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

Messages remain runtime facts. Agent Team ownership lives in Work; Dynamic
Workflow owns its steps; Host execution records its observable outcome.
Residual Assignment-message fields are removal debt and cannot define another
ownership path.

## Agent Team Objects

`AgentTeamRun` is one execution attempt of a durable flat AgentTeam. It is not
standalone and is not a standing organization.

| Object | Meaning | Rule |
| --- | --- | --- |
| `AgentTeam` | Durable flat execution agency with Host membership and immutable Node placement. | Created without any Mission; no parent/child Team topology. |
| `AgentTeamRun` | One Team execution with frozen Team, Node, and Project Binding. | Internal diagnostics and history only; every terminal run remains read-only and it never scopes durable identity or responsibility. |
| `AgentMember` | The sole durable addressable agent identity and organization status. | It is not a provider process, Team membership, Work owner, or native transcript. |
| `AgentIdentity` | Deprecated same-ID read-only compatibility projection of `AgentMember`. | Retained for legacy readers only; it is never a second identity root and nothing may be bound to it independently. |
| `AgentSession` | One machine-local provider session owned by an exact NodeDaemon generation, hanging off its `AgentMember`. | It has no Team identity and cannot outlive or bypass its NodeDaemon authority. |
| `TeamMembership` | The participation record joining an AgentMember to one flat Team on the Team's immutable Node. | It carries no identity and does not own the provider session or Work result. |
| `WorkExecutionBinding` | Exact Work revision → membership → AgentMember → AgentSession generation binding. | A successor Session cannot inherit active Work implicitly. |
| `MemberRun` | Run-scoped coordination/history projection for role and Work attribution. | Internal diagnostics and history only; it is not provider runtime authority and cannot dispatch, interrupt, resume, or stop a provider. |
| `Work` | Durable-Team-scoped responsibility (`accountable_team_id`), owner, readiness, state, criteria and result. | `team_run_id` only correlates the run that surfaced it; assignment, claim, block, submission and acceptance are Work operations governed by ADR 0050. |
| `WorkOperation` | Crash-atomic Store replay row containing one WorkEvent, its complete resulting Work, delivery creates/updates, and target-caused WorkDelegation revisions. | It prevents Work, delivery, and cross-Team roll-up projections from becoming independently visible; Hosts still act on Work, not WorkOperation. |
| `WorkDelivery` | Reliable delivery of one Work version to a Member runtime. | It reuses delivery machinery but is not authored conversation or Work ownership. |
| `Message` | Immutable identity-authored, source-NodeDaemon-attested conversation envelope addressed through canonical subscriptions. | It cannot carry Work ownership or runtime-control authority. |
| `MessageSubscription` / `SubscriptionCursor` | Recipient policy and exact delivery/ACK progress. | The browser and Control Plane cannot fabricate recipient state. |
| `ExecutionNode` / `NodeDaemonLease` | Stable machine identity and its one active daemon generation. | One NodeDaemon owns all local Teams and registered Project Bindings. |
| `TeamSupervisorLease` | Latest-wins cross-process authority for one active TeamRun generation. | Parent-fenced by NodeDaemon generation; owns this run's transports, claims, reconnect, and real controls. |
| `CanonicalMessageDelivery` | Delivery state for one immutable Message recipient at an exact AgentSession generation. | Makes provider receipt, acknowledgement, retries, and uncertainty explicit without a second Message or inbox authority. |
| `RuntimeCommand` | Durable prepare/settle journal for start, resume, turn/input, interrupt, and stop effects. | Ambiguous effects become `RecoveryRequired`; TeamRun, MemberRun, Message, and WorkDelivery cannot bypass it. |
| `ProviderInvocation` | Clean-cut provider-facing projection derived by the target NodeDaemon from a claimed delivery. | Public callers and browsers cannot author it or select provider compatibility. |
| `MemberAction` | Transitional Harness action row. Target use is limited to Harness-owned coordination/control facts. | Provider tool, command, file, chat, turn, and reasoning streams stay solely in the native provider session. |
| `DelegationRun` | Attribution record for observed or orchestrated delegation. | Parent permissions, paths, and budgets bound the child. |
| `TeamRunEvent` | Transitional ordered event projection for Harness-owned run lifecycle. | It must not become a mirror of provider-native activity. |

Runtime control authority is resolved by the server as exact self or the exact
machine Operator/NodeDaemon. Team Host authority is Team-scoped and cannot
start, stop, resume, or cancel a global AgentSession. Callers cannot submit
capabilities, provider profiles, permission envelopes, full AgentSession
objects, or a different machine placement. A standalone AgentMember may own
a machine Session without TeamMembership; join/leave never creates or closes
that Session. A Team and AgentMember have at most one active
TeamMembership generation; duplicate historical authority makes RoleViews fail
closed. Unknown provider effects remain Operator-visible RecoveryRequired work
until an evidence-backed, generation-fenced resolution records certainty
without replaying the native effect.

Relationship rules:

- ownership is explained by the latest Work projection and the ordered
  WorkOperations that preserve its WorkEvent audit;
- every message carries typed sender and recipient provenance; UI or MCP callers
  cannot impersonate a Member unless they are explicitly bound to that
  MemberRun;
- the current Supervisor atomically claims `CanonicalMessageDelivery` only
  after its provider transport is healthy and records the native receipt.
  Recipient acknowledgement/cursor progress is a separate idempotent delivery
  fact; WorkDelivery has no ACK state and a Work
  claim/start records responsibility acknowledgement;
- `Work`, `Message`, explicit outcomes, and Harness control facts may reference
  artifacts or `Evidence`; the Host acceptance needs an explicit outcome but
  does not require Proposal/Review/Decision objects;
- residual task-named runtime fields are removal debt, not the product model or
  a supported ownership path.

`TeamMessage`, `TeamMessageProjection`, embedded delivery rows,
`team_messages.jsonl`, and manual/legacy ACK APIs are pre-cutover compatibility
records. They may be read or exported only through explicitly Legacy surfaces;
they are never current authoring, delivery, inbox, interaction, or acceptance
authority. Provider questions and answers use correlated current `Message`
kinds and the same per-recipient `CanonicalMessageDelivery` path as ordinary
conversation.

## Generic Object Model

The learning and governance layer remains domain-neutral.

| Object | Rule |
| --- | --- |
| `Review` | Structured evaluator or critic output. Evidence for a Decision, never the Decision itself. |
| `Gap` | Defect/risk ledger row. `category=bug` is a bug; there is no separate Bug object. |
| `Evaluation` | Optional structured assessment layered on a high-risk outcome. |
| `LearningNote` | Reusable teaching artifact distilled from a completed Team-run slice. |
| `Vision` | Long-lived target that Team work advances toward. |

## Agent Runtime And Native Session

`MemberRun` and `NativeSessionRef` connect durable members, independent
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

- Work submissions, reviews, and Host acceptance record the judgment trail
  with actor/time, notes, and artifact refs;
- AgentTeamRun and WorkflowRun remain distinct execution record types; a
  historical Wave does not need or own an `accepted_run_id`. That field is
  legacy direct-executor compatibility only;
- run acceptance is based on accepted Work evidence and explicit Host
  acceptance; historical Mission/Wave rows are read-only context;
- this repository may layer review, evidence, or evaluation on high-risk work,
  but those objects are not mandatory for every self-hosting change.

The legacy governance chain must not leak into every Agent Team run as
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
