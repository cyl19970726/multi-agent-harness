# Agent Control Plane

This document defines the provider-neutral control plane for durable
`AgentMember` teams. It sits between the product architecture in
[architecture.md](architecture.md) and provider-specific implementations such
as [integration/codex.md](integration/codex.md).

The design reference is Claude Code Agent Teams: coordinated sessions with a
shared task list, direct teammate messaging, separate contexts, and explicit
team lifecycle. Multi-Agent Harness uses the same product lesson, but records
the coordination state in harness objects so it can be reviewed, replayed, and
shown in the Agent Dashboard.

Reference model:

- Claude Code agent teams use independent teammates, a shared task list, and
  direct teammate messaging. That is the right product shape for complex work
  where agents need to challenge, ask, and coordinate with each other.
- Claude Code subagents are different: they have separate context but normally
  report back to the caller. They are useful for focused work, but they do not
  replace durable harness teammates.
- Claude/agent hooks show the required event surface: prompt submit, tool
  calls, stop, subagent stop, notification, and session lifecycle. Harness must
  reduce these events into stable objects instead of making the Dashboard parse
  provider transcripts.

## Core Position

An `AgentMember` is the durable identity of a teammate. A provider session,
Codex thread, native subagent, shell process, or temporary chat helper is only
runtime execution behind that identity.

```text
AgentMember
  -> AgentRuntime
  -> ProviderSession / provider thread / provider child thread
  -> AgentEvent
  -> Message / Proposal / Evidence / Decision
```

The control plane owns:

- member identity, role, prompt, skills, team membership, and permissions;
- lifecycle: create, start, health, busy, idle, handoff, close;
- message-first communication and delivery state;
- task queue and peer-to-peer teammate messages;
- team-generated goal proposals, task-graph changes, blockers, and follow-up
  work;
- provider event reduction into harness objects;
- Dashboard views for operating the team.

The provider owns model execution. It does not own task acceptance, Leader
decisions, evidence policy, or the canonical message ledger.

## AgentMember Identity

`AgentMember.id` is stable across provider restarts. It is the actor id used by
tasks, messages, proposals, evidence, provider sessions, and Dashboard state.

Minimum identity fields:

```text
AgentMember
  id
  name
  description
  role
  provider
  model/profile?
  provider_config
  capabilities
  team_ids
  prompt_ref
  skill_refs
  workspace_policy?
  worktree_ref?
  permission_profile?
  runtime_workspace_roots
  status
  current_task_id?
  current_proposal_id?
  provider_runtime_id?
  provider_thread_id?
  control_endpoint?
```

Provider-native subagents are not `AgentMember` records by default. They should
be observed as `ProviderChildThread` or provider-child events until the Leader
intentionally promotes them to durable teammates. Schema promotion is justified
when child work needs independent assignment, review, evidence ownership, or
Dashboard accountability beyond the parent member's task.

## Lifecycle

Create, start, and close are separate operations.

```text
create
  -> append AgentMember(status=created)
  -> write prompt_ref / role / skills / permission profile

start
  -> spawn or attach AgentRuntime
  -> record process/socket/protocol health
  -> set AgentMember(status=idle)

close
  -> require handoff or force if a task is active
  -> interrupt/archive provider runtime
  -> stop latest non-stopped runtimes
  -> set AgentMember(status=closed)
```

Create records identity even when no provider process is running. Start makes
that identity executable. A member should return to idle after a turn and wait
for the next message. Close is not task completion; it is retirement, handoff,
or forced cleanup. Close preserves messages, events, provider sessions,
proposals, evidence, and decisions.

Health has layers:

- process: provider runtime pid exists and has not exited;
- endpoint: socket or remote control endpoint accepts connections;
- protocol: provider initialize/probe succeeds;
- delivery: at least one message can reach a terminal delivered or failed
  state with a provider-session record.

The Dashboard must not present process health as execution readiness when
protocol or delivery health is unknown.

## Persistent Autonomous Team Loop

An `AgentTeam` is a standing organization around a project or scenario, not a
temporary collection of job runners. The team can generate work as well as
execute assigned work.

Normal autonomous loop:

```text
team observes project state / dashboard warnings / prior GoalCases
  -> member proposes goal, blocker, follow-up task, or graph change
  -> Lead accepts, rejects, prioritizes, or asks for more evidence
  -> accepted goal gets GoalDesign and task graph
  -> tasks are assigned through messages
  -> members execute, ask peers, report evidence, and request review
  -> decision and GoalEvaluation produce the next proposals
```

