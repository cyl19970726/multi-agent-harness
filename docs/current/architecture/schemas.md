# Schemas

Schemas define object contracts shared by Rust types, API responses, CLI
outputs, adapters, and the Agent Dashboard.

## Native Mission Objects

| Object | Purpose |
| --- | --- |
| `Mission` | Durable intent/context, one owning AgentTeam, and closeout |
| `MissionLogEntry` | One append-only Host judgment, replan, recovery, or closeout-evidence record inside a Mission; not a lifecycle object |
| `AgentTeam` | One Mission's flat Team with required Host Agent and immutable Node placement |
| `AgentTeamRun` | One Team execution with required Team, Node, and Project Binding identity |
| `MemberRun` | One role/provider execution instance inside a TeamRun |
| `Work` / `WorkOperation` / `WorkEvent` / `WorkDelivery` | TeamRun-scoped responsibility projection, crash-atomic replay row, append-only semantic transition, and versioned runtime delivery |
| `WorkDelegation` / `WorkDelegationEvent` | Cross-Team responsibility handoff with CAS, idempotency, cycle prevention, and source rollup |
| `Message` | Immutable identity-first conversation envelope with typed author, correlation/causation, optional Work relation, and closed semantic kind. Provider requests and responses are Message kinds. |
| `MessageSubscription` / `SubscriptionCursor` | Authorized recipient policy and recipient progress without copying or mutating Message content. |
| `CanonicalMessageDelivery` | One recipient's queue, claim, exact AgentSession generation, provider receipt, and acknowledgement/cursor state. |
| `TeamMessage` / `TeamMessageProjection` | Legacy pre-cutover conversation projection with embedded delivery/manual ACK state; read/export only through explicitly Legacy surfaces. |
| `ExecutionNode` / `NodeProjectRegistration` / `NodeDaemonLease` | Machine identity, available Project Bindings, and the one daemon generation that owns all local TeamRuns |
| `TeamSupervisorLease` | Latest-wins TeamRun control owner parent-fenced by NodeDaemon generation |
| `MemberAction` | Transitional action schema; target scope is Harness-owned coordination/control facts, never mirrored provider activity |
| `DelegationRun` | Honest attribution for observed or harness-controlled delegation |
| `TeamRunEvent` | Ordered sanitized event projection for one TeamRun |

Dynamic Workflow and Host execution retain their distinct execution-specific
objects. Existing Goal/Task schemas and retired TeamMessage projections are
historical compatibility contracts; Evidence/Proposal/Decision remain optional
governance contracts. They are not the
active Agent Team coordination model (the retired Mission/Mission Log model is not either), and new Agent Team work
must not depend on Goal, Task Graph, Plan Gate, or a TeamMessage compatibility
path.

`Skill`, `ToolAdapter`, and `Dashboard` can start as configuration or views.

## Contract Maturity

