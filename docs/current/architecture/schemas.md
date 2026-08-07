# Schemas

Schemas define object contracts shared by Rust types, API responses, CLI
outputs, adapters, and the Agent Dashboard.

## Native Mission/Wave Objects

| Object | Purpose |
| --- | --- |
| `Mission` | Durable intent/context, linked Agent Teams, ordered Waves, and closeout |
| `Wave` | One lightweight versioned Host plan/judgment and advance outcome |
| `AgentTeamRun` | One standalone or Mission-scoped use of an independent Agent Team |
| `MemberRun` | One role/provider execution instance inside a TeamRun |
| `Work` / `WorkOperation` / `WorkEvent` / `WorkDelivery` | TeamRun-scoped responsibility projection, crash-atomic replay row, append-only semantic transition, and versioned runtime delivery |
| `TeamMessage` | Typed sender/recipients, optional `work_id`, correlation/causation, optional origin Wave, response intent, claim/provider-receipt/ACK delivery state; conversation only |
| `TeamSupervisorLease` | Latest-wins TeamRun control owner, generation fence, heartbeat, and loopback locator |
| `AgentMessageRoute` | Idempotent bridge from stable Agent Inbox mail to a MemberRun/TeamMessage |
| `MemberAction` | Transitional action schema; target scope is Harness-owned coordination/control facts, never mirrored provider activity |
| `DelegationRun` | Honest attribution for observed or harness-controlled delegation |
| `TeamRunEvent` | Ordered sanitized event projection for one TeamRun |

Dynamic Workflow and Host execution retain their distinct execution-specific
objects. Existing Goal/Task/Message/Evidence/Proposal/Decision schemas are
historical compatibility or optional governance contracts. They are not the
active Mission/Wave or Agent Team coordination model, and new Agent Team work
must not depend on Goal, Task Graph, Plan Gate, or generic `Message`.

`Skill`, `ToolAdapter`, and `Dashboard` can start as configuration or views.

## Contract Maturity

| Concept | Current maturity | Gateable now |
| --- | --- | --- |
| `Mission` | Rust + JSON schema + JSONL store + CLI/API/MCP/read model | yes |
| `Wave` | Rust + JSON schema + JSONL revisions + Host update/advance | yes |
| `AgentTeamRun` family | Rust + JSON schemas + store + CLI/API/MCP/read model | yes |
| `Work` / `WorkEvent` / `WorkDelivery` | Rust + JSON schemas + WorkOperation JSONL store + CLI/API/MCP/read model | yes |
| `TeamSupervisorLease` | Rust + JSON schema + JSONL latest-wins store + cross-process routing | yes |
| `AgentMessageRoute` | Rust + JSON schema + JSONL idempotent routing store | yes |
| `Goal` | historical compatibility schema; retired for new coordination | no for new work |
| `AgentTeam` | Rust + JSON schema | yes |
| `AgentMember` | Rust + JSON schema | yes |
| `Task` | historical compatibility schema; retired for new coordination | no for new work |
| `Message` | Standing Agent/runtime compatibility message; Agent Team uses `TeamMessage` | no for Agent Team |
| `AgentRuntime` | Rust + JSON schema | yes |
| `AgentEvent` | Rust + JSON schema | yes |
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
| Mission | [mission.schema.json](../schemas/mission.schema.json) |
| Wave | [wave.schema.json](../schemas/wave.schema.json) |
| Agent Team run | [agent-team-run.schema.json](../schemas/agent-team-run.schema.json) |
| Member run | [member-run.schema.json](../schemas/member-run.schema.json) |
| Work | [work.schema.json](../schemas/work.schema.json) |
| Work event | [work-event.schema.json](../schemas/work-event.schema.json) |
| Work delivery | [work-delivery.schema.json](../schemas/work-delivery.schema.json) |
| Provider-native session locator | [native-session-ref.schema.json](../schemas/native-session-ref.schema.json) |
| Team message | [team-message.schema.json](../schemas/team-message.schema.json) |
| Team Supervisor lease | [team-supervisor-lease.schema.json](../schemas/team-supervisor-lease.schema.json) |
| Durable Member Close | [team-member-close-request.schema.json](../schemas/team-member-close-request.schema.json) |
| Stable Agent message route | [agent-message-route.schema.json](../schemas/agent-message-route.schema.json) |
| Member action | [member-action.schema.json](../schemas/member-action.schema.json) |
| Pending provider interaction | [pending-interaction.schema.json](../schemas/pending-interaction.schema.json) |
| Delegation run | [delegation-run.schema.json](../schemas/delegation-run.schema.json) |
| Team run event | [team-run-event.schema.json](../schemas/team-run-event.schema.json) |
| Agent team | [agent-team.schema.json](../schemas/agent-team.schema.json) |
| Agent member | [agent-member.schema.json](../schemas/agent-member.schema.json) |
| Durable Organization Agent member | [durable-agent-member.schema.json](../schemas/durable-agent-member.schema.json) |
| Message | [message.schema.json](../schemas/message.schema.json) |
| Agent runtime | [agent-runtime.schema.json](../schemas/agent-runtime.schema.json) |
| Agent event | [agent-event.schema.json](../schemas/agent-event.schema.json) |
| Provider child thread | [provider-child-thread.schema.json](../schemas/provider-child-thread.schema.json) |
| Proposal | [proposal.schema.json](../schemas/proposal.schema.json) |
| Evidence | [evidence.schema.json](../schemas/evidence.schema.json) |
| Decision | [decision.schema.json](../schemas/decision.schema.json) |
| Tool descriptor | [agent-harness-tool-descriptor.schema.json](../schemas/agent-harness-tool-descriptor.schema.json) |
| Doc descriptor | [doc-descriptor.schema.json](../schemas/doc-descriptor.schema.json) |
| Review | [review.schema.json](../schemas/review.schema.json) |
| Gap | [gap.schema.json](../schemas/gap.schema.json) |
| Vision | [vision.schema.json](../schemas/vision.schema.json) |

`WorkOperation` is the Store's crash-atomic replay envelope around one
WorkEvent, its complete resulting Work, and delivery creates/updates. It is not
a separately authored public lifecycle object and therefore has no standalone
public JSON Schema in V1; the three public schemas above define the projections
and semantic event/delivery records exposed by CLI/API/Dashboard.

## Schema Evolution

Provider-native execution history is referenced only by
`native-session-ref.schema.json`. Harness deliberately has no schema for a
mirrored transcript, stdout stream, tool stream, or provider turn ledger.

Schemas evolve additively where a current contract permits it; Company OS
contracts define their own required migration and validation rules.

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
| Docs governance | [registry.json](registry.json) | `harness governance check` |

## Fixture Validation

Schema fixtures live under `../schemas/fixtures/<schema-name>/valid` and
`../schemas/fixtures/<schema-name>/invalid`. `pnpm check:schema-fixtures`
requires every current schema to have at least one passing and one failing
fixture.

## Rust Coverage Rule

If a field affects storage, API, adapter behavior, or dashboard rendering, it
must be represented in both:

```text
crates/harness-core/src/*.rs
schemas/*.schema.json
```

CI should eventually check this coverage.
