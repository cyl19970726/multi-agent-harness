# Agent Team Works

```text
status: accepted product direction; implementation pending
owner_role: execution-foundation
canonical_for: Agent Team Work, shared Kanban, assignment/claim, Work delivery,
  Message boundary, child delegation, and Mission/Wave relationship
decision: ADR 0050
```

## Outcome

Every `AgentTeamRun` has one shared `Works` board. The reusable `AgentTeam`
page may aggregate several run boards as a read-only index, but it never merges
their identities or versions. The board makes current demand, ownership,
readiness, blockers, review, and completion queryable without reconstructing
Team messages.

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

The product name is **Works**. `Work` is the provider-neutral Agent Team
responsibility contract. Company `WorkItem` remains a separate
business-governance object. A `source_work_item_ref` may link one Company
WorkItem to one or more Agent Team Works, but Work status never silently
mutates WorkItem responsibility, approval, finance, or closure. Shared names
are a semantic mapping, not storage inheritance.

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
  parent_work_id?                 # same TeamRun only
  source_work_item_ref?
  title
  context_markdown
  completion_criteria_markdown
  status
  owner_member_id?
  active_member_run_id?
  claim_mode
  eligible_member_ids[]
  prerequisite_work_ids[]         # same TeamRun only
  priority
  created_by_actor
  result_summary?
  blocker_reason?
  artifact_refs[]
  check_refs[]
  version
  created_at / updated_at
```

`owner_member_id` expresses stable responsibility. `active_member_run_id` is the
current execution binding and may change generation or resume without losing
ownership. A Provider process exit never clears the owner.

V1 scopes Works to an `AgentTeamRun`. Persistent Organization Teams may use a
long-lived TeamRun as an execution mechanism; the TeamRun never becomes the
Organization identity or business scheduler. Lifting a board to reusable
`AgentTeam` identity is a later decision, not required to prove the initial
model.

## Source Of Truth And Command Transaction

`WorkEvent` is the authoritative append-only transition history. `Work` is its
latest rebuildable projection. A successful command is one logical transaction:

```text
compare expected Work version
  -> append exactly one WorkEvent
  -> update the Work projection
  -> enqueue zero or more WorkDelivery outbox rows
```

The store must not expose an updated Work without its event, or an event without
the matching projection. Stores without a physical transaction use one
crash-recoverable append batch and replay the outbox idempotently.

```text
WorkEvent
  event_id
  team_run_id / work_id
  sequence
  transition
  expected_version / resulting_version
  performed_by_actor
  authority_actor?                # when a Human/Operator acts for the Host
  causation_ref?                  # Message, Work, WorkItem, or control record
  idempotency_key
  payload
  created_at

WorkDelivery
  work_event_id
  recipient_member_run_id
  work_id / resulting_version
  state                           # queued|claimed|provider_received|acked|failed
  claim / lease / receipt facts
```

`(team_run_id, idempotency_key)` identifies one command retry.
`(work_event_id, recipient_member_run_id)` identifies one delivery. Readiness
changes caused by a prerequisite append a distinct `WorkBecameReady` event on
the dependent Work, increment its version, and create its delivery if owned.

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
ready      = status == open && every prerequisite Work is done
```

Minimal `prerequisite_work_ids` answers only whether a Work is ready. It does not
add conditions, branches, loops, retries, Wave barriers, or a universal Task
Graph. Dynamic Workflow continues to own deterministic workflow steps.

When the last blocker completes, readiness changes automatically. An assigned
Work creates a new WorkDelivery for its owner; an unassigned Work appears in the
ready pool. Readiness never auto-assigns a Member and never starts two active
Works on one Member.

If a prerequisite is cancelled, the dependent Work does not become ready. It
receives `WorkPrerequisiteCancelled` and requires the Host to replace/remove
the prerequisite, explicitly block it with a reason, or cancel it.

## Canonical Transitions

Every mutation requires `expected_version`, `idempotency_key`, and the actual
`performed_by_actor`. Member authority comes from the bound runtime/session,
not a caller-supplied member name. Unbound local CLI calls are Host/Operator
actions. `VERSION_CONFLICT` returns the latest Work rather than silently
retrying with changed intent.

| Command | Actor and precondition | Result | Delivery / notable error |
| --- | --- | --- | --- |
| create | Host; or active Member creating self-owned/unassigned | `open`, version 1 | owned Work queues delivery; `INVALID_OWNER` |
| assign | Host; `open`, not provider-received by another owner | owner changes, stays `open` | invalidate unclaimed old delivery; queue new; `RECONCILIATION_REQUIRED` |
| claim | active, eligible, idle/capacity-available Member; unowned ready `open` | owner set and `in_progress` atomically | winner receives delivery; loser `CLAIM_LOST` with latest Work |
| start | owner Member; ready assigned `open`; no other active Work unless configured capacity permits | `in_progress` | provider cycle may start; `MEMBER_BUSY` |
| release | owner Member or Host; `open` and not provider-received | owner cleared, remains `open` | invalidate unclaimed delivery; `RECONCILIATION_REQUIRED` otherwise |
| block | owner or Host; `in_progress`; non-empty reason | `blocked`, owner retained | notify Host; `BLOCKER_REASON_REQUIRED` |
| resume work | owner or Host; `blocked`; blocker resolution recorded | `in_progress` | queue latest version if runtime idle |
| submit | owner; `in_progress`; result summary and required evidence/check refs present | `review` | notify Host; `RESULT_REQUIRED` |
| request changes | Host; `review`; non-empty reason | `in_progress`, owner retained | notify owner |
| accept | Host only; `review` | `done` | reviewer Member never gains accept authority |
| cancel | Host; any non-terminal state; non-empty reason | `cancelled` | provider-received Work requires reconciliation |
| reprioritize/update criteria | Host, or owner within granted scope | status/owner retained; version increments | notify owner when execution meaning changed |

