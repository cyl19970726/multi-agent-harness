# Note: nested AgentTeam topology has been removed. See docs/mental/agent-firm-mental-model.md for current flat model.

# Organization and Actors

```text
status: canonical Company OS contract
owner_role: product
canonical_for: recursive AgentTeam organization, actor identity, responsibility, and administration boundary
```

## Purpose

Company OS needs a durable answer to four questions:

1. who exists;
2. which Team each actor belongs to;
3. who may assign and accept Work for that Team; and
4. where responsibility moves when an actor delegates downward.

The agent organization is not a separate scheduler layered above Agent Team.
It is the persistent projection of recursive Agent Teams themselves.

```text
Human Owner / Supervising Operator
  -> Root Team Host (Lead AgentMember)
     -> direct AgentMembers
        -> optional child Team hosted by that Member
           -> child AgentMembers
              -> ...
```

This replaces the earlier target of one `StandingAgent` identity joined to a
separate execution `AgentMember`. It also removes the fixed requirement for a
Lead plus four Governance Agents. Docs, Work, Finance, Org/HR, CTO, or another
domain are roles that a company may choose, not hard-coded product layers.

The full target contract and transition are defined by
[Nested Agent Team organization](nested-agent-team-organization.md),
[ADR 0052](../decisions/0052-nested-agent-teams-are-the-agent-organization.md),
and the [implementation Spec](../../specs/nested-agent-team-organization/requirements.md).

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
| Agent | `AgentMember` | May own Work, belong to a parent Team, host a child Team, and bind to replaceable MemberRuns/native sessions. |
| External | `ExternalParticipant` | Receives only explicit time- and scope-bounded visibility or Work. |
| Service | `ServiceActor` | Performs declared automation; never impersonates a Human or AgentMember. |

An `AgentMember` is the durable organization identity. `MemberRun`, runtime
process, provider-native session, and current writable Workspace are execution
bindings. A runtime may crash, resume, or be replaced without creating a new
organizational person. Conversely, a running provider process does not grant
membership, Work authority, or acceptance authority.

## Recursive Team topology

`AgentTeam` is the unit of local administration:

```text
AgentTeam
- id
- name
- purpose
- parent_team_id?
- host_member_id
- member_ids[]
- status
- policy_refs[]
```

The root Team has no `parent_team_id`. A child Team is hosted by one direct
Member of its parent Team. That Member therefore has two simultaneous roles:

- Member in the parent Team, accountable to the parent Host; and
- Host in the child Team, responsible for assigning and accepting child Work.

The topology is explicit and acyclic. V1 permits at most one primary child Team
per hosting Member. The system never infers ancestry from display names,
Documents, Work assignees, provider sessions, or filesystem paths.

Humans, external participants, and services may be represented in related
organization records and projections without becoming AgentTeam runtime
members. `OrgUnit` may remain as a business grouping or view, but it is not a
second agent scheduling hierarchy.

## Work is the administrative language

Organization uses the same `Work` kernel as Agent Team:

```text
Work
- id
- team_id
- parent_work_id?
- creator_actor_ref
- assignee_member_id?
- status
- title / context / completion_criteria
- source_refs[] / artifact_refs[] / check_refs[]
```

Assigned versus unassigned is derived from `assignee_member_id`; it is not a
second lifecycle. Status remains `open`, `in_progress`, `blocked`, `review`,
`done`, or `cancelled`.

Creation and assignment stay deliberately small:

| Actor | May create | May assign | May accept |
| --- | --- | --- | --- |
| Supervising Operator | Unassigned Work in an explicitly selected visible Team | No routine assignment | No |
| Ordinary Member | Unassigned or self-owned Work in its current Team; child Work under Work it owns | Self only in parent Team | No peer Work |
| Team Host | Any Work in its direct Team | Direct members or unassigned pool | Work in its direct Team |
| Child Team Host | Any Work in its child Team | Direct child members | Child Team Work |

