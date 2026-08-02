# ADR 0050: Agent Team Works And Message Boundary

```text
status: accepted; implementation pending
owner_role: architecture
canonical_for: Work as the Agent Team scheduling primitive, no Assignment
  Message ownership, shared Kanban, claim authority, and Mission boundary
```

## Context

Agent Team currently proves ownership with
`TeamMessage(kind=assignment) + correlation_id`. A long-running self-hosting
Team grew to 23 MemberRuns and 1,103 messages while retaining no queryable
shared list of assigned, unassigned, ready, blocked, review, and completed work.
The Host repeatedly reconstructed task state from conversation history and idle
Members could not atomically claim known ready work.

The failure reconstruction and Claude Code comparison are preserved in
[Agent Team Shared Task List research](../research/agent-team-shared-task-list.md).

The product also needs one simple execution-board object that Company WorkItem
can reference. Collapsing their storage or lifecycle would create competing
owner/status/approval truths, so the relation must remain explicit.

## Decision

### Work is the base responsibility object

Agent Team adds a TeamRun-scoped `Work` object and a shared `Works` projection.
Company WorkItem remains a separate governed business object and may link one
or more execution Works through `source_work_item_ref`. A Team Work transition
does not mutate WorkItem authority, approval, finance, or closure. Kanban is a
view over Work, not another source of truth.

### Assignment is a Work operation

New ownership is created only through Work assignment or atomic self-claim.
`TeamMessage(kind=assignment)` is removed rather than retained as a new-write or
read-compatibility ownership path.

Assignment appends a `WorkAssigned` event and creates a `WorkDelivery` for the
target Member. WorkDelivery reuses durable mailbox delivery machinery but is
not an authored Message and does not own Work content.

Historical dogfood evidence is retained in governed research or verified native
exports. Active stores are reset or explicitly migrated; product code does not
maintain two ownership projections.

`WorkEvent` is authoritative append-only transition history. `Work` is a
rebuildable latest projection. A command atomically compares the expected Work
version, appends one idempotent event, updates the projection, and enqueues
deterministically identified WorkDelivery outbox rows. Runtime/member authority
is derived from its trusted binding; client-provided actor strings are never
sufficient authorization.

### Messages are authored conversation

TeamMessage carries authored Markdown conversation with optional `work_id`,
minimal `response_intent`, correlation, and reply lineage. It never changes
Work owner or status by itself.
Assignment, claim, start, block, submit, request changes, accept, release, and
cancel are Work operations and WorkEvents.

PendingInteraction remains the paused-provider interaction object. WorkDelivery
is the reliable transport for a Work transition that a Member must consume.

### Status and readiness remain small

Work statuses are `open`, `in_progress`, `blocked`, `review`, `done`, and
`cancelled`. Assigned/unassigned and ready/not-ready are derived dimensions.
Dependencies only compute readiness and do not form a general Task Graph.

### Creation authority follows Team structure

The Team Host may manage every Work in its Team. Ordinary Members may create
self-owned Work, unassigned Work, and child Work beneath Work they own. They
cannot force assignment to a same-level peer. A Member allowed to create a
child Team becomes its Host and may assign child Works there.

### Mission remains optional and distinct

Mission owns durable outcome and shared context. Wave owns versioned Host
judgment and material re-plan. Works own current execution demand, ownership,
and state. Standalone Teams need no Mission; multi-team or long-horizon outcomes
may use Mission and Wave.

Works remove task enumeration from Wave and replace Assignment-message
ownership. They do not replace Mission closeout or Wave decision history.

## Consequences

- Agent Team gains a queryable shared work pool and atomic self-claim.
- Host no longer serializes every ordinary task through chat.
- Members can discover follow-up work without silently assigning peers.
- Work status, conversational delivery, Provider execution, and acceptance are
  separate and observable.
- Dashboard adds Works as a primary Team surface and task state no longer has
  to be inferred from Activity.
- Organization may select nested Teams as an execution mechanism while keeping
  StandingAgent, AgentMember, TeamRun, and authority identities distinct.
- Company WorkItem can link execution Works without inheriting their lifecycle.
- Existing Assignment-message schemas, CLI writes, projections, warnings,
  fixtures, Skills, Plugin copies, and dogfood stores require a breaking cleanup.

## Rejected Alternatives

### Keep Assignment Message as compatibility

Rejected. It preserves two ownership paths and forces every consumer to decide
whether Message or Work is current.

### Make every message a task

Rejected. Questions, clarification, coordination, and discussion are not
durable work commitments.

### Allow only Host-created Work

Rejected. It keeps discovery and task creation serialized through the Host.

### Let every Member assign peers

Rejected. It permits silent responsibility transfer and makes peer ownership
unstable. Members create eligible unassigned Work or delegate through a child
Team they Host.

### Replace Mission with the board

Rejected. A board explains current execution, not durable intent, multi-team
context, material re-plan judgment, or final closeout.

### Add a general dependency graph

Rejected. Minimal blockers determine readiness; Dynamic Workflow owns complex
deterministic flow.

## Implementation Boundary

### Breaking cutover contract

Until all gates below land together, `TeamMessage(kind=assignment)` remains the
implemented legacy ownership truth and this ADR is target-only. The cutover is
atomic across root operating instructions, schemas, store projections,
CLI/API/MCP, Supervisor/providers, Dashboard, Company OS joins, Skills, Plugin,
fixtures, and active data. The new binary refuses active Execution Spaces that
contain legacy Agent Team Assignment messages; dogfood uses a fresh space after
a manifested historical export. No merged release may expose two ownership
authorities.

At cutover, Mission/Wave remain the only native durable intent and Host
plan/judgment objects. Work is executor-specific responsibility state, not a
third planning hierarchy or a universal Task Graph.

The decision becomes operational only after:

1. Work/WorkEvent/WorkDelivery schemas and store projections exist;
2. atomic assign and claim invariants pass concurrent tests;
3. CLI, HTTP, MCP, Supervisor, Dashboard, Skills, and Plugin share one
   application service;
4. Assignment-message code and active data are removed;
5. busy, idle, crash, Reopen, Close, and Retire behavior pass;
6. Team Workbench proves assigned, unassigned, ready, blocked, review, child,
   and Message-linked states; and
7. mixed-provider dogfood proves standalone and Mission-scoped Teams.
