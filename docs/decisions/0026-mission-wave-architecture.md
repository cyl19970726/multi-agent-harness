# ADR 0026: Mission/Wave execution foundation

## Status

Partially superseded by ADR
[0034](0034-host-plan-waves-and-mission-teams.md). Mission/Wave names and the
transient-thinking policy remain; Wave-as-executor and accepted-attempt
ownership are historical compatibility semantics. ADR 0027 establishes Docs
and the mixed Organization as the primary product.

## Decision

New coordination uses one durable hierarchy:

```text
Mission -> ordered Wave -> executor
```

A Mission states durable intent. A Wave is a lightweight ordered outcome
boundary with an objective, selected executor, artifacts, and an explicit gate.
Executors are `agent_team`, `dynamic_workflow`, or `host`.

The shared substrate provides provider sessions and runtimes, capability
snapshots, permission and budget ceilings, messages, artifacts, events,
plugins/MCP, and Dashboard projections. It does not merge executor-specific
truth into one universal run model.

For an Agent Team Wave, acceptance is reconstructable from the linked
`AgentTeamRun`, member runs, assignment-correlated team messages, explicit
outcome and artifact/check references, and the accepted Wave gate. Provider
subagents remain implementation details unless an integration can record honest
attribution without claiming lifecycle control.

Thinking is transient live state only. It is never durable evidence, replay
input, or peer message payload.

## Boundaries

- Company operations use Documents, WorkItems, Approvals, Actors, finance, and
  other Company OS records; execution facts link back to them without replacing
  their authority.
- A Wave does not prescribe a planning shape or UI layout. Its executor owns
  its internal mechanics, and the Wave gate records the result.
- Retired coordination records are historical provenance only. Their archive,
  export, and deletion rules are owned exclusively by ADR 0028 and the removal
  plan.

## Validation

A meaningful run must expose Mission, Wave, selected executor, observable run
attempt, outcome/artifacts, and a gate that names the accepted attempt. Company
acceptance remains separate: sensitive actions require their applicable policy,
and durable business effects must update their linked canonical records.
