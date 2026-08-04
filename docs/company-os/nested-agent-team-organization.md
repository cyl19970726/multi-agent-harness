# Nested Agent Team Organization

```text
status: canonical target product contract; implementation pending
owner_role: product-architecture
canonical_for: AgentMember-based Organization, recursive Agent Teams, unified
  Work administration, and Supervising Operator boundary
```

## One minimal administrative model

The Agent organization uses the Agent Team foundation directly:

```text
Supervising Operator
  <-> Lead AgentMember (Host of root AgentTeam)
        -> direct AgentMembers
             -> optional child AgentTeam hosted by that Member
                  -> direct child AgentMembers
```

An AgentMember may execute Work and may also Host one child Team. The same
pattern repeats recursively. Organization is the durable multi-level view of
these relations, not a separate scheduler, assignment service, or provider
runtime hierarchy.

`Host` is relative to one Team. The Lead Hosts the root Team. A Member that
creates a child Team becomes that Team's Host. A Member remains accountable to
its parent Host while managing its own direct children.

## Durable identity and replaceable runtime

```text
AgentMember -> MemberRun -> provider-native Session
```

AgentMember owns durable name, role, Team location, provider/workspace policy,
and business-access ceiling. MemberRun owns current runtime state. The Provider
owns its transcript and tool activity. Restart, Resume, Interrupt, Close, or
provider replacement never creates a new organization identity.

Provider-native subagents remain internal execution details unless a Host
explicitly creates a durable AgentMember for them.

## One Work kernel

Work is the shared responsibility object for Agent Team and Organization:

```text
Work
  -> one Team scope
  -> optional assignee AgentMember
  -> optional parent Work
  -> status, context, completion criteria, result, evidence
  -> optional Document, Milestone, Module, Approval, Finance, Mission, or
     external-delivery relations
```

Agent Team presents the execution board. Organization adds recursive Team,
business-source, milestone, and global portfolio views. They read the same Work
identity and lifecycle.

Assigned/unassigned is derived from whether the Work has an assignee. It is not
a second state machine. Ordinary status remains `open`, `in_progress`,
`blocked`, `review`, `done`, or `cancelled`.

The existing Company WorkItem and Agent Team Work models are current
implementation truth. The target removes their duplicate responsibility state
through an explicit cutover; it does not preserve a dual-write compatibility
path.

## Creation and assignment

Every Member may:

- create unassigned Work in its current Team;
- create Work assigned to itself;
- update and submit Work it owns; and
- create child Work beneath Work it owns.

A Team Host may additionally:

- assign Team Work to itself or a direct Member;
- reassign, request changes, accept, or cancel Work in that Team; and
- create direct Members and a child Team within the Host's execution ceiling.

An ordinary Member cannot force assignment to a same-level peer. Unassigned
Work is allocated by the Host unless its creator takes it or the Work is
explicitly claimable.

## Delegation preserves accountability

When a Member delegates to its child Team, it creates child Work linked to the
parent Work. Children own their child Works. The delegating Member remains
responsible for the parent Work, reviews child results, integrates them, and
submits upward. A child completion never auto-completes its parent.

This creates local autonomy without forcing the Lead to coordinate every
grandchild:

```text
Lead assigns W0 to CTO
  -> CTO remains accountable for W0
  -> CTO child Team executes W1, W2, W3
  -> CTO reviews/integrates children
  -> CTO submits W0 to Lead
```

## Every Agent continuously discovers the next Work

Lead, every lower Host, and every ordinary Member are active Work-discovery
nodes. While performing or reviewing Work, reading Docs, observing runtime and
code, or receiving external facts, they continuously ask what new demand has
appeared.

Each observation becomes one of three permitted Work placements:

1. **self-owned** — the Member owns it and can execute within its ceiling;
2. **unassigned** — the current Team Host must prioritize or allocate it; or
3. **direct-child assignment** — the Member Hosts a child Team and assigns it
   to one of that Team's direct Members.

A Member cannot use this loop to assign a same-level peer or unrelated Team.
When the right owner is outside its authority, it preserves the observation as
unassigned Work with source provenance and notifies the appropriate Host.

This is the company's self-evolution mechanism: accepted results, defects,
reviews, document gaps, and operating signals continuously produce the next
inspectable Work cycle. It is not uncontrolled auto-execution. Hosts still
prioritize, deduplicate, limit capacity, and accept results.

## Message is conversation

Message carries questions, answers, progress, blocker explanation, steer,
review discussion, and peer coordination. It may link Work. It does not assign,
start, submit, accept, or complete Work.

Work assignment creates WorkDelivery for the target runtime. The Runtime
Supervisor claims and injects delivery according to busy, idle,
offline/recovered, closed, and retired state. Assignment Message does not
return.

## Topology is the scheduling permission model

V1 keeps administration intentionally small:

- a Member controls its own Work;
- a Host controls one directly hosted Team;
- a child Host controls only its child Team;
- no Agent administers siblings, ancestors, or unrelated Work; and
- child execution scope cannot exceed its parent's workspace, provider budget,
  or business-access ceiling.

Business effects remain separate. Team topology cannot authorize payments,
legal filings, credential changes, irreversible external actions, or scope
expansion. Those actions follow the owning module's explicit policy.

## Supervising Operator

The Human and current AI task may act together as a Supervising Operator. It
can read the complete Organization and active Work projection, create
unassigned Work in any explicit Team scope, send durable Messages to the Lead,
and request runtime controls.

It does not become an AgentMember, impersonate a Member, assign peer Work,
accept Member results, or silently change Team topology. Root assignment and
acceptance remain the Lead's responsibility.

## Product surfaces

Organization renders the recursive Team tree. Global Works aggregates the same
Work rows across that tree. Member Focus reuses the Agent Team member workspace
and adds created Work, child Team, and delegation controls. Team War Room keeps
Works, Activity, Members, mailbox, and truthful runtime capacity.

Views distinguish discovered-unassigned, self-owned, delegated, and follow-up
Work and show the source observation that created each row. This makes
self-evolution visible and governable instead of hiding it in Message history.

Every relationship is explicit. UI must not infer ancestry, responsibility, or
runtime health from matching names, provider sessions, document authorship, or
first-row fallback.

## Mission/Wave

Mission/Wave remains optional. Use it when an outcome spans several Teams or
the Host needs durable long-horizon plan and replan history. Work still owns
responsibility and state; Mission/Wave never becomes the Organization tree or
task board.

## Implementation spec and decision

- [Requirements](../../specs/nested-agent-team-organization/requirements.md)
- [Technical design](../../specs/nested-agent-team-organization/design.md)
- [Implementation plan](../../specs/nested-agent-team-organization/tasks.md)
- [ADR 0051](../decisions/0051-nested-agent-teams-are-the-agent-organization.md)
