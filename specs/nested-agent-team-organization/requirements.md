# Nested Agent Team Organization Requirements

```text
status: proposed implementation spec
owner_role: product-architecture
target: persistent recursive Agent Teams as the Organization agent model
```

## Problem

The current Company OS design layers StandingAgent, Organization membership,
Company Assignment, AgentMember, MemberRun, Agent Team Work, and TeamMessage
into one scheduling path. The implementation can represent each fact, but the
operator and every Agent must reconstruct too many joins before answering a
simple question: who owns this work, who may delegate it, and what should run
next?

The target product needs one minimal administrative model:

```text
Host -> direct AgentMembers
AgentMember -> optional child AgentTeam -> direct child AgentMembers
```

Every AgentMember may perform Work. A Member that creates a child Team becomes
that Team's Host and may delegate owned Work to direct children. Organization
is the persistent multi-level projection of these nested Teams. It must not add
a second task system or a second agent identity.

## Goals

1. Make `AgentMember` the durable organization-agent identity and
   `MemberRun`/provider-native Session its replaceable execution state.
2. Make Host a relative role over one Team, not a separate global actor type.
3. Allow recursive but locally governed Agent Team nesting.
4. Reuse one Work kernel for Agent Team scheduling and Organization work.
5. Allow every Member to create unassigned Work or self-owned Work.
6. Allow a Team Host to assign Work inside its directly managed Team.
7. Preserve accountability when an Agent delegates execution to a child Team.
8. Keep Message as communication and Work as responsibility.
9. Give the Supervising Operator global read, unassigned-Work creation, and
   Lead communication without Member impersonation.
10. Provide truthful multi-level Organization and Works views.

## Non-goals

- A general Task Graph or conditional workflow engine.
- A fixed Docs/Work/Finance/Org-HR management hierarchy.
- Free assignment between peer Members.
- Treating provider chat, transcripts, hooks, or native subagents as the Work
  source of truth.
- Requiring Mission/Wave for ordinary Organization work.
- A universal business-permission language in the first implementation.
- Dual-write compatibility between the target Work kernel and the existing
  Company WorkItem / Agent Team Work responsibility models after cutover.

## User stories

### R1 — Recursive Team identity

As the Lead, I want to manage direct AgentMembers and allow a capable Member to
create its own child Team, so that the organization can grow without making the
root Lead manage every executor.

Acceptance:

- When a root organization is created, the system shall represent the Lead as
  one durable AgentMember that Hosts the root AgentTeam.
- When an AgentMember creates a child Team, the system shall record that Member
  as the child Team Host and the child Team's parent as the Member's current
  Team.
- The system shall reject a child Team whose Host is not a direct Member of its
  declared parent Team.
- The same AgentMember may be a Member of its parent Team and Host of one child
  Team without creating a duplicate organization identity.
- Provider restart, runtime replacement, TeamRun completion, or native-session
  replacement shall not change AgentMember identity or the organization tree.

### R2 — Work creation and assignment

As an AgentMember, I want to record newly discovered Work without waiting for
the Host, and either leave it unassigned or take responsibility myself.

Acceptance:

- When a Member creates Work with no assignee, the system shall place it in the
  current Team's unassigned pool.
- When a Member creates Work assigned to itself, the system shall record the
  Member as assignee and deliver the exact Work version to its runtime.
- When a Member attempts to assign a same-level peer, the system shall reject
  the operation unless that Member is the Team Host.
- When a Host assigns unassigned Work, the target shall be the Host itself or a
  direct Member of that Team.
- Unassigned Work shall not be generally claimable unless the creator takes its
  own Work or the Work is explicitly marked claimable.

### R3 — Delegation without lost accountability

As a Member responsible for a larger Work, I want to split it into child Work
for my child Team while remaining accountable to my parent Host.

Acceptance:

- When an owning Member delegates, every child Work shall reference the parent
  Work and the child Team.
- The parent Work shall remain assigned to the delegating Member until its
  parent Host accepts or reassigns it.
- Completion of child Works shall not automatically complete the parent Work.
- The child Team Host shall integrate child results and submit the parent Work
  upward with explicit result and evidence references.
