# Schemas

Schemas define the stable object contracts shared by Rust types, API responses,
CLI outputs, adapters, and the Agent Dashboard.

## First Objects

The first version should cover:

- `AgentMember`
- `Task`
- `Message`
- `Evidence`
- `Decision`
- `ToolDescriptor`

## Current Schema Files

- [ToolDescriptor](../schemas/agent-harness-tool-descriptor.schema.json)

## Rule

If a field affects storage, API, adapter behavior, or dashboard rendering, it
must be represented in both Rust types and schema examples.
