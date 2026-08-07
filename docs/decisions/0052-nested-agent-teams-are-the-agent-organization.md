# ADR 0052: Nested Agent Teams Are The Agent Organization

```text
status: superseded — replaced by docs/mental/agent-firm-mental-model.md (flat topology, no nesting)
date: 2026-08-04
superseded_by: docs/mental/agent-firm-mental-model.md
```

> **Superseded.** This ADR proposed a recursive/nested Agent Team organization.
> The current model is FLAT — Agent Teams do not nest. See the
> [Agent Firm Mental Model](../mental/agent-firm-mental-model.md) for the
> authoritative architecture.

## Historical Context

Company OS introduced a separate StandingAgent organization identity,
Organization membership/reporting records, Company Assignment, AgentMember,
MemberRun, Agent Team Work, and TeamMessage. Each distinction addressed a real
risk, but together they created an administrative path too complex for the
product's core use.

## Historical Decision

The original decision adopted Nested Agent Team Organization: AgentMember as the
durable agent identity, Organization as recursive AgentTeam topology, and one
Work kernel serving Team and Organization.

This is retained as a historical record. The flat model supersedes the nesting
aspect. Keep this ADR for decision provenance; do not treat it as current
architecture.
