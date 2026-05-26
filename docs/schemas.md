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

## Current JSON Schemas

| Schema | File |
| --- | --- |
| Agent member | [agent-member.schema.json](../schemas/agent-member.schema.json) |
| Task | [task.schema.json](../schemas/task.schema.json) |
| Message | [message.schema.json](../schemas/message.schema.json) |
| Evidence | [evidence.schema.json](../schemas/evidence.schema.json) |
| Decision | [decision.schema.json](../schemas/decision.schema.json) |
| Tool descriptor | [agent-harness-tool-descriptor.schema.json](../schemas/agent-harness-tool-descriptor.schema.json) |

## Rust Coverage Rule

If a field affects storage, API, adapter behavior, or dashboard rendering, it
must be represented in both:

```text
crates/harness-core/src/*.rs
schemas/*.schema.json
```

CI should eventually check this coverage.
