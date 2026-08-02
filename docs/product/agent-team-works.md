# Agent Team Works

```text
status: accepted product direction; implementation pending
owner_role: execution-foundation
canonical_for: Agent Team Work, shared Kanban, assignment/claim, Work delivery,
  Message boundary, child delegation, and Mission/Wave relationship
decision: ADR 0050
```

## Outcome

Every Agent Team has one shared `Works` surface. It makes current demand,
ownership, readiness, blockers, review, and completion queryable without
reconstructing Team messages.

The smallest mental model is:

```text
Agent          = responsible actor
Work           = durable responsibility and current state
Kanban         = a view over Works, never a second source of truth
TeamMessage    = authored conversation, optionally linked to Work
WorkEvent      = append-only Work state transition
WorkDelivery   = reliable delivery of a Work change to a Member runtime
Native Session = execution transcript, tools, commands, and turn truth
```

The product name is **Works**. `Work` is the provider-neutral base contract.
Company `WorkItem` later extends the same core with company provenance,
Milestone, Docs, Approval, Finance, GitHub, and Organization relations.

## Why Work Is Not A Message

A Message explains or negotiates work. It does not own title, completion
criteria, current owner, status, readiness, result, or acceptance.

New work assignment is therefore not `TeamMessage(kind=assignment)`:

```text
assign Work W-12 to Member A
  -> append WorkAssigned
  -> update the Works projection
  -> create WorkDelivery for Member A
  -> Supervisor injects a bounded Work envelope at a safe boundary
```

`WorkDelivery` may reuse the mailbox's claim, lease, provider-receipt, ACK, and
recovery machinery. It is not an authored chat message and does not duplicate
the Work body as canonical truth.

There is no compatibility path for Assignment Messages. The implementation
removes new and historical Assignment-message interpretation after preserving
the research evidence needed to explain the decision. Active dogfood stores are
reset or explicitly migrated to Works; runtime code does not maintain two
ownership models.

## Work Core

The initial contract is intentionally small:

```text
Work
  id
  team_run_id
  parent_work_id?
  source_work_item_ref?
  title
  context_markdown
  completion_criteria_markdown
  status
  owner_member_id?
  active_member_run_id?
  claim_mode
  eligible_member_ids[]
  blocked_by_work_ids[]
  priority
  created_by_actor
  result_summary?
  artifact_refs[]
  check_refs[]
  version
  created_at / updated_at
```

`owner_member_id` expresses stable responsibility. `active_member_run_id` is the
current execution binding and may change generation or resume without losing
ownership. A Provider process exit never clears the owner.

V1 scopes Works to an `AgentTeamRun`. Persistent Organization Teams use a
long-lived TeamRun. Lifting a board to reusable `AgentTeam` identity is a later
decision, not required to prove the initial model.

## Status And Readiness

The only statuses are:

```text
open -> in_progress -> review -> done
           <-> blocked
open|in_progress|blocked|review -> cancelled
review -> in_progress  # changes requested
```

Meanings:

| Status | Meaning |
| --- | --- |
| `open` | created but not executing; may be assigned or unassigned |
| `in_progress` | the owner is actively responsible for execution |
| `blocked` | ownership remains, with a required blocker reason |
| `review` | result submitted; owner remains responsible for correction |
| `done` | accepted by the Host of this Team, not merely Provider-completed |
| `cancelled` | intentionally ended while preserving history |

Assigned/unassigned and ready/not-ready are independent projections:

```text
assigned   = owner_member_id != null
unassigned = owner_member_id == null
ready      = status == open && every blocked_by Work is done
```

Minimal `blocked_by_work_ids` answers only whether a Work is ready. It does not
add conditions, branches, loops, retries, Wave barriers, or a universal Task
Graph. Dynamic Workflow continues to own deterministic workflow steps.

When the last blocker completes, readiness changes automatically. An assigned
Work creates a new WorkDelivery for its owner; an unassigned Work appears in the
ready pool. Readiness never auto-assigns a Member and never starts two active
Works on one Member.

## Assignment And Claim

### Allocation rule

The board, not chat history, is the allocation surface. Every schedulable piece
of responsibility is created once as Work and then follows exactly one of two
paths:

```text
direct allocation: Host creates or assigns Work -> owner receives WorkDelivery
shared pool:       Host or Member creates eligible unassigned Work
                   -> one eligible Member atomically claims it
```

