# ADR 0051: Nested Agent Teams Are The Agent Organization

```text
status: accepted target contract; implementation pending
date: 2026-08-04
owner_role: product-architecture
```

## Context

Company OS introduced a separate StandingAgent organization identity,
Organization membership/reporting records, Company Assignment, AgentMember,
MemberRun, Agent Team Work, and TeamMessage. Each distinction addressed a real
risk, but together they created an administrative path too complex for the
product's core use: one Lead delegates Work to direct Agents, and any Agent may
form a child Team and delegate its owned Work further.

ADR 0050 already made Work the Agent Team scheduling primitive, removed
Assignment Message ownership, and allowed Members to create self-owned,
unassigned, and child Work. The remaining split between Company WorkItem and
Agent Team Work, and between StandingAgent and AgentMember, would recreate the
same coordination burden at the Organization boundary.

## Decision

Adopt [Nested Agent Team Organization](../company-os/nested-agent-team-organization.md).

### AgentMember is the organization-agent identity

AgentMember is durable across MemberRuns, provider processes, native sessions,
and execution attempts. MemberRun and provider-native Session remain the
runtime/execution truth. The current separate StandingAgent record is a
compatibility implementation to be converged through an explicit migration;
new target architecture must not add another durable agent identity.

### Organization is recursive AgentTeam topology

The Lead AgentMember Hosts the root AgentTeam. A direct Member may create and
Host one child AgentTeam, whose Members may repeat the pattern. Host is a
relation to one Team, not a separate global actor. Organization UI is a
recursive projection over explicit Team parent, Host, and direct-Member refs.

No fixed Governance-Agent topology is required. Docs, Work, CTO, Finance,
Org/HR, or domain roles are ordinary AgentMembers created when useful.

### One Work kernel serves Team and Organization

ADR 0050 Work semantics become the base responsibility model. The target Work
is Team-scoped beyond one TeamRun and may hold optional business relations for
Document, Milestone, Module, Approval, Finance, Mission, or external delivery.
Organization Work views and Team boards read the same owner, status, event, and
delivery truth.

The current Company WorkItem remains implementation truth until explicit
cutover. This decision supersedes ADR 0050's statement that Company WorkItem
must remain a permanently separate responsibility lifecycle. It does not
collapse Approval, Finance, Document, Mission, or provider-native execution
truth into Work.

### Administration follows direct topology

Every Member may create unassigned Work, self-owned Work, and child Work below
Work it owns. A Team Host may assign and review Work for itself and its direct
Members. A Member cannot assign a same-level peer unless it is that Team's
Host. A child Host cannot administer siblings or ancestors. Delegation of child
execution does not transfer the parent's accountability upward.

This topology is the V1 scheduling permission model. Business modules still
govern sensitive effects and child execution cannot exceed its parent's
workspace, provider-budget, or business-access ceiling.

### Supervising Operator is global read and intake

The Supervising Operator may inspect every Team and active Work, create
unassigned Work in an explicit Team scope, message the Lead, and request
runtime controls. It does not become a Member, impersonate a Member, directly
assign peers, accept Work, or mutate Team topology.

### Message remains conversation

Message may link Work but never establishes or changes responsibility. Work
assignment creates WorkDelivery. Runtime Supervisor retains durable delivery,
busy/idle/recovery, interrupt, resume, Close, and exactly-one-driver semantics.

### Mission/Wave remains optional

Mission/Wave continues to own durable outcome and Host plan/replan history for
long-horizon work. It is not required for ordinary Organization work and never
owns task assignment.

## Amendments to earlier decisions

- ADR 0027 remains active for Docs, human/external actors, Approval, Finance,
  and Company OS product direction; its separate agent-organization scheduling
  interpretation is superseded.
- ADR 0045 remains historical implementation truth for the current explicit
  StandingAgent-to-AgentMember join; its two-identity target is superseded.
- ADR 0046 remains active for Supervising Operator and Runtime Supervisor
  separation; its separate durable StandingAgent target is superseded.
- ADR 0047/0048 remain relevant only for sensitive Company effects. Routine
  Work/Team administration uses the topology rules in this decision rather
  than requiring the proposed authority-broker graph.
- ADR 0049 runtime lifecycle remains active.
- ADR 0050 Work, WorkEvent, WorkDelivery, Message, status, claim, and TeamRun
  completion decisions remain active; its permanently separate Company
  WorkItem lifecycle is superseded by the target unified Work kernel.

## Consequences

- The Organization mental model becomes the recursive version of the Agent
  Team model already used for execution.
- Lead and lower Hosts use the same Work and Message protocols.
- Organization and Team UI can reuse the accepted Team War Room and Member
  Focus components without inventing a second scheduling surface.
- Parent accountability and direct-child authority remain visible at every
  level.
- Current schemas, stores, CLI/API, Company projections, Skills, fixtures, and
  active data need a breaking, verified migration; the ADR itself changes no
  live Store or authority.

## Rejected alternatives

### Keep StandingAgent and AgentMember as permanent separate identities

Rejected. It makes every Agent view, Work assignment, Inbox delivery, and
runtime recovery depend on an avoidable cross-store join.

### Keep Company WorkItem and Team Work as permanent responsibility systems

Rejected. It requires owner/status reconciliation between two Kanbans and
recreates hidden scheduling at every delegation boundary.

### Let any Member assign any peer

Rejected. Responsibility becomes unstable and hierarchy stops constraining
administration. Peers may create unassigned Work and communicate; Hosts assign.

### Make the Supervising Operator the root Host

Rejected. The current AI task is replaceable and should not own Company
identity or silently accept Agent results.

### Require Mission/Wave for Organization work

Rejected. Mission/Wave is valuable for long-horizon judgment, not ordinary
queue and hierarchy management.

## Validation

The decision becomes implemented only when:

1. the Store proves an explicit acyclic Lead -> second-level -> third-level
   nested Team tree;
2. Members create unassigned and self-owned Work, while peer assignment is
   rejected and direct Host assignment succeeds;
3. a child Host delegates owned Work and remains accountable for the parent;
4. Agent Team and Organization views read one Work owner/status projection;
5. WorkDelivery and Message behavior pass idle, busy, crash/recovery, Close,
   Reopen, and Retire tests;
6. the Supervising Operator creates unassigned Work and messages the Lead but
   cannot impersonate, assign, accept, or mutate topology;
7. UI reconstructs Team ancestry, Work lineage, runtime state, and source/result
   relations without inference; and
8. a real AgentOS dogfood run uses Lead -> CTO -> child Team to implement and
   review a repository change, then promotes the accepted result to Docs.
