# ADR 0050: Agent Team Works And Message Boundary

> Work-graph amendment (ADR 0058): the historical rejection of a general
> dependency graph, “minimal blockers” limitation, child-Work creation rule,
> and child-state acceptance item below are superseded. Current Work is flat;
> multiple hard `depends_on` edges form one cycle-safe DAG. The remaining
> Work/Message, responsibility, delivery, and Host-acceptance boundaries stand.

> Successor (DOC-16 row, DEV-40 flip 2026-08-18): [DOC-106](https://app.notion.com/p/3be49a4fa3798126a598e634ed5d0807).

```text
status: accepted; flat-Team amendment implemented by Wave 3
owner_role: architecture
canonical_for: Work as the Agent Team scheduling primitive, no Assignment
  Message ownership, shared Kanban, claim authority, and the historical
  Mission boundary (Mission itself is retired by DOC-108)
```

ADR 0056 supersedes this ADR's PendingInteraction boundary. A provider question
and its answer are correlated Messages; permissions are frozen on AgentSession
start and never become a second lifecycle.

## Context

Agent Team currently proves ownership with
`TeamMessage(kind=assignment) + correlation_id`. A long-running self-hosting
Team grew to 23 MemberRuns and 1,103 messages while retaining no queryable
shared list of assigned, unassigned, ready, blocked, review, and completed work.
The Host repeatedly reconstructed task state from conversation history and idle
Members could not atomically claim known ready work.

The failure reconstruction and Claude Code comparison are preserved in
Agent Team Shared Task List research.

The product also needs one simple execution-board object that the global
operating surface can use. This ADR originally kept the legacy Company WorkItem
as a permanently separate lifecycle. The Unified Work cutover removed that
duplicate responsibility kernel; the former Approval, Finance, Document, and
Mission relations it preserved are themselves retired (DOC-108), leaving
provider-native truth as the remaining distinct relation. ADR 0052's recursive Team
proposal is superseded historical evidence and has no active authority here.

## Decision

### Work is the base responsibility object

Agent Team adds a `Work` object and a shared `Works` projection. Work is the
authoritative Team responsibility object; the Global Work aggregate is
read-only and owns no second task identity (it replaced the former Company Work
aggregate).
The Approval, Finance, and Docs relations named here are retired (DOC-108);
Work review and acceptance carry what the generic Approval used to carry.
Kanban is a view over Work, not another source of truth.

### Assignment is a Work operation

New ownership is created only through Work assignment or atomic self-claim.
`TeamMessage(kind=assignment)` is removed rather than retained as a new-write or
read-compatibility ownership path.

Assignment appends a `WorkAssigned` event and creates a `WorkDelivery` for the
target Member. WorkDelivery reuses durable mailbox delivery machinery but is
not an authored Message and does not own Work content.

Atomic Member self-claim is different: it is a pull performed from a trusted,
bound MemberRun/provider turn. Its `WorkClaimed` event and exact command result
are the responsibility/runtime-possession proof, so it does not create a
loopback WorkDelivery. On crash, the same active Work and provider-native
session are resumed without inventing a provider receipt. Host-originated
assignment, resume, request-changes, and rebind continue to create deliveries.

Historical dogfood evidence is retained in governed research or verified native
exports. Active stores are reset or explicitly migrated; product code does not
maintain two ownership projections.

`WorkEvent` is the append-only semantic transition record, but a bare event is
not the physical replay unit because its payload may intentionally be empty.
One command atomically compares the expected Work version and appends one
`WorkOperation` containing the event, the complete resulting Work projection,
and deterministic WorkDelivery creates/updates. Store read models rebuild Work
and delivery state from ordered WorkOperations. Runtime/member authority is
derived from its trusted binding; client-provided actor strings are never
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

### TeamRun completion is gated by terminal Works

A Host may complete a TeamRun only when every current Work in that run is
`done` or `cancelled`. Submitted `review` Work is not complete until the Host
accepts it. The Store checks this predicate and persists TeamRun completion in
one atomic boundary so a concurrent Work mutation cannot race between the
check and completion write. TeamRun completion remains independent of Member
Close/Retire, Wave advance, and Mission closeout.

Work submission always requires a non-empty result summary. Artifact and check
references are required only when the Work's completion criteria or Host review
requires them; their arrays may otherwise be empty. This keeps acceptance
evidence proportional to the Work instead of inventing a universal attachment
gate.

### Creation authority follows Team structure

The Team Host may manage every Work in its Team. Ordinary Members may create
self-owned Work, unassigned Work, and child Work beneath Work they own. They
cannot force assignment to a same-level peer. Cross-Team responsibility is an
explicit `WorkDelegation` from source Work to target Work in another flat Team;
it never creates parent/child Team authority.

### Team placement, and the historical Mission relation

> Historical note (DOC-108): at decision time this ADR bound each Team to
> exactly one Mission in both directions. That relation is retired — Teams are
> durable and are created without any Mission, keeping at most an optional
> `legacy_mission_id` migration marker — and Mission/Wave write authority fails
> closed. The placement rule below remains current.

The Organization contains flat AgentTeams, and every Team has an
immutable `node_id` placement on one machine. Works own current execution demand,
ownership, and state. Cross-Team or cross-machine cooperation uses
WorkDelegation without merging Team identity.

Works remove task enumeration from the historical Wave concept and replace
Assignment-message ownership.

## Consequences

- Agent Team gains a queryable shared work pool and atomic self-claim.
- Host no longer serializes every ordinary task through chat.
- Members can discover follow-up work without silently assigning peers.
- Work status, conversational delivery, Provider execution, and acceptance are
  separate and observable.
- Dashboard adds Works as a primary Team surface and task state no longer has
  to be inferred from Activity.
- The organization projects multiple flat AgentTeams; the legacy StandingAgent
  join named here is retired compatibility truth only.
- The legacy Company WorkItem could link execution Works during transition, but
  the target does not retain two responsibility lifecycles.
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
unstable. Members create eligible unassigned Work; cross-Team transfer requires
an explicit WorkDelegation owned by the relevant Hosts.

### Replace Mission with the board

Rejected. A board explains current execution, not durable intent, multi-team
context, material re-plan judgment, or final closeout.

### Add a general dependency graph

Rejected. Minimal blockers determine readiness; the then-current Dynamic Workflow owned complex
deterministic flow.

## Implementation Boundary

### Breaking cutover contract

The cutover is atomic at the release boundary across root operating
instructions, schemas, store projections, CLI/API/MCP, Supervisor/providers,
Dashboard, the retired Company OS joins, Skills, Plugin, fixtures, and data. New code
does not read or write Agent Team Assignment Messages as responsibility.
Execution Spaces containing legacy Assignment-message rows are refused and
must be archived/reset or passed through a future explicit offline converter;
there is no dual-read, dual-write, or silent inference path. Dogfood starts in
a fresh space after a manifested historical export. No merged release may
expose two ownership authorities.

At cutover time, Mission/Wave were the only native durable intent and Host
plan/judgment objects (both since retired by DOC-108). Work is
executor-specific responsibility state, not a third planning hierarchy or a
universal Task Graph.

The decision becomes operational only after:

1. Work/WorkEvent/WorkDelivery schemas and store projections exist;
2. atomic assign and claim invariants pass concurrent tests;
3. CLI, HTTP, MCP, Supervisor, Dashboard, Skills, and Plugin share one
   application service;
4. Assignment-message code and active data are removed;
5. busy, idle, crash, Reopen, Close, and Retire behavior pass;
6. Team Workbench proves assigned, unassigned, ready, blocked, review, child,
   and Message-linked states; and
7. TeamRun completion atomically rejects every non-terminal Work state; and
8. mixed-provider dogfood proves Mission-scoped flat Teams and explicit
   cross-Team WorkDelegation.