Creation and assignment may be one atomic operation. A separate authored
Message is optional explanation, never the allocation primitive. Reassignment
increments the Work version, invalidates any unclaimed delivery to the previous
owner, and creates a WorkDelivery for the new owner. A delivery already accepted
by a Provider requires explicit interrupt/reconciliation before reassignment so
two Members do not act on one writable responsibility.

### Host assignment

The Host may create, assign, reassign, reprioritize, cancel, request changes,
and accept any Work in its TeamRun.

Assign changes owner but leaves status `open`. The Member explicitly starts it.
An idle managed Member is woken after WorkDelivery is claimed. A busy Member
receives the Work at the next safe boundary; ordinary assignment never silently
interrupts an active turn.

### Member self-claim

An eligible Member may atomically claim one ready unassigned Work. Claim sets
owner and `in_progress` in one transition. Compare-and-append semantics prevent
two Members from claiming the same version.

V1 claim policy is:

```text
claim_mode = host_assign | team_claim
eligible_member_ids = []       # every active Member
eligible_member_ids = [A, B]   # bounded pool
```

No scoring, bidding, model marketplace, or automatic capability optimizer is
needed for V1.

## Creation Authority

Creation must reduce Host bottlenecks without allowing peers to silently assign
each other.

| Action | Team Host | Ordinary Member | Host of a child Team |
| --- | ---: | ---: | ---: |
| create top-level Work | yes | self-owned or unassigned | yes, in child Team |
| assign to any Member | yes | no | yes, in child Team |
| create child Work | yes | under Work it owns | yes |
| claim eligible Work | yes | yes | yes |
| update owned Work | yes | yes | yes |
| change peer ownership | yes | no | yes, for child Members |
| submit for review | yes | yes | yes |
| accept Work | yes | no | yes, for child Works |
| cancel arbitrary Work | yes | no | yes, in child Team |

An ordinary Member may create an unassigned Work when it discovers necessary
follow-up. The Host can reprioritize, merge, assign, or cancel it. This prevents
every discovered issue from becoming another Host conversation round.

Direct peer assignment is not allowed. A Member may create an eligible
unassigned Work and ask a peer to claim it. When a Member has permission to
create a child Team, it becomes that Team's Host and can assign child Works
directly. V1 needs only the structural Host/Member distinction plus
`can_create_child_team`; it does not require a general RBAC engine.

## Message Boundary

New authored Team messages use one small conversational model:

```text
TeamMessage
  id
  team_run_id
  sender / recipients
  body_markdown
  work_id?
  correlation_id
  reply_to?
  delivery facts
```

Question, answer, discussion, and peer coordination are conversation intents,
not Work states. Assignment, blocker, submission, changes requested, acceptance,
and cancellation are Work operations with WorkEvents.

Rules:

1. a Message may exist without a Work;
2. one Work may have many Message correlations;
3. a Message never changes owner or status by itself;
4. a conversational request that creates durable action must create or update a
   Work; and
5. Provider final text is not automatically a Message, Work result, or Host
   acceptance.

The practical interaction table is:

| Intent | Canonical operation | Optional conversation |
| --- | --- | --- |
| give responsibility | create/assign or claim Work | explanation linked by `work_id` |
| change scope or criteria | update Work and emit WorkDelivery | explain why in a linked Message |
| ask or answer | TeamMessage | link `work_id` when relevant |
| report a blocker | block Work with structured reason | discuss alternatives in Messages |
| discover follow-up | create self-owned or unassigned Work | notify Host/peer when useful |
| return a result | submit Work with result/evidence refs | add a concise review note |
| request changes | Work review action | explain required changes in a linked Message |
| accept completion | accept Work | optional acknowledgement |

If a conversation creates a durable obligation, the sender or responsible Host
creates Work. The UI offers **Send message** and **Create Work from message** as
separate actions. It never guesses that a sentence is an assignment.

`PendingInteraction` remains separate for a Provider turn actually paused on a
question or authorization request.

## Efficient Member Context

At each safe execution boundary the Member receives:

1. one active Work with id, version, context, completion criteria, owner, and
   relevant constraints;
2. bounded summaries of assigned open Works;
3. eligible ready unassigned Works;
4. unread Work-linked and ordinary Messages; and
5. the existing Workspace and provider-native Session binding.

