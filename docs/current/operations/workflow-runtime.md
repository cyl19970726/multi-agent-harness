# Dynamic Workflow (retired)

Dynamic Workflow, its Starlark authoring surface, `firm workflow` commands,
runtime writers/readers, Dashboard routes, plugins, and runnable examples are
retired. They are not a current Star Harness executor model and must not be
used as a fallback for Agent Team or Host execution.

## Current coordination model

Star Harness currently coordinates execution through durable `AgentTeam`,
`Work`, identity-first `Message` delivery, provider-native `AgentSession`
bindings, and fenced `RuntimeCommand` effects. A Host or provider may use local
plans, loops, or native subagents as implementation details, but those details
do not create another workflow ledger, task authority, or acceptance state.

## Historical records

Historical `WorkflowRun`, `WorkflowStep`, output, patch, artifact, and authored
program records are evidence only. Preserve them through the lossless legacy
archive export, verification, and restore-read path. No current CLI, HTTP, MCP,
service, migration, test helper, plugin, or background process may create,
resume, mutate, or project them as live state.

Retirement does not mean that old data was deleted, that an owner or
administrator cannot alter storage, or that provider-native history moved into
Harness. Provider-native stores remain the transcript authority, and archived
Harness records remain historical coordination evidence.

The original design rationale is retained in historical ADRs
[0022](../../decisions/0022-dynamic-workflow-runtime-json-ir.md) and
[0023](../../decisions/0023-starlark-workflow-frontend.md), both marked
superseded by the Dynamic Workflow retirement cutover.