The Lead owns final interpretation and priority, but the Lead is not the only
source of work. A Critic can propose a quality goal, a Dashboard member can
propose a visibility task, a Provider Runtime member can propose a reliability
task, and a domain member can propose a new adapter or strategy investigation.

The `Observer` role is the default way to make this continuous instead of
accidental. Observer is a durable `AgentMember` whose primary job is to watch
state and propose work:

| Input | Observer output |
| --- | --- |
| Dashboard warnings | blocker, repair task, or graph-change proposal |
| CI/check failures | infrastructure task or acceptance-gate proposal |
| stale tasks/messages/sessions | unblock task, reassignment proposal, or escalation |
| prior GoalCases | reusable workflow suggestion or anti-pattern warning |
| adapter/domain evidence | new investigation goal or promotion/blocking question |

Observer does not decide final priority. It gives the team a steady stream of
structured proposals so progress does not depend on the user noticing every
gap.

Team-generated work must be durable:

- proposed goals are messages or evidence before they become accepted `Goal`
  objects;
- task-graph changes are messages, decisions, or task records, not hidden
  provider chat;
- blockers name the evidence or missing infrastructure that prevents progress;
- rejected proposals remain visible so future Leads know why the path was not
  taken;
- accepted proposals enter the same GoalDesign, assignment, review, decision,
  and evaluation flow as user-requested work.

This is the difference between Multi-Agent Harness and one-shot subagents. A
subagent answers a prompt. A harness team maintains a backlog, a shared task
graph, and an evidence-backed learning loop.

Current first-version implementation keeps this minimal:

```text
autonomy observe
  -> Evidence(source_type=goal_proposal|graph_change_proposal|blocker|follow_up)
  -> Message(channel=observer-proposal, to=Lead)

autonomy plan-next
  -> Evidence(source_type=next_round_plan)
  -> Evidence(source_type=goal_proposal)
  -> Message(channel=next-round-proposal, to=Lead)

autonomy decide
  -> Decision(accept|reject|defer|request_evidence)
  -> optional Goal
  -> optional Task(parent_task_id=<source task>)
  -> optional Message(kind=task) assignment

autonomy tick / loop
  -> Provider Gateway delivery
  -> GoalClose gate for active goals whose task graph is done and evaluated
  -> Goal(status=complete)
  -> Decision(goal_complete)
  -> next_round_plan comparing GoalEvaluation with vision_ref
  -> goal_proposal Message to Lead
  -> optional Lead auto-accept
  -> new GoalDesign and minimal TaskGraph
  -> task assignment Message
  -> Provider Gateway delivery
```

This deliberately avoids a premature `GoalProposal` schema. The source of
truth remains the existing `Evidence`, `Message`, `Decision`, `Goal`, and
`Task` objects until repeated gates need stable proposal fields.

The runner is lifecycle-gated. It should not create a next goal merely because
a task produced a report. A goal is eligible only when its task graph is
complete, strict goal learning has no warnings, GoalEvaluation/final acceptance
exists before GoalClose, a vision context is supplied, and no prior next-round
plan exists unless the operator explicitly passes `--force`.

## Message-First Communication

All agent work starts from a harness `Message`, usually tied to a `Task`.
Provider chat is transport, not the source of truth.

Normal assignment:

```text
Leader
  -> Message(kind=task, to_agent_id=worker, task_id=T)
  -> queued delivery
  -> AgentRuntime turn/input
  -> AgentEvent stream
  -> Message(kind=report, from_agent_id=worker, evidence_ids=[...])
  -> Proposal / Evidence
  -> Critic/Gate
  -> Decision
```

Peer-to-peer communication uses the same ledger:

```text
Worker A
  -> Message(kind=message, to_agent_id=Worker B, task_id=T?)
  -> delivered when B is idle or policy allows active-turn injection
  -> optional reply Message
```

Direct teammate messaging is useful for clarification, handoff, and review
feedback. It must remain inspectable by the Leader and review gate. Broadcasts
should be explicit channel messages and used sparingly because they increase
coordination cost.

## Busy, Idle, And Queue Semantics

Each member has one current active turn for MVP.

| Member state | Queue behavior |
| --- | --- |
| `idle` | Next queued message can be delivered immediately. |
| `busy` / `running` | New messages remain queued unless policy allows steer/inject. |
| `blocked` | Messages remain queued until the block is resolved or reassigned. |
| `closed` | New delivery fails; existing queued messages need reassignment or waiver. |

Queue rules:

- `Message.delivery_status=queued` means the harness has accepted the message,
  not that the provider has seen it.
- `delivered` requires a provider turn/input acceptance event or equivalent
  provider-session proof.
- `acknowledged` requires the member or reducer to record that the message was
  read, started, or answered.
