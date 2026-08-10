# AgentMember Focus

```text
status: partial implementation
owner_role: product-design
canonical_for: one durable AgentMember identity with membership and execution history
route_or_surface: Organization -> AgentMember
```

## Product question

The operator opens a durable AgentMember to answer: what is this Member
responsible for, which Teams does it belong to, which MemberRuns are active,
what Work is assigned, and is intervention required?

AgentMember is the single organization-agent identity. Company organization
stores a membership projection referencing
`ActorRef(kind=agent_member, id=...)`; it does not own provider processes,
sessions, runtime status or a second agent identity. MemberRun is one execution
attempt and must carry the exact AgentMember id.

## Read model

The focus page joins by canonical ids:

```text
AgentMember
  -> Company AgentMembership projection
  -> Team memberships
  -> MemberRuns
  -> WorkspaceBinding / deliveries / provider session availability
  -> assigned Work and reports
```

Missing or duplicate identity is an integrity error. The UI never guesses from
name, role, provider or matching display ids. Organization fields and runtime
fields retain separate freshness and source labels.

## Actions

Organization actions change responsibility or permission-policy refs. Runtime
actions close, reopen or retire a specific MemberRun generation. Work actions
operate on exact Work revisions. Messaging creates one immutable TeamMessage
and separate MessageDelivery rows. Every write uses the canonical trust service
and returns its operation/event identity.

## Empty and failure states

No active MemberRun is a valid durable-member state. A missing membership
projection means the Member is not currently represented in Company
organization. Stale delivery claims, unavailable provider sessions and unsafe
workspace bindings appear as bounded intervention cards with exact recovery
actions; none silently rewrites identity.
