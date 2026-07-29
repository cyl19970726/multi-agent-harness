# ADR 0045: Company-owned Standing Agent execution relation

Status: active

## Decision

Company OS may link organization identity to reusable execution configuration
only through:

```text
StandingAgent.execution_agent_member_ref -> AgentMember.id
AgentMember.id <- MemberRun.agent_member_id
```

The first edge is optional, Company-owned, and one-to-one. Equal ids, names,
roles, providers, models, sessions, or timestamps never create the relation.
Duplicate latest StandingAgent refs are an integrity error, not last-write-wins.

## Lifecycle boundary

StandingAgent owns organization identity, authority, and declared availability.
AgentMember owns reusable execution configuration. MemberRun owns one TeamRun
participation. MemberRun start, idle, failure, Close, Supervisor recovery, or
native-session changes never write lifecycle state back to either durable
identity.

The Company projection is read-only. It may expose assignment-less
participation, chronological Agent Team assignments, mailbox and pending
interaction counts, Supervisor/Close facts, evidence references, and navigation
without inferring business availability or authority.

Wave and Mission ids are optional navigation context. Agent Team assignment
messages, not Waves, own member work.
