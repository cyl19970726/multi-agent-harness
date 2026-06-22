# Schemas

Schemas define object contracts shared by Rust types, API responses, CLI
outputs, adapters, and the Agent Dashboard.

## First Objects

Only seven core objects are required for the first version:

| Object | Purpose |
| --- | --- |
| `Goal` | What durable outcome the task graph is pursuing |
| `AgentTeam` | Which Agent Members work together |
| `AgentMember` | Who can do work and which provider backs it |
| `Task` | What needs to be done, by whom, with which dependencies, workspace, branch, and PR |
| `Message` | How agents communicate |
| `AgentRuntime` | How a persistent provider process is tracked |
| `AgentEvent` | What happened inside a provider runtime |
| `ProviderChildThread` | Provider-native child threads, such as Codex native subagents spawned under one AgentMember |
| `Proposal` | What an agent proposes for a task before final decision |
| `Evidence` | What supports a claim or result |
| `Decision` | What the Leader decided |
| `ProviderSession` | How an external agent execution is recorded |

`Skill`, `ToolAdapter`, and `Dashboard` can start as configuration or views.

## Contract Maturity

| Concept | Current maturity | Gateable now |
| --- | --- | --- |
| `Goal` | Rust + JSON schema | yes |
| `AgentTeam` | Rust + JSON schema | yes |
| `AgentMember` | Rust + JSON schema | yes |
| `Task` | Rust + JSON schema | yes |
| `Message` | Rust + JSON schema | yes |
| `AgentRuntime` | Rust + JSON schema | yes |
| `AgentEvent` | Rust + JSON schema | yes |
| `ProviderChildThread` | Rust + JSON schema | yes |
| `Proposal` | Rust + JSON schema | yes |
| `Evidence` | Rust + JSON schema | yes |
| `Decision` | Rust + JSON schema | yes |
| `ProviderSession` | Rust + JSON schema | yes |
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
| Goal | [goal.schema.json](../schemas/goal.schema.json) |
| Agent team | [agent-team.schema.json](../schemas/agent-team.schema.json) |
| Agent member | [agent-member.schema.json](../schemas/agent-member.schema.json) |
| Task | [task.schema.json](../schemas/task.schema.json) |
| Message | [message.schema.json](../schemas/message.schema.json) |
| Agent runtime | [agent-runtime.schema.json](../schemas/agent-runtime.schema.json) |
| Agent event | [agent-event.schema.json](../schemas/agent-event.schema.json) |
| Provider child thread | [provider-child-thread.schema.json](../schemas/provider-child-thread.schema.json) |
| Proposal | [proposal.schema.json](../schemas/proposal.schema.json) |
| Evidence | [evidence.schema.json](../schemas/evidence.schema.json) |
| Decision | [decision.schema.json](../schemas/decision.schema.json) |
| Provider session | [provider-session.schema.json](../schemas/provider-session.schema.json) |
| Tool descriptor | [agent-harness-tool-descriptor.schema.json](../schemas/agent-harness-tool-descriptor.schema.json) |
| Doc descriptor | [doc-descriptor.schema.json](../schemas/doc-descriptor.schema.json) |
| Review | [review.schema.json](../schemas/review.schema.json) |
| Gap | [gap.schema.json](../schemas/gap.schema.json) |
| Goal design | [goal-design.schema.json](../schemas/goal-design.schema.json) |
| Goal evaluation | [goal-evaluation.schema.json](../schemas/goal-evaluation.schema.json) |
| Goal case | [goal-case.schema.json](../schemas/goal-case.schema.json) |
| Vision | [vision.schema.json](../schemas/vision.schema.json) |

## Schema Evolution

Schemas evolve **additive-optional**: see
[decisions/0017-generic-object-model.md](decisions/0017-generic-object-model.md).

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
