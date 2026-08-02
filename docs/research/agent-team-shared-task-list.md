# Agent Team Shared Task List: Failure Reconstruction and Design Research

```text
status: active research
owner_role: execution-foundation
authority_class: research
decision_target: Agent Team shared task-list product contract and ADR
implementation_state: design_only
evidence_snapshot: 2026-08-02
```

> This is research input, not an implemented contract. Current authority remains
> the Agent Team ADRs, schemas, store, CLI, and provider-native runtime records;
> proposed names and transitions require an ADR.

## Executive finding

Star Harness Agent Team has a durable mailbox but no durable shared task list.
That omission forces one `TeamMessage(kind=assignment)` plus its
`correlation_id` to act simultaneously as:

- a work request;
- ownership evidence;
- the current task description;
- an implicit status record;
- a dependency marker;
- and the conversation thread about the work.

Messages are good at communication and poor at representing mutable shared
state. In the real self-hosting run studied here, the result was repeated state
reconstruction, duplicate-looking assignments, Host bottlenecks, idle Standing
Agents, and no cheap answer to four basic questions:

1. What work exists?
2. Which work is assigned, unassigned, blocked, or ready for review?
3. Who owns each item now?
4. What should a newly idle Member claim next?

The recommended direction is a small TeamRun-scoped shared task list. The UI may
present it as a WorkItem-like Kanban, but its execution object should remain
distinct from the company-level `WorkItem`. This report calls the candidate
object `TeamTask`.

```text
Company WorkItem = durable company commitment and business provenance
TeamTask         = shared execution responsibility inside one AgentTeamRun
TeamMessage      = communication about a task or another coordination matter
Mission / Wave   = Host intent, current plan, judgment, and re-plan history
Native Session   = transcript, tools, commands, file activity, and turn truth
```

This separation fixes task visibility without reintroducing `Goal`,
`GoalPhase`, a Plan Gate, or a universal Task Graph.

## Research questions

This study asks:

- Does the present message-only Assignment model cause material coordination
  waste in a real, long-running Agent Team?
- Which behavior in Claude Code Agent Teams is useful to copy, and which parts
  should remain provider-specific?
- How should a Host assign work while allowing Members to claim unassigned work
  safely?
- How should an idle, working, crashed, or resumed Member discover its work?
- How should a multi-level Organization delegate work without collapsing
  Company `WorkItem`, Team execution, and ordinary messages into one object?
- What is the smallest useful readiness/dependency model that does not become a
  general workflow engine?

## Current contract and missing object

The current Star Harness relation is intentionally simple:

```text
Mission <-> independent AgentTeam
Mission -> ordered Host-plan Wave
AgentTeamRun -> MemberRun -> provider-native session
AgentTeamRun -> TeamMessage
```

An Assignment is currently proven by
`TeamMessage(kind=assignment) + correlation_id`. That proves that a message
assigned work. It does not provide a first-class current task projection.

The system therefore has:

- a durable Team identity and TeamRun;
- persistent MemberRuns and provider-native Sessions;
- ordinary Host/Member/Peer mail;
- delivery, ACK, Handoff, control, and runtime lifecycle records; and
- company-level WorkItems outside Agent Team.

It lacks a TeamRun-local record for:

- task title and completion criteria;
- current owner or unassigned state;
- pending, active, blocked, review, and completed state;
- minimal readiness dependencies;
- claim/assignment history;
- result, check, and artifact references; and
- roll-up to an optional source Company WorkItem.

That missing record is the architectural gap. The answer is not to make
messages more elaborate. It is to stop asking messages to be task storage.

## Failure reconstruction

The full evidence and causal timeline are preserved in
[Agent Team shared task-list failure reconstruction](agent-team-shared-task-list-evidence.md).
The key observation is that a focused two-Member audit expanded into a
23-MemberRun, 1,103-message operating system without gaining a queryable view of
work. Adding Members increased mail, while the Host remained the private task
database and scheduler.

## Claude Code Agent Teams comparison