| Concept | Current maturity | Gateable now |
| --- | --- | --- |
| `Mission` | Rust + JSON schema + JSONL store + CLI/API/MCP/read model | yes |
| `MissionLogEntry` | Rust + append-only `mission_log.jsonl` store + CLI/API/MCP/read model | yes |
| `Wave` | Rust + JSON schema + historical JSONL reads/export only; create/update/advance/gate retired by ADR 0051 | legacy only |
| `AgentTeamRun` family | Rust + JSON schemas + store + CLI/API/MCP/read model | yes |
| `Work` / `WorkEvent` / `WorkDelivery` | Rust + JSON schemas + WorkOperation JSONL store + CLI/API/MCP/read model | yes |
| `TeamSupervisorLease` | Rust + JSON schema + JSONL latest-wins store + cross-process routing | yes |
| `Goal` | historical compatibility schema; retired for new coordination | no for new work |
| `AgentTeam` | Rust + JSON schema | yes |
| `AgentMember` | Rust + JSON schema | yes |
| `Task` | historical compatibility schema; retired for new coordination | no for new work |
| `Message` | Rust + JSON schema + canonical Store/API projection; identity-first current conversation authority | yes |
| `MessageSubscription` / `SubscriptionCursor` | Rust + JSON schemas + canonical Store/API projection | yes |
| `CanonicalMessageDelivery` | Rust + JSON schema + canonical Store/API projection | yes |
| `TeamMessage` / `TeamMessageProjection` | historical JSONL/schema reads and export only; current writes and ACK routes retired | legacy only |
| `MemberRun` | Rust + JSON schema | yes |
| `MemberRunEvent` | Rust + JSON schema | yes |
| `ProviderChildThread` | Rust + JSON schema | yes |
| `Proposal` | Rust + JSON schema | yes |
| `Evidence` | Rust + JSON schema | yes |
| `Decision` | Rust + JSON schema | yes |
| `ToolDescriptor` | JSON schema + example descriptor | partially |
| `DocDescriptor` | JSON schema + docs registry + governance check | yes |
| `Skill` | markdown skill + metadata check | partially |
| `PermissionPolicy` | planned concept | no |
| `Report` / `Claim` / `Blocker` | future concepts, not first-version contracts | no |
| Agent Dashboard read model | Rust snapshot + TypeScript projection types | partially |

Do not present planned or future concepts as stable contracts. A concept
becomes gateable only when its source of truth and CI check are clear. Current
schema contracts are checked with valid and invalid fixtures.

## Current JSON Schemas

| Schema | File |
| --- | --- |
| Mission | [mission.schema.json](../../../schemas/mission.schema.json) |
| Historical Wave (ADR 0051 pre-cutover rows only) | [wave.schema.json](../../../schemas/wave.schema.json) |
| Agent Team run | [agent-team-run.schema.json](../../../schemas/agent-team-run.schema.json) |
| Member run | [member-run.schema.json](../../../schemas/member-run.schema.json) |
| Work | [work.schema.json](../../../schemas/work.schema.json) |
| Work event | [work-event.schema.json](../../../schemas/work-event.schema.json) |
| Work delivery | [work-delivery.schema.json](../../../schemas/work-delivery.schema.json) |
| Provider-native session locator | [native-session-ref.schema.json](../../../schemas/native-session-ref.schema.json) |
| Message | [message.schema.json](../../../schemas/message.schema.json) |
| Message subscription | [message-subscription.schema.json](../../../schemas/message-subscription.schema.json) |
| Subscription cursor | [subscription-cursor.schema.json](../../../schemas/subscription-cursor.schema.json) |
| Canonical message delivery | [canonical-message-delivery.schema.json](../../../schemas/canonical-message-delivery.schema.json) |
| Legacy Team message projection | [team-message.schema.json](../../../schemas/team-message.schema.json) |
| Member execution trust error | [trust-error.schema.json](../../../schemas/trust-error.schema.json) |
| Team Supervisor lease | [team-supervisor-lease.schema.json](../../../schemas/team-supervisor-lease.schema.json) |
| Member action | [member-action.schema.json](../../../schemas/member-action.schema.json) |
| Delegation run | [delegation-run.schema.json](../../../schemas/delegation-run.schema.json) |
| Team run event | [team-run-event.schema.json](../../../schemas/team-run-event.schema.json) |
| Agent team | [agent-team.schema.json](../../../schemas/agent-team.schema.json) |
| Agent member | [agent-member.schema.json](../../../schemas/agent-member.schema.json) |
| AgentMember canonical mutation event | [canonical-mutation-event.schema.json](../../../schemas/canonical-mutation-event.schema.json) |
| MemberRun mutation event | [member-run-event.schema.json](../../../schemas/member-run-event.schema.json) |
| Provider child thread | [provider-child-thread.schema.json](../../../schemas/provider-child-thread.schema.json) |
| Proposal | [proposal.schema.json](../../../schemas/proposal.schema.json) |
| Evidence | [evidence.schema.json](../../../schemas/evidence.schema.json) |
| Decision | [decision.schema.json](../../../schemas/decision.schema.json) |
| Tool descriptor | [agent-harness-tool-descriptor.schema.json](../../../schemas/agent-harness-tool-descriptor.schema.json) |
| Doc descriptor | [doc-descriptor.schema.json](../../../schemas/doc-descriptor.schema.json) |
| Review | [review.schema.json](../../../schemas/review.schema.json) |
| Gap | [gap.schema.json](../../../schemas/gap.schema.json) |
| Vision | [vision.schema.json](../../../schemas/vision.schema.json) |

