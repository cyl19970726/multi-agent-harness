# Schemas

Schemas define object contracts shared by Rust types, API responses, CLI
outputs, adapters, and the Agent Dashboard.

## First Objects

Only five core objects are required for the first version:

| Object | Purpose |
| --- | --- |
| `AgentMember` | Who can do work |
| `Task` | What needs to be done |
| `Message` | How agents communicate |
| `Evidence` | What supports a claim or result |
| `Decision` | What the Leader decided |

`Skill`, `ToolAdapter`, `ProviderSession`, and `Dashboard` can start as
configuration or views.

## Contract Maturity

| Concept | Current maturity | Gateable now |
| --- | --- | --- |
| `AgentMember` | Rust + JSON schema | yes |
| `Task` | Rust + JSON schema | yes |
| `Message` | Rust + JSON schema | yes |
| `Evidence` | Rust + JSON schema | yes |
| `Decision` | Rust + JSON schema | yes |
| `ToolDescriptor` | JSON schema + example descriptor | partially |
| `DocDescriptor` | JSON schema + docs registry + governance check | yes |
| `Skill` | markdown skill + metadata check | partially |
| `PermissionPolicy` | planned concept | no |
| `Report` / `Claim` / `Blocker` | future concepts, not first-version contracts | no |
| `ProviderSession` | future concept | no |
| Agent Dashboard read model | planned | no |

Do not present planned or future concepts as stable contracts. A concept
becomes gateable only when its source of truth and CI check are clear.

## Current JSON Schemas

| Schema | File |
| --- | --- |
| Agent member | [agent-member.schema.json](../schemas/agent-member.schema.json) |
| Task | [task.schema.json](../schemas/task.schema.json) |
| Message | [message.schema.json](../schemas/message.schema.json) |
| Evidence | [evidence.schema.json](../schemas/evidence.schema.json) |
| Decision | [decision.schema.json](../schemas/decision.schema.json) |
| Tool descriptor | [agent-harness-tool-descriptor.schema.json](../schemas/agent-harness-tool-descriptor.schema.json) |
| Doc descriptor | [doc-descriptor.schema.json](../schemas/doc-descriptor.schema.json) |

## Current Registries

| Registry | File | Check |
| --- | --- | --- |
| Docs governance | [registry.json](registry.json) | `pnpm check:doc-governance` |

## Rust Coverage Rule

If a field affects storage, API, adapter behavior, or dashboard rendering, it
must be represented in both:

```text
crates/harness-core/src/*.rs
schemas/*.schema.json
```

CI should eventually check this coverage.