Claude Code Agent Teams separates the Team's shared task list from its
mailbox. Official documentation describes four components: Team Lead,
Teammates, a shared Task List, and a Mailbox. Tasks use `pending`,
`in progress`, and `completed` states. The Lead may assign work explicitly, or a
Teammate may claim the next unassigned and unblocked task. Claims use file
locking to prevent races. Tasks may depend on other tasks; completing a
prerequisite unblocks its dependents. See
[Claude Code Agent Teams](https://code.claude.com/docs/en/agent-teams).

This yields two distinct collaboration paths:

```text
Task list: what work exists, readiness, ownership, and status
Mailbox:   questions, context, coordination, review, and direct discussion
```

The distinction is more important than Claude's exact storage format. Claude
stores Team configuration and task state locally under its own user data
directories; Star Harness needs provider-neutral, durable coordination records
that work across Codex, Claude, Kimi, Dashboard, CLI, and Organization.

Claude also exposes task-oriented hooks such as `TaskCreated`, `TaskCompleted`,
and `TeammateIdle`, which can validate or react to transitions. See
[Claude Code hooks](https://code.claude.com/docs/en/hooks). These hooks suggest
useful acceptance points, but Star Harness should enforce its own store
invariants rather than depend on a provider hook.

The reference has limits. Claude Agent Teams is experimental; its docs note
task status lag, one team per session, no nested teams, and restrictions around
resuming in-process teammates. Star Harness should copy the clean separation of
task state and communication, not inherit those lifecycle boundaries.

Subagents are also different from Teammates: a subagent reports back to its
caller, while a teammate is an independently addressable collaborator with its
own context and direct communication. See
[Claude Code subagents](https://code.claude.com/docs/en/agents). This supports
Star Harness's existing rule that provider-native subagents remain an internal
implementation detail of the Member that invoked them.

## Proposed minimal model

The recommended candidate is `TeamTask`, scoped to one `AgentTeamRun`.

```text
TeamTask
  id
  team_run_id
  title
  context_markdown
  completion_criteria_markdown
  status
  owner_member_run_id?
  created_by_actor
  source_work_item_ref?
  parent_team_task_id?
  blocked_by_task_ids[]
  assignment_correlation_id?
  result_summary?
  artifact_refs[]
  check_refs[]
  created_at / updated_at
```

The name is intentionally not `WorkItem`. Company WorkItems remain company
commitments with business provenance, approval, milestone, result routing, and
cross-system relations. A TeamTask is an execution responsibility within one
run. One WorkItem may produce several TeamTasks, and one TeamTask may exist for
Mission-only work with no Company Store at all.

### Status model

Research recommends this small state set:

```text
pending -> in_progress -> review -> completed
   |            |
   +----------> blocked
   +----------> cancelled
```

- `pending` plus no owner means unassigned.
- `pending` plus an owner means assigned but not started.
- `in_progress` means one MemberRun owns active execution.
- `blocked` requires a short blocker reason; it does not free ownership unless
  the Host or owner explicitly releases it.
- `review` means the Member submitted work and still owns correction duty.
- `completed` requires an accepted result, not merely Provider turn completion.
- `cancelled` preserves history without presenting the task as remaining work.

`review` is useful because Member completion and Host acceptance are already
separate product facts. It avoids pretending a Handoff is automatically done.

### Assigned and unassigned views

The minimum useful board begins with two questions, not a complex workflow:

| View | Query |
| --- | --- |
| Unassigned ready work | `status=pending`, `owner=null`, all blockers complete |
| Assigned work | owner is set, grouped by Member and status |

The richer Kanban may render `Pending`, `In progress`, `Blocked`, `Review`, and
`Completed`, but “unassigned vs assigned” remains the primary scheduling lens.

### Host assignment

The Host creates or selects a TeamTask and atomically assigns it to a MemberRun.
The assignment operation:

1. verifies the task and Member belong to the same TeamRun;
2. verifies the task is not completed/cancelled and has no other owner;
3. records the owner transition;
4. appends an Assignment TeamMessage linked to the task for notification and
   initial conversation; and
5. wakes an idle Member through the current Supervisor delivery path.

The TeamTask is ownership truth. The Assignment message is the durable briefing
and notification, not a second task database.

### Member self-claim

An idle Member may query unassigned ready work and atomically claim one task.
The claim must be compare-and-append or otherwise transactionally exclusive so
two Members cannot own the same task.

Selection policy may be simple in v1:

- explicit Host priority first;
- capability/owned-path compatibility;
- oldest ready task;
- stable id as the final tie-breaker.

The Member does not need to ask the Host “is there anything else?” when the
board already contains authorized ready work. The Host still controls what work
is created, its priority and constraints, and may disable self-claim for a task
or Member.

### Readiness, not a Task Graph

`blocked_by_task_ids` provides only one useful fact: whether a task is ready to
claim. It does not define arbitrary branches, loops, retries, conditional
expressions, Wave barriers, or executor ownership.

```text
Task A completed
  -> Task B has no remaining blockers
  -> Task B becomes visible in Unassigned ready work
  -> Host assigns it or an eligible Member claims it
```

Dynamic Workflow continues to own deterministic workflow steps. Mission/Wave
continues to own Host planning and re-plan. The shared task list is deliberately
not another workflow language.

### Task-linked messages

`TeamMessage` should gain an optional `team_task_id` relation. Correlation keeps
its existing meaning: one conversation or causal chain. A task may have several
conversation chains across planning, blocker resolution, review, and correction.

Examples:

```text
TeamTask T-17
  ├─ Assignment correlation C-1
  ├─ Member -> Host question correlation C-2
  ├─ Peer coordination correlation C-3
  └─ Handoff/review correlation C-4
```

This prevents a correlation id from being overloaded as the permanent task id.

## Member acquisition and execution loop

Every persistent Member cycle should receive a small coordination context pack:

1. its active assigned TeamTasks;
2. ready unassigned TeamTasks it is allowed to claim;
3. recently changed tasks relevant to it;
4. unread task-linked TeamMessages; and
5. its existing provider-native Session and Workspace binding.

Behavior differs by runtime state:

| Member state | Task behavior |
| --- | --- |
| Working | Current owner remains stable; new ordinary assignments queue for the next safe boundary unless Host explicitly Steers a supported turn |
| Idle | Supervisor delivers assigned ready work; otherwise Member may claim from the ready pool |
| Waiting interaction | Matching PendingInteraction is resolved first; task ownership remains unchanged |
| Disconnected/crashed | Tasks remain assigned; no second Member silently claims them |
| Resumed | Same MemberRun and native Session reconcile active task, inbox, and last accepted transition before continuing |
| Closed | Host must reassign or cancel unfinished tasks explicitly |

Crash recovery must not infer task completion from a Provider process exit. It
must compare TeamTask state, message delivery, Provider-native Session receipts,
and explicit Handoff/review facts.

## Host loop with many tasks

The Host should manage exceptions and judgment, not hand-serialize all work.

```text
Host decomposes current Wave into TeamTasks
  -> assigns constrained/high-risk tasks
  -> leaves safe tasks in the ready pool
  -> Members execute or atomically self-claim
  -> board updates provide global state
  -> questions/blockers use TeamMessage
  -> submissions enter Review
  -> Host accepts, requests correction, reassigns, or re-plans the Wave
```

The board lets the Host answer “what next?” with a query rather than transcript
reconstruction. It also makes under-utilization visible: many idle Members plus
ready unassigned work is a scheduling failure, not an invisible feeling.

## Multi-level Organization delegation

Organization should consume this Agent Team capability rather than invent a
second scheduler.

Example:

```text
Company WorkItem: Implement GitHub source binding
  -> parent AgentTeamRun TeamTask owned by CTO Agent
      -> CTO decides delegation is useful
      -> creates or reuses a child AgentTeamRun
          -> Backend TeamTask owned by Core Member
          -> Dashboard TeamTask owned by UI Member
          -> Acceptance TeamTask owned by Reviewer Member
      -> child tasks complete and return evidence
      -> CTO integrates and submits the parent TeamTask for review
  -> Lead accepts result and updates the Company WorkItem
```

Important boundaries:

- the Company WorkItem remains the business commitment;
- the parent TeamTask remains the CTO's delegated responsibility;
- child TeamTasks expose the CTO's internal execution board;
- child completion does not automatically accept the parent task;
- the CTO may create temporary Members or a child Team when its authority
  allows it;
- reporting lines and authority remain Organization truth, not TeamRun nesting;
  and
- ordinary questions and peer coordination remain messages, not new tasks.

This is how multiple Organization levels can delegate without every decision
returning to the Human or root Supervisor. Authority can be pushed downward,
while shared task state keeps delegation observable.

## Kanban and Member UX

The Team War Room should place the task board beside, not inside, Team Activity.

### Team board

- primary switch: `Unassigned` / `Assigned` / `All`;
- Kanban status: Pending, In progress, Blocked, Review, Completed;
- filters: Member, task source, status, priority, owned path, and updated time;
- each card: title, owner/avatar, completion criteria preview, blocker count,
  source WorkItem, last activity, and unread thread count;
- actions: assign, claim, release, block, submit for review, accept, request
  changes, and open task thread; and
- an explicit empty state that distinguishes “no work exists” from “all work is
  blocked” and “all ready work is assigned”.

### Member page

The existing derived Current Assignment becomes “My tasks”:

- active task and acceptance criteria;
- queued assigned tasks;
- eligible ready tasks;
- task-linked Host/Peer discussion;
- Workspace, native Session, tools, artifacts, and checks; and
- claim/release controls when policy permits.

The provider-native transcript remains the execution history. The board does
not copy tools, thinking, commands, or chat into Harness.

### Team Activity

Team Activity remains the group conversation and event history. It should link
messages to task cards and allow filters by Member and message type. It must not
be the only place where an operator discovers work status.

## Candidate interfaces

Names remain research proposals:

```bash
harness team-task create --team-run-id <run> --title <title> \
  --context <markdown> --completion-criteria <markdown>
harness team-task list --team-run-id <run> --ready --unassigned --json
harness team-task assign --id <task> --member-run-id <member>
harness team-task claim --id <task> --member-run-id <member>
harness team-task update --id <task> --status blocked --reason <markdown>
harness team-task submit --id <task> --result <markdown> --artifact <ref>
harness team-task review --id <task> --accept --summary <markdown>
harness team-task release --id <task> --reason <markdown>
```

CLI, HTTP, MCP, Dashboard, and Plugin should reuse one application service and
the same transition checks. Assignment-message compatibility can remain during
migration, but new task ownership must not diverge between surfaces.

## Explicit non-goals

The first version should not add:

- `Goal` or `GoalPhase`;
- Provider-independent Plan Mode or Plan Gate;
- a universal Task Graph;
- arbitrary conditional execution, branching, loops, or retries;
- Wave ownership of Team runtime;
- automatic Host acceptance based on Provider completion;
- duplicate provider transcript, tool stream, or thinking storage;
- a requirement that every TeamTask link a Company WorkItem; or
- a requirement that every message link a TeamTask.

These exclusions preserve the product's simple mental model.

## Decision and implementation path

If the direction is accepted, the next work should follow the repository's
normal progression:

1. **Decision:** ADR freezes `TeamTask`, its boundary from Company WorkItem and
   Dynamic Workflow, state transitions, atomic claim semantics, and migration.
2. **Schema/store:** append-only TeamTask records, latest-wins projection,
   readiness query, ownership invariant, and result/evidence references.
3. **CLI/API/MCP:** create, list, assign, claim, update, submit, review, release,
   and task-linked messaging through one application service.
4. **Provider context:** inject active/ready task summaries at safe boundaries;
   never start a second top-level execution driver.
5. **Dashboard:** Team Kanban, Member My Tasks, Host readiness/under-utilization
   signals, and task-linked Team Activity.
6. **Org integration:** Company Assignment links a WorkItem to one accountable
   TeamTask; delegated child teams preserve parent responsibility.
7. **Dogfood:** repeat the GitHub source-binding scenario and compare assignment
   count, repeated state-inspection prompts, idle-with-ready-work duration,
   duplicate warnings, and Host coordination load against this baseline.

## Acceptance questions for the future ADR

Before implementation, the decision must settle the public object name; one-owner
semantics; actor permissions; whether Review is a state or decision; priority
and capability fields for self-claim; carry-over across Waves; crash, Close and
Session reconciliation; wake boundaries; parent/child roll-up without automatic
acceptance; and how Assignment-only history is read without inventing tasks.

## Publication takeaway

The lesson is not “add a Kanban because Kanbans are convenient.” Collaboration
needs messages and shared mutable work state. Without both, each participant
receives fragments while the Host becomes the only place where plan, ownership,
readiness, and completion coexist. More autonomous Agents then increase mail
faster than throughput.

A small shared task list changes the shape of the system:

```text
messages explain and negotiate work
tasks expose and transfer work
Mission/Wave records Host judgment
native Sessions prove execution
Company WorkItems preserve business accountability
```

That separation is what lets Agent Team become a reliable execution substrate
for a multi-level Organization instead of a larger group chat.

## Sources

- [Claude Code Agent Teams](https://code.claude.com/docs/en/agent-teams)
- [Claude Code subagents](https://code.claude.com/docs/en/agents)
- [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- Native TeamRun `team-run-1785417589241-p28630-0` and linked Sessions
- [Agent Team foundation closure plan](../product/agent-team-foundation-closure-plan.md)