The Host is not implicitly a MemberRun. It assigns and accepts Work but may
claim or execute Work only through an explicit Lead MemberRun. UI wording is
`Awaiting Host acceptance`, not generic `Review complete`.

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
increments the Work version, invalidates any `queued` delivery to the previous
owner, and creates a WorkDelivery for the new owner. A delivery already accepted
(`claimed` or `provider_received`) requires explicit
interrupt/reconciliation before reassignment so
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
| claim eligible Work | only through explicit Lead MemberRun | yes | only through explicit Lead MemberRun |
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
  response_intent                 # informational|response_required
  correlation_id
  reply_to?
  delivery facts
```

Question, answer, discussion, and peer coordination are conversation intents,
not Work states. V1 does not add a broad message-kind taxonomy; minimal
`response_intent` exists only so runtimes can distinguish bounded unread
context from a response-required wake/fence. Assignment, blocker, submission, changes requested, acceptance,
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

Creating Work from a Message records `causation_ref = TeamMessage(id)` on the
`WorkCreated` event and a client idempotency key. It may create several Works
only through several explicit commands. The original Message remains immutable;
its UI reverse link is derived from WorkEvents. The modal prefills title/context,
requires completion criteria, never infers an owner from recipients, and allows
an ordinary Member to choose only self-owned or eligible unassigned Work. The
Host may additionally select any eligible Member.

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

A Member may own several open Works. V1 capacity defaults to exactly one active
`in_progress` Work unless a concrete configured capacity says otherwise.
Provider-native subagents remain internal to that Work's owner.

## Busy, Idle, Crash, And Resume

| Runtime state | Work behavior |
| --- | --- |
| idle | assigned Work wakes the Member; otherwise it may claim ready work |
| working | new assignment queues until the next safe boundary |
| waiting interaction | ownership remains; resolve the matching interaction first |
| crashed/disconnected | ownership remains; no automatic peer claim |
| resumed | same Member identity and a compatible native Session reconcile Work version and deliveries before continuing |
| closed | unfinished Works require explicit reassign, cancel, or Reopen |
| retired | Works require explicit reassign or cancel; ordinary delivery cannot revive the Member |

Transport receipt, Member start, Provider completion, Work submission, and Host
acceptance are separate facts.

When the native Session cannot resume, the Host creates a replacement
MemberRun/session and appends `WorkRebound` to update
`active_member_run_id`. Stable `owner_member_id` remains unchanged; the prior
binding remains evidence. Existing queued deliveries freeze while a Member is
closed, but new deliveries to a closed Member are rejected. Reopen or reassign
must reconcile any claimed/provider-received delivery first.

## Child Delegation And Organization

A Member assigned a parent Work remains accountable when it delegates:

```text
Root Host -> parent Work owned by CTO Agent
CTO Agent -> child Team as Host
child Team -> Backend Work + Frontend Work + Review Work
CTO Agent -> integrates child results -> submits parent Work
Root Host -> accepts or requests changes
```

Same-run child Works may link `parent_work_id`. Cross-Team delegation uses:

```text
WorkDelegation
  parent_work_ref { team_run_id, work_id }
  parent_owner_member_run_id
  child_agent_team_id
  child_team_run_id
  child_host_actor
```

Creation requires the same Execution Space, an authorized parent owner,
acyclic immutable lineage, and an active child TeamRun. Closing or losing the
child run produces an explicit partial/blocked roll-up; it never erases the
relation. Child completion updates a roll-up projection but never automatically
completes or accepts the parent. The parent owner owns integration and
correction.

Organization may select this mechanism: multi-level Org Agents can create
nested Agent Teams for execution, but Organization identity, reporting, and
authority remain distinct. The joins are explicit:

```text
StandingAgent <-> AgentMember
Company WorkItem <-> Agent Team Work
Org delegation <-> WorkDelegation
```

Company WorkItem remains the business-governance scheduler; Team Work is one
execution board and cannot approve legal, financial, or organization effects.

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

The Team page uses `Works | Activity | Members`, with Works as the default.
Works owns Kanban/dense-list state and mutations; Activity owns source-labelled
conversation/events; Members owns factual capacity and runtime pressure.
`Owned by me` appears only for a participating Member, otherwise the filter is
`By owner`. Capacity never invents a percentage without a configured limit.
Full desktop/mobile interaction, density, state, identity, and accessibility
contracts live in [Team Workbench](../dashboard/pages/team-run-war-room.md) and
[the implementation/acceptance plan](../design/agent-team-works-implementation-plan.md).

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

The deterministic, responsive, accessibility, and real-provider gates are
normative in the linked implementation/acceptance plan.