Only the active Work carries full context. Queue and ready-pool entries are
bounded summaries (`id`, title, owner/readiness, priority, version). The Member
fetches a full record only when it starts or claims it. This avoids copying the
whole board and repeating stale criteria in every prompt.

The delivery envelope cites `work_id` and `version`. The Member or adapter reads
the latest Work before side effects. Updating Work increments its version and
creates another WorkDelivery when the change affects the owner.

A Member may own several open Works but should have one active `in_progress`
Work by default. Provider-native subagents remain internal to that Work's owner.

## Busy, Idle, Crash, And Resume

| Runtime state | Work behavior |
| --- | --- |
| idle | assigned Work wakes the Member; otherwise it may claim ready work |
| working | new assignment queues until the next safe boundary |
| waiting interaction | ownership remains; resolve the matching interaction first |
| crashed/disconnected | ownership remains; no automatic peer claim |
| resumed | same Member identity and native Session reconcile Work version and deliveries before continuing |
| closed | unfinished Works require explicit reassign, cancel, or Reopen |
| retired | Works require explicit reassign or cancel; ordinary delivery cannot revive the Member |

Transport receipt, Member start, Provider completion, Work submission, and Host
acceptance are separate facts.

## Child Delegation And Organization

A Member assigned a parent Work remains accountable when it delegates:

```text
Root Host -> parent Work owned by CTO Agent
CTO Agent -> child Team as Host
child Team -> Backend Work + Frontend Work + Review Work
CTO Agent -> integrates child results -> submits parent Work
Root Host -> accepts or requests changes
```

Child Works link `parent_work_id`. Child completion updates a roll-up projection
but never automatically completes or accepts the parent. The parent owner owns
integration and correction.

Organization later reuses this exact mechanism: multi-level Org Agents are
multi-level Agent Teams plus Organization identity, reporting, and authority.
Company WorkItem adds business governance but does not create a second task
scheduler.

## Mission And Wave

Works do not replace Mission:

```text
Mission = why, durable outcome, boundary, shared context, and closeout
Wave    = what changed in Host judgment and why the plan was revised
Works   = what currently exists, who owns it, and its execution state
```

Works replace Assignment messages and remove task enumeration from Wave prose.
A Wave cites important Work ids only when recording a material re-plan,
composition change, integration decision, or advance judgment.

Mission remains optional. A standalone Agent Team may operate only with Works.
Use Mission when one durable outcome spans several Teams, Workflows, Host work,
or important plan revisions. Wave remains a lightweight memo, never a task
container or synchronization barrier.

## Agent Team Workbench

The Agent Team page becomes a Team workbench with three primary views:

```text
Works | Activity | Members
```

`Works` is the default operational view:

- top filters: My Works, Unassigned, Assigned, Blocked, Review, All;
- Kanban columns: Open, In progress, Blocked, Review, Done;
- optional dense list for large Teams;
- cards show title, owner/avatar, readiness, priority, completion preview,
  blockers, child progress, source WorkItem, unread thread count, and update
  time;
- actions include create, assign, claim, start, block, submit, request changes,
  accept, release, cancel, and delegate; and
- a detail drawer contains full context, criteria, WorkEvent history, linked
  Messages, child Works, artifacts, checks, Workspace, and Session link.

`Activity` remains the group conversation and event timeline. It filters by
Member, Work, conversational intent, and WorkEvent type, but is never the only
source of task state.

`Members` shows each Member's active Work, queued count, blocked/review count,
capacity, runtime status, and child Team. Member detail reuses the same Works
components for My Works, ready pool, delegation, messages, and evidence.

## Acceptance

The model is accepted only when a real mixed-provider Team proves:

- Host assignment delivers one Work without an Assignment Message;
- an eligible idle Member atomically claims one unassigned Work;
- two concurrent claims cannot both succeed;
- a busy Member receives Work at the next safe boundary without interruption;
- crash and Reopen preserve owner, Work version, Member identity, and native
  Session continuity;
- a Member creates follow-up unassigned Work without Host rewriting it;
- a Member delegates to a child Team while retaining parent responsibility;
- question, blocker explanation, peer coordination, and review discussion link
  to Work without becoming task state;
- Host can reconstruct all current work from Works without reading messages;
  and
- standalone Team operation and Mission-scoped operation both work.
