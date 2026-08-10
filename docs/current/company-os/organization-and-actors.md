# Organization and Actors

```text
status: canonical Company OS contract
owner_role: product
canonical_for: flat AgentTeam organization, actor identity, responsibility, and administration boundary
```

Company OS answers four questions:

1. who exists;
2. which Agent Team each actor belongs to;
3. who may assign and accept Work for that Team; and
4. how Work flows across Teams.

The agent organization is the persistent projection of Agent Teams. The authoritative model is the [Agent Firm Mental Model](../../mental/agent-firm-mental-model.md). **Organization topology is FLAT: Agent Teams do not nest.**

## Organization model

```text
Company
├── Agent Team A (machine 1)
│   └── Host Agent + Members
├── Agent Team B (machine 2)
│   └── Host Agent + Members
└── Agent Team C (machine 3)
    └── Host Agent + Members
```

- Teams are independent, non-nested execution units.
- Each Team has a Host Agent and Members.
- A Team has one immutable `node_id`; its Members do not execute across machines.
- Standing Agents are durable identities that persist across Team Runs (e.g. a governance Agent on a schedule).

## Native actor identities

All durable company records reference an actor through `ActorRef`:

```text
ActorRef
- actor_type = human | agent | external | service
- actor_id
```

Each actor type retains a distinct lifecycle:

| Actor | Durable identity | Responsibility boundary |
| --- | --- | --- |
| Human | `HumanMember` | May own Work and remain mandatory for policy-selected legal, financial, credential, or irreversible effects. |
| Agent | `AgentMember` | May own Work, belong to an Agent Team, and bind to replaceable MemberRuns/native sessions. |
| External | `ExternalParticipant` | Receives only explicit time- and scope-bounded visibility or Work. |
| Service | `ServiceActor` | Performs declared automation; never impersonates a Human or AgentMember. |

An `AgentMember` is the durable organization identity. `MemberRun`, runtime
process, provider-native session, and current writable Workspace are execution
bindings. A runtime may crash, resume, or be replaced without creating a new
organizational person. Conversely, a running provider process does not grant
membership, Work authority, or acceptance authority.

## Agent Team topology (flat)

`AgentTeam` is the unit of local administration:

```text
AgentTeam
- id
- name
- description
- mission_id
- host_agent_id
- node_id
- member_ids[]
- status
```

Topology is flat. There is no `parent_team_id` and no child Team concept in the
current model. The system never infers ancestry from display names, Documents,
Work assignees, provider sessions, or filesystem paths.

Humans, external participants, and services may be represented in related
organization records and projections without becoming AgentTeam runtime
members. `OrgUnit` may remain as a business grouping or view, but it is not a
second agent scheduling hierarchy.

## Work is the administrative language

Work is the responsibility kernel connecting Organization to Execution:

```text
Work
  -> one Team scope
  -> optional assignee AgentMember
  -> status: open, in_progress, blocked, review, done, cancelled
  -> optional document_refs (links to Docs)
  -> optional labels (filtering)
```

Agent Team presents the execution board. Organization provides a portfolio view
across all Teams. Both read the same Work identity and lifecycle.

## Administration boundary

- A Team Host may assign and review Work for its own Team's Members.
- A Member cannot assign a same-level peer unless it is that Team's Host.
- Business modules still govern sensitive effects (approvals, finance, legal).

## Standing Agents

Standing Agents are durable agent identities not tied to any single Team Run.
They can be scheduled (e.g. periodic governance audits of Docs and Works) or
role-based (e.g. a Chat governance agent). Their lifecycle is managed by the
Company, not by a Team Run.