- Parent Hosts shall not need to manage grandchildren directly.

### R4 — One Work kernel

As an operator, I want Agent Team and Organization task views to read the same
Work truth so that status never diverges between two boards.

Acceptance:

- The target model shall have one Work identity, lifecycle, event stream, and
  delivery model.
- Assigned/unassigned shall be derived from optional assignee state rather than
  a second lifecycle.
- Organization concerns such as Document, Milestone, BusinessModule, Approval,
  and external-delivery refs shall extend Work through optional relations.
- Agent Team execution views and Organization views shall not dual-write owner
  or status fields.
- Cutover shall use an explicit migration/reset boundary; active product code
  shall not maintain two responsibility authorities.

### R5 — Message boundary

As a team participant, I want messages for questions and coordination without
making chat a hidden task database.

Acceptance:

- TeamMessage shall optionally reference Work but shall not assign, start,
  submit, accept, or complete it.
- Work assignment shall create WorkDelivery, not an Assignment Message.
- Busy, idle, offline/recovered, and closed delivery behavior shall remain
  deterministic and provider-neutral.
- Provider-native text shall not become a team-visible report unless the
  Member explicitly updates Work or sends a durable Message.

### R6 — Topology-derived administration

As a Team Host, I want authority to follow direct Team ownership so that normal
delegation does not require a large permission framework.

Acceptance:

- Every Member may create unassigned Work, create self-owned Work, and update
  Work it owns.
- A Team Host may manage Work and direct Members only inside that Team.
- A Member that Hosts a child Team may create and assign child Work there.
- No Member may change a sibling Team, ancestor Team, or unrelated Work.
- Child execution scope shall not exceed the parent Member's workspace,
  provider-budget, or business-access ceiling.
- Sensitive business effects remain subject to their owning module policy;
  Team topology does not authorize payments, legal filings, credentials, or
  irreversible external actions.

### R7 — Supervising Operator

As the Human's current AI operator, I want to observe the whole organization,
seed demand, and communicate with the Lead without becoming a hidden company
member.

Acceptance:

- The Supervising Operator shall read the full organization tree and active
  Work projection.
- It may create unassigned Work in a selected Team scope.
- It may send durable Messages to the Lead.
- It shall not impersonate a Member, assign Work to a non-self Member, accept
  Member Work, or silently change Team topology.
- The Lead remains responsible for root-team assignment and acceptance.

### R8 — Operator UX

As an operator, I want to understand the organization and its Work without
opening provider transcripts.

Acceptance:

- Organization shall render the recursive Team tree from native relations.
- A Member node shall expose current owned Work, created Work, child Team,
  Inbox/Outbox, runtime state, and native-session link.
- The global Works surface shall filter by Team path, Host, Member, status,
  source, and milestone.
- Team pages shall continue to use Works, Activity, Members, and mailbox views.
- Work detail shall show parent/child lineage and the accountable Member at
  every level.
- No UI shall infer hierarchy, assignment, or runtime health from names or
  first-row fallback.

### R9 — Optional Mission/Wave

As a Host, I want Mission/Wave only when long-horizon planning or material
replanning is useful.

Acceptance:

- Ordinary Organization Work shall run without Mission/Wave.
- A Work may link a Mission when durable intent spans multiple Teams or Waves.
- Mission/Wave shall not own Work assignment or replace the shared board.

## End-to-end acceptance scenario

1. The Supervising Operator creates an unassigned root Work: “Implement the
   Store-backed Organization view.”
2. The Lead assigns it to the CTO AgentMember.
3. The CTO creates a child Team with Frontend, Runtime, and Reviewer Members.
4. The CTO creates three child Works and assigns them to the direct children.
5. One child asks a Work-linked question; another becomes blocked and resumes;
   none of those Messages changes Work ownership.
6. The Runtime Supervisor proves idle delivery, busy queueing, process restart,
   same-native-session resume where compatible, and exactly-once WorkDelivery.
7. The Reviewer submits its child Work; CTO integrates all accepted child
   results and submits the parent Work.
8. The Lead accepts the root Work and its Document source/result relations.
9. The Organization and global Works pages reconstruct the full tree and Work
   lineage from native Store records.