No universal permission engine is required for this routine scheduling path.
Team topology supplies the minimum authority boundary. Product modules may add
sensitive-effect policies, but they must not make ordinary task delegation wait
for layered Company approvals.

### Continuous Work discovery

Every AgentMember—not only the Lead—continuously turns observations from its
current Work, Docs, code, runtime, reviews, or external facts into explicit
Work. It may assign suitable Work to itself, leave uncertain ownership
unassigned for its current Host, or assign it to a direct child when it Hosts a
child Team. Source provenance explains why the Work exists.

This is how the company evolves from its own operation. Topology still limits
placement, and Hosts still control priority, capacity, duplication, and
acceptance; discovery does not grant cross-Team assignment or automatic
permission to execute every idea.

## Delegation preserves accountability

Delegation creates child Work; it does not transfer the parent's promise.

```text
Lead assigns W0 to CTO in root Team
  -> CTO creates child Team
  -> CTO creates W1, W2, W3 with parent_work_id = W0
  -> child Members execute and submit W1/W2/W3
  -> CTO accepts or requests changes
  -> CTO integrates results and submits W0 to Lead
  -> Lead accepts or requests changes on W0
```

The parent Work cannot become done merely because one child Work completed.
The parent assignee remains accountable for integration, conflicts, evidence,
and the result returned upward. Child status is visible context, not an
automatic Task Graph.

## Messages are communication, not responsibility

`TeamMessage` carries questions, answers, progress, blockers, reviews, and
coordination. It may link a `work_id`, but sending a Message never creates,
assigns, claims, accepts, or closes Work. Work operations own those transitions.

This boundary lets a busy or offline Member receive durable conversation and
Work deliveries independently. Recovery resumes the same durable AgentMember
and unfinished Work against a compatible native session, or binds a new native
session while keeping the old session as execution evidence.

## Supervising Operator

The current Codex task may act as a Supervising Operator for the Human Owner.
It is an outside control role, not an implicit root AgentMember. It may:

- inspect every visible Team, Member, active Work, runtime, and Message;
- create unassigned intake Work in an explicitly selected Team;
- communicate with the root Lead; and
- diagnose or temporarily drive recovery when the normal path fails.

It does not impersonate a Member, silently assign routine Work to peers, accept
their Work, or become the hidden source of company history. If the Operator is
later provisioned as a real AgentMember, that is an explicit topology change.

## Product UI

Organization UI is a recursive view over native Team, Member, Work, and runtime
records:

- Organization overview shows the root Team and nested child Teams;
- every Member card opens the shared Member Focus page;
- each Team opens the shared Team War Room and Works board;
- Global Works groups Work by Team path, Host, assignee, and status;
- badges distinguish organizational identity from runtime state; and
- no card may claim a role, availability, Work relation, or authority that the
  Store cannot reconstruct.

The complete Agent Team pages are reused. Organization adds hierarchy,
cross-Team Work roll-up, business relations, and Human/external context; it
does not fork a second implementation of member activity or messaging.

## Mission and Mission Log boundary

Mission remains optional. It is useful when a Host needs durable intent,
multi-Team context, or explicit closeout; its append-only Mission Log records
material judgment and re-plan history. Ordinary Organization scheduling uses
Team Works directly and does not require a Mission or Assignment Message.

## Current implementation truth and migration

The repository currently implements Company `StandingAgent`, `OrgUnit`,
`Membership`, Company `WorkItem`, and explicit
`StandingAgent.execution_agent_member_ref` compatibility rows. Those schemas,
stores, CLI commands, and UI projections remain current implementation truth
until an explicit migration/reset lands.

ADR 0052 changes the target architecture; it does not pretend that cutover is
already complete. During transition:

- existing rows remain readable and honestly labelled compatibility data;
- new architecture must not add another join or dual-write path;
- native AgentTeam Works remain the scheduling source of truth; and
- implementation acceptance must prove recursive topology, delegation,
  recovery, global Works, and UI projection before old scheduling semantics are
  retired.