Remote Node Fabric is a versioned closed bundle under
[`schemas/remote-fabric`](../../../schemas/remote-fabric). Its executable
inventory is `schema-bundle.v1.json`; `pnpm check:remote-fabric` requires every
listed schema to have valid and hostile fixtures and to match the Rust closed
operation registry. The bundle uses explicit protocol, schema, and canonical
JSON versions because it crosses machine and release boundaries.

`WorkOperation` is the Store's crash-atomic replay envelope around one
WorkEvent, its complete resulting Work, delivery creates/updates, and any
`WorkDelegation` revisions caused by that exact target-Work transition. The
embedded delegation revisions ensure HTTP, MCP, and CLI mutations cannot expose
a newer target Work with a stale cross-Team roll-up after a crash. It is not a
separately authored public lifecycle object and therefore has no standalone
public JSON Schema in V1; the public schemas above define the projections and
semantic event/delivery records exposed by CLI/API/Dashboard.

## Schema Evolution

Provider-native execution history is referenced only by
`native-session-ref.schema.json`. Harness deliberately has no schema for a
mirrored transcript, stdout stream, tool stream, or provider turn ledger.

Schemas evolve additively where a current contract permits it; retired
legacy schemas (Mission, Wave) stay pinned to their historical read contract
so old rows remain readable and exportable.

- New fields on existing objects are added as property-but-NOT-required, using
  nullable type unions (`["string","null"]`) for scalars, arrays for lists, and
  booleans for flags. Schemas stay `additionalProperties:false`, so old rows
  that omit a new optional key still validate. This is the existing
  `Evidence.task_id` precedent.
- Rust models these as `Option<T>` / `Vec<T>` / `bool` with `#[serde(default)]`,
  so old JSONL deserializes unchanged.
- There is **no `schema_version` field** and there are no `*.v2` schema files. A
  future *required* field is the only trigger for a versioned schema plus a
  migration.
- New objects get their own `<obj>.schema.json` (still
  `additionalProperties:false`, with full `required` for their own mandatory
  fields) plus valid and invalid fixtures.
- Open enums (`verdict`, `decision`, `review_kind`, `evidence_kind`,
  `decision_kind`) are free `string` in JSON Schema and validated against a
  canonical set in Rust (`#[serde(other)] Other(String)`). Only truly closed,
  harness-owned sets (`Gap.severity`, `Gap.status`) use a hard JSON `enum`.
  Harness core carries zero domain vocabulary; domain values live in adapters,
  skills, or free `*_detail` / `source_type` fields.

## Current Registries

| Registry | File | Check |
| --- | --- | --- |
| Docs governance | [registry.json](../../registry.json) | `firm governance check` |

## Fixture Validation

Schema fixtures live under `../schemas/fixtures/<schema-name>/valid` and
`../schemas/fixtures/<schema-name>/invalid`. `pnpm check:schema-fixtures`
requires every current schema to have at least one passing and one failing
fixture.

## Rust Coverage Rule

If a field affects storage, API, adapter behavior, or dashboard rendering, it
must be represented in both:

```text
crates/firm-core/src/*.rs
schemas/*.schema.json
```

CI should eventually check this coverage.