- `failed` requires a provider-session fixture or explicit reducer error.
- ordered delivery is per recipient member; cross-member ordering is expressed
  through task dependencies and messages.
- queued peer messages should be visible in the Dashboard inbox even before
  delivery.

Steering an active member is a control-plane operation. It should record why
the new message interrupted or amended active work and whether it superseded,
augmented, or waited behind the current message.

Delivery policy should be explicit on the message or inferred from the
channel:

| Policy | Behavior | Use |
| --- | --- | --- |
| `queue` | Wait until the member is idle, then deliver in order. | Default task, report request, ordinary peer message. |
| `inject` | Add context to the active turn if the provider supports it. | Clarification that should influence current work without stopping it. |
| `interrupt` | Stop or steer active work, then deliver the new message. | Safety issue, user correction, task cancellation, or urgent blocker. |
| `broadcast` | Fan out one logical channel message into per-member queued messages. | Team-wide announcement or decision. |
| `manual_ack` | Mark as visible but require a human or Lead decision before delivery. | Permission, money-moving, secret-touching, or destructive actions. |

The MVP can implement only `queue` and explicit failed delivery, but the object
model must not block later `inject` and `interrupt`. If the Dashboard cannot
explain which policy was used, the user cannot tell whether a message was
ignored, queued, injected, or blocked.

## Shared Task List And Team Context

An `AgentTeam` is a set of durable members around a goal or task graph.

The shared task list is the harness `Task` graph, not an in-provider scratchpad.
Each teammate can inspect the tasks relevant to its team and propose changes.
The Leader owns acceptance and priority for graph changes, but the graph should
grow from member reports, critic findings, dashboard warnings, adapter
evidence, and GoalEvaluation output.

Team members have separate contexts:

- each member gets its own role prompt, provider thread, runtime state, and
  optional worktree;
- task assignment messages provide the shared context needed for the task;
- evidence and reports summarize findings back into the shared harness store;
- long logs and provider transcripts stay as evidence refs instead of being
  copied into every teammate's context.

Subagents are different from agent teams. A subagent is a provider-native child
execution under one member. An agent team is multiple durable harness members
that can own tasks, message peers, and be reviewed independently.

## Hook And Provider Event Reducer

Providers emit events in provider-specific shapes. Hooks can add lifecycle
signals. The harness reduces both into stable objects.

```text
provider notification / hook event / rollout reconciliation
  -> ProviderSession
  -> AgentEvent
  -> Message.delivery update
  -> Proposal / Evidence candidate
  -> report Message candidate
```

Reducer rules:

- provider events and hooks are evidence candidates, not Leader decisions;
- hooks must not mark a message delivered if provider delivery failed;
- terminal provider signals can create report candidates, but reviewer or
  Leader gates still decide acceptance;
- every failed delivery should leave a reproducible request/response or log
  fixture where possible;
- native subagent start/stop events should link to the parent `AgentMember`
  and task through `ProviderChildThread` until promoted.

This is where schema promotion should happen carefully. Keep event payloads as
evidence or payload refs until repeated gates need stable fields. Promote fields
to schema when review, Dashboard, or CLI checks depend on them across providers.

## Dashboard Control Plane

The Agent Dashboard is the operator control plane for harness state. It is not
only a roster of members.

Required control-plane views:

- Goals: design status, task graph health, evaluation status, follow-ups.
- Teams: team owner, member roles, runtime health, current tasks, idle/busy
  state, and peer-message counts.
- Task board: shared task list with assignee, reviewer, dependencies, owned
  paths, workspace refs, blockers, proposal state, and decision state.
- Inbox/outbox: queued, delivered, acknowledged, and failed messages by member
  and task.
- Runtime timeline: provider sessions, process/protocol/delivery health,
  event reducer output, hooks, and child threads.
- Evidence and proposals: check evidence, diff evidence, report messages,
  critic findings, and review-gate status.

Dashboard actions should call the same CLI/API paths as agents. Current safe
actions include `message send`, `agent deliver`, safe delivery retry, provider
session reconciliation, review request, and `agent close`. Future actions
include `agent create`, `agent start`, task graph edits, proposal review, and
decision recording. The Dashboard must not become a parallel state machine.

Minimum useful screens:

| Screen | Answers |
| --- | --- |
| Team board | Which goal/team is active, who is lead, who owns each workstream, and which agents are idle, busy, blocked, or closed? |
| Agent detail | What prompt/skills/tools/permissions does this member have, what is its active turn, and which queued messages are waiting? |
| Inbox/outbox | Which messages are unread, queued, delivered, acknowledged, answered, failed, or waiting on permission? |
| Task graph | Which tasks are blocked by dependencies, which are claimed, and which are ready for self-claim or assignment? |
| Runtime timeline | What provider sessions, hooks, tool calls, permission requests, child threads, and terminal events happened? |
| Evidence/decision lane | Which report/proposal/check/critic evidence supports the current decision? |

The Dashboard must be able to show an agent team doing work without asking the
operator to inspect raw JSON. If the answer to "what is this agent doing and
what message is it reacting to?" requires opening provider logs, the control
plane is incomplete.

## Current Gaps

The repository currently has enough surface to prove live persistent
`AgentMember` execution, but the control plane is still immature.

| Gap | Why it matters | Required work |
| --- | --- | --- |
| Delivery terminal detection is weak | A member can modify files while `agent deliver` records failure or timeout. | Reconcile app-server events, hooks, and thread/read into one terminal state. |
| Message state is too small | `queued/delivered/failed` cannot explain read, active, answered, deferred, interrupted, or permission-blocked messages. | Extend message delivery state and add delivery policy. |
| Busy/idle is inferred poorly | Dashboard cannot know whether to deliver, queue, inject, or interrupt. | Add active-turn and reducer-derived member state. |
| Peer communication is not a first-class view | Agents can send messages, but the operator cannot see the collaboration graph. | Add inbox/outbox, reply/correlation refs, and channel fanout. |
| Team autonomy is early-stage | The CLI and Dashboard can represent Observer proposals and next-round planning, but proposal review actions and richer graph editing are still limited. | Harden `autonomy` commands, add Dashboard safe actions for Lead disposition, and promote stable fields only after repeated gates require them. |
| Dashboard safe actions are partial | It can send/deliver/retry/reconcile/request review/close, but cannot yet create full teams or record final decisions. | Add create/start, task graph edits, proposal review, and decision actions through the same API/CLI paths. |
| Provider child work is easy to hide | Native subagents or child threads can disappear under the parent member. | Ingest child-thread events and render them under the parent/task. |
| Goal/task planning is scattered | The repo has phases, but no concise execution roadmap tied to control-plane gaps. | Use the phased plan below as the next implementation graph. |

## Phased Roadmap

| Phase | Goal | Primary tasks | Acceptance |
| --- | --- | --- | --- |
| P0 | Make the contract explicit. | Keep this document, PRD, architecture, MVP, schemas, and skill instructions aligned. | Docs explain member lifecycle, queue policy, peer messaging, reducer, Dashboard, and roadmap without exceeding split rules. |
| P1 | Fix message and member state. | Add delivery policy, active turn, acknowledged/answered/deferred/interrupted states, and correlation/reply refs. | A queued message sent to a busy member stays visible and later delivers or records a policy failure. |
| P2 | Build the reducer. | Reduce app-server notifications, hooks, provider sessions, and thread/read reconciliation into member status, message delivery, child-thread events, and report candidates. | A live member that edits files cannot finish with only a timeout; the store shows terminal success, failure, or explicit unresolved state. |
| P3 | Make Dashboard operational. | Extend the first control-plane slice into create/start, task graph edits, proposal review, and decision actions. | The operator can answer what each agent is doing, what message it is handling, what is queued, what is blocked, and perform the normal safe repair path without raw JSON. |
| P4 | Support true teams. | Add channel fanout, peer replies, self-claim/claim locks, task dependency readiness, and reviewer handoff. | A Worker and Critic can coordinate through messages without routing every exchange through the Lead. |
| P5 | Make teams autonomous. | Add proposed goals, graph-change proposals, blockers, follow-up task creation, and Lead accept/reject/prioritize decisions. | The team can create the next goal or graph change from evidence without waiting for the user to name every task. |
| P6 | Package provider integrations. | Stabilize Codex app-server, managed hooks, optional plugin, and later Claude/hermes adapters behind the same control-plane API. | Provider-specific details are hidden behind the same AgentMember/message/event objects. |
| P7 | Close the learning loop. | Add GoalCase examples and evaluator closeout for each control-plane improvement. | Future Leads can inspect prior goal runs to design better teams and task graphs. |

The next product milestone should be P1 plus P2, not more dashboard decoration.
Without reliable message state and reducer-derived member state, the Dashboard
cannot be truthful.

## Non-Goals

- Do not replace project dashboards with the Agent Dashboard.
- Do not treat provider-native subagents as durable teammates unless promoted.
- Do not use hooks as the message bus.
- Do not accept chat-only reports without harness messages and evidence refs.
- Do not build a workflow DSL before the task/message/evidence loop is stable.
