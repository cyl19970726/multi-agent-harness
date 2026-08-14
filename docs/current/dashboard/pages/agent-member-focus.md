# AgentMember Focus

```text
status: authenticated Agent Workspace implemented for Team Host and Member execution identities
owner_role: product-design
canonical_for: one durable AgentMember identity with membership and execution history
route_or_surface: Agent Team -> Agent Workspace; Organization -> AgentMember remains durable identity focus
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

Inside an Agent Team, Host and Member execution detail use one authenticated
three-column Agent Workspace rather than separate overview pages. The left
roster selects one durable AgentMember, the center switches between Session,
Messages and Work, and the right rail contains only selected/current facts.
Clicking the Agent identity opens configuration, skills, permissions, runtime,
workspace and Session history without replacing the conversation canvas.

Provider-private Session projection is exact-owner only. A Member may inspect
that Member's own Session; a Host may inspect only the Host's own Session. When
the Host selects a Member, the server returns the `host_member_public` scope:
public authored Messages, responsibility, Work state, evidence, and exact
authorized coordination-control targets, but no Member Thinking, tool output,
provider observation, runtime command, AgentSession id/generation, native event,
or workspace binding. Authored Messages remain separate coordination records
and are visible through their authenticated sender AgentIdentity/Session,
recipients, optional Work link and per-recipient public delivery state.

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

Organization actions change responsibility or permission-policy refs.
MemberRun actions close, reopen, or retire one coordination participation; they
must never be labeled as stopping, resuming, or interrupting an AgentSession.
Provider turn/session control uses durable RuntimeCommand authority and is not
currently exposed by the Agent Workspace RoleAction adapter. Work actions
operate on exact Work revisions. Messaging creates one immutable identity-first
Message and one `CanonicalMessageDelivery` per authorized recipient identity.
Every write uses the canonical trust service and returns its operation/event
identity. The page never reads or writes the Legacy TeamMessage projection.

## Empty and failure states

No active MemberRun is a valid durable-member state. A missing membership
projection means the Member is not currently represented in Company
organization. Stale delivery claims, unavailable provider sessions and unsafe
workspace bindings appear as bounded intervention cards with exact recovery
actions; none silently rewrites identity.
