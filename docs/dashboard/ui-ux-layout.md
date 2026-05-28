# Agent Dashboard UI/UX Layout Plan

This document defines the desired Agent Dashboard layout before frontend
implementation changes. Product-level purpose stays in
[../dashboard.md](../dashboard.md). Core UI/UX principles stay in
[design-principles.md](design-principles.md). Frontend module boundaries stay
in [frontend-architecture.md](frontend-architecture.md). Candidate layouts and
Designer/Questioner critique stay in [layout-variants.md](layout-variants.md).
Accepted layout decisions and killed alternatives stay in
[layout-decisions.md](layout-decisions.md).
Browser and web-quality acceptance stays in [acceptance.md](acceptance.md).

## Doctrine

The Agent Dashboard layout is an operator workbench for proving and repairing
the harness workflow:

```text
Vision -> Goal collection -> GoalDesign
  -> Goal graph / Goal Kanban
  -> Task graph / Task Kanban
  -> Message assignment -> AgentMember runtime
  -> Report/Evidence -> Proposal/Review -> Decision/Evaluation
```

The layout must make that chain visible without requiring raw JSON, provider
transcripts, or hidden chat context. The first screen should answer:

- what goal is active;
- which tasks can move now;
- which AgentMembers are idle, running, blocked, or stale;
- whether assignment and reporting happened through messages;
- which evidence, review, decision, or warning needs operator action.

The Dashboard is not a metric wall. Summary counts can help orientation, but
they must not push the task/member/evidence chain below the fold. The principles
behind Vision, Goal, Task, graph/Kanban, AgentTeam, AgentMember, Git, and visual
system choices are in [design-principles.md](design-principles.md).

## Global Shell

Desktop layout:

```text
+--------------------------------------------------------------------+
| Top bar: active store, live status, gateway health, safe commands   |
+----------+---------------+----------------------------+------------+
| App rail | Team rail     | Team workspace             | Inspector  |
|            |                                          |            |
| Vision     | Team spaces   | active Vision/Goal strip   | Member     |
| Teams      | role groups   | Team activity + work queue | Task       |
| Goals      | member status | Goal/Task document tabs    | Docs       |
| Docs       | role gaps     | graph/Kanban relationship  | Warnings   |
| Warnings   | queues        | decision queue             | Evidence   |
+----------+---------------+----------------------------+------------+
| Collapsed debug drawer: raw JSON, snapshot import/export, sessions  |
+--------------------------------------------------------------------+
```

Layout rules:

- top bar stays compact and operational: live status, generated time, API URL,
  gateway tick, and import/debug access;
- the selected Vision and Goal stay visible above Team activity and work tabs;
- snapshot paste and file import live in a collapsible debug drawer, not the
  default first viewport;
- left rails are navigation and Team context, not detail panels;
- center workspace owns Team activity, current work, Goal/Task document tabs,
  graph/Kanban relationship views, and decision queue;
- right inspector owns the selected Member, Task, Docs, Warnings, or Evidence
  tab instead of stacking every detail at once;
- raw views are hidden behind a debug drawer and never expand the default page
  height;
- all scroll should happen inside the workbench, inspector, or debug drawer;
  the whole page should not become a long mixed-content document.

Recommended desktop columns:

```text
app rail: 56-72px
team rail: 260-300px
workspace: minmax(620px, 1fr)
inspector: 360-420px
page max width: none for work surface, but each panel uses readable line widths
```

The accepted direction is recorded in
[layout-decisions.md](layout-decisions.md): Team workspace shell, Goal/Task
document surfaces, and controlled graph/Kanban relationship layer.

### Team Workspace Layout

Purpose: make the Dashboard feel like a persistent multi-agent collaboration
space while preserving workflow proof.

```text
Team header
  active Vision / selected Goal / goal health / next action

Team roster
  Lead / Observer / Worker / Critic / Specialist
  status, queue, current task, last event, role gap

Team workspace
  activity stream mapped to canonical objects
  current Goal and Task document tabs
  operational queues and decision queue
  graph/Kanban relationship tab
```

Rules:

- Team activity cannot become chat-only; every row must link to `Message`,
  `Task`, `Evidence`, `Proposal`, `Decision`, or warning state;
- AgentTeam is never graph-first; graph is reserved for Vision/Goal/Task
  relationships or diagnostics;
- Member selection opens the Member workbench in the inspector or `/members/:id`.

## Core Layouts

### Vision Context Layout

Purpose: show why the current goal exists and how goal learning creates the
next goal.

```text
Vision board
  vision ref or summary
  final acceptance signals
  active pilot or scenario
  goal collection progress grouped by complete / not complete

Goal ladder
  not-complete goals
  proposed goals
  active goals
  blocked goals
  completed goals
  recently evaluated goals
  next-round proposals

Distance-to-vision
  what changed after the selected goal
  remaining gaps
  missing infra or evidence
  next proposed goal rationale

Learning status
  goal design present
  assignment/report/review/decision order
  evaluation present
  follow-up tasks / GoalCase links
```

Rules:

- a selected goal should always show its vision context when one exists;
- a vision should show its goal collection, not only the currently selected
  goal;
- the goal collection should distinguish completed goals from goals that still
  need work, review, evaluation, or replanning;
- proposed goals are not accepted goals until a Lead decision says so;
- next-round plans should be visible as bridge objects between a completed
  goal's evaluation and the next proposed goal;
- completed goals should show whether they reduced distance to the vision,
  revealed a new gap, or failed to move the vision forward;
- if no vision context is available, the Dashboard should show that as a
  workflow gap rather than silently presenting the goal as standalone work.

### Goal Operations Layout

Purpose: answer what the current goal is, why it exists, and what needs a
decision.

```text
Goal header
  objective, owner, status, success criteria, goal learning state

Goal design summary
  scenario, non-goals, chosen AgentTeam, initial task graph, evidence gates
  goal branch and production target

Goal action strip
  design present, assigned tasks, reports, review, decisions, evaluation

Git integration lane
  goal branch
  task worktrees / task PRs
  integration status into goal branch
  goal PR readiness into production

Goal graph / Kanban toggle
  graph: goal dependencies, generated goals, blockers, follow-up links
  Kanban: proposed, active, blocked, review/evaluation, complete/archived

Task graph / Kanban toggle
  graph: task dependencies, blocker edges, split/killed/follow-up tasks
  Kanban: backlog, ready, running, review, blocked, closed
  graph changes, added tasks, blocked tasks, killed/split tasks

Decision queue
  pending warnings, requested reviews, proposals, follow-up proposals
```

Rules:

- success criteria and goal learning state must be visible above the task graph;
- selected goal status should make complete versus not-complete unambiguous;
- GoalDesign should show the intended team and task graph before implementation
  details;
- file-changing goals should show goal branch, task branch/PR targets, and
  whether each task is integrated into the goal branch;
- production-branch merge readiness should be goal-level, gated by goal
  acceptance and GoalEvaluation;
- the goal board should provide both goal-level graph and goal-level Kanban
  projections before dropping into task-level execution;
- the goal board should not be only a list of task cards;
- Observer/autonomous proposals should appear as a decision queue near the goal
  header, not buried in raw evidence.

### Task Graph And Task Detail Layout

Purpose: prove a task was assigned, executed, reported, reviewed, and decided.

```text
Task graph / lanes
  grouped by operational state:
    backlog, ready, running, review, blocked, closed
  graph-change proposals and accepted revisions

Selected task detail
  assignment proof
  current owner/assignee/reviewer
  acceptance criteria
  workspace / branch / PR / owned paths
  messages and reports
  provider sessions
  evidence
  proposal/review/decision
  warnings and repair actions
```

Rules:

- compress or group low-value statuses instead of forcing all enum states into
  equal-width columns;
- task graph changes are expected and should be visible as graph-change
  proposals, decisions, added/split tasks, blockers, or follow-up tasks;
- the current graph should distinguish executable tasks from proposed or blocked
  graph changes;
- selected task detail should sit below or beside the task graph depending on
  viewport width, but it must remain in the main workbench;
- assignment proof must show `Message(kind=task)` before reports and decisions;
- owned-path and PR/worktree data must be visually close to proposals because
  that is where path violations become actionable.

### Agent Team Layout

Purpose: show whether there is a durable team, not just provider output.

```text
Goal team design
  required roles from GoalDesign
  current AgentTeam
  missing / extra / retired members

Team roster
  Lead / Observer / Worker / Critic / Specialist groups
  each member: status, queue count, current task, health summary

Member selection
  opens member inspector or member page
```

Rules:

- do not render AgentTeam as the default graph view; use a roster/control-plane
  layout first, and reserve graphs only for optional message-flow diagnostics;
- the roster should show all active team members for the selected team, not only
  members referenced by currently selected tasks;
- each goal should show the AgentTeam that was designed for it, plus any runtime
  adjustments caused by graph changes or evidence;
- adding, retiring, or replacing a member should be presented as a goal/team
  decision or graph adjustment, not as a hidden UI change;
- role grouping is more useful than alphabetical ordering for operator work;
- closed or retired members should be visually distinct from active members;
- native provider child threads are shown under the parent member and are not
  promoted to AgentMember identity unless the store says so.

### AgentMember Detail Layout

Purpose: let an operator inspect one durable member's live work surface.

Inspector tab layout:

```text
Member summary
  identity, role, provider, status, queue, current task, current proposal

Runtime health
  process, endpoint/socket, protocol, delivery, last event age

Activity timeline
  inbound task/message
  delivery claim
  provider session start/end
  AgentEvent summaries
  outbound report/message
  evidence/proposal refs

Actions
  send message, deliver, retry, reconcile, close
```

Dedicated member page layout:

```text
/members/:id
  header: member identity + runtime state
  left: current task and queue
  center: chronological activity stream
  right: runtime/session health and safe actions
```

Rules:

- inbox, outbox, sessions, events, reports, and evidence should be presented as
  one chronological activity stream when inspecting a single member;
- the activity stream may be built from polling first, but the UI should leave a
  clean path for future SSE/WebSocket event updates;
- the member page should make external chat helpers visibly different from
  durable harness `AgentMember` records;
- duplicate message ids from append-only history must be projected to latest
  state before React list rendering.

### Runtime And Provider Layout

Purpose: expose execution health without making provider transcripts the source
of truth.

```text
Runtime overview
  member, runtime id, pid/control endpoint, health layers

Provider sessions
  status, message correlation, task, thread/turn, start/end, terminal source

Child threads
  parent member, provider nickname/role, status, last message ref

Reconciliation actions
  retry safe claim, mark unresolved, fail stale session, request report
```

Rules:

- process health alone must not be shown as "working";
- session status must be correlated with message delivery state;
- unresolved accepted/running/stale sessions should block normal delivery and
  appear as operator warnings;
- provider-native child threads remain runtime evidence unless promoted.

### Evidence, Proposal, Review, And Decision Layout

Purpose: make acceptance auditable.

```text
Evidence lane
  checks, diffs, screenshots, review notes, provider outputs, docs

Proposal lane
  changed paths, evidence refs, check commands, PR/worktree refs

Review lane
  critic findings, missing evidence, path ownership, gate result

Decision lane
  Leader decision, rationale, evidence refs, follow-up tasks
```

Rules:

- these lanes belong near selected task detail because acceptance is task-local
  before it contributes to goal close;
- decision cards without evidence refs should be visually marked as incomplete;
- review evidence should be distinguished from the final Leader decision.

### Warnings And Repair Layout

Purpose: turn broken workflow links into repairable operator actions.

```text
Warnings tab
  grouped by severity and object:
    delivery, runtime, assignment, evidence, path, learning

Warning detail
  affected goal/task/member/session/proposal/decision
  why it matters
  repair action or next task
```

Rules:

- warnings should be near the object they affect and also available in a global
  queue;
- warnings cannot be only text; each warning needs navigation to the object and,
  when safe, a repair action;
- UI-only warnings remain advisory until promoted to Rust/CLI/schema/review
  gates.

### Raw Debug Layout

Purpose: preserve audit/debug access without letting raw objects become the
operator experience.

```text
Debug drawer
  snapshot import/export
  raw messages
  raw sessions
  raw evidence/decisions
  copied CLI commands
```

Rules:

- hidden by default;
- opened by an explicit debug button in the top bar;
- uses internal scrolling;
- never pushes the main control plane below the first viewport.

## Responsive Layout

Desktop, `>= 1100px`:

```text
top bar
scope rail | workbench | inspector
debug drawer collapsed
```

Tablet, `760px - 1099px`:

```text
top bar
goal header
two-column layout:
  workbench | inspector
scope rail becomes a collapsible side drawer
```

Mobile, `< 760px`:

```text
top bar
vision + goal/status strip
primary tabs:
  Work
  Member
  Warnings
  Evidence
  Debug
```

Mobile rules:

- no horizontal page overflow;
- task graph appears before raw queues;
- selected member and warning details are reachable through tabs, not stacked
  below a long board;
- summary counts collapse to compact chips;
- all buttons must keep labels readable without viewport-scaled typography.

## Data Requirements

The layout can be implemented with the current snapshot for the first pass, but
these improvements should be promoted into the read model or backend as the UI
stabilizes:

| Need | Required state or projection |
| --- | --- |
| Vision context | vision object or `vision_ref` / `vision_summary` supplied to goal close / autonomy loop, plus active pilot/scenario |
| Vision goal collection | mapping from vision to proposed, not-complete, blocked, complete, archived/rejected, and generated goals |
| Distance-to-vision | GoalEvaluation or NextRoundPlan fields that explain remaining gaps and why the next goal should exist |
| Goal ladder | proposed goals, active goals, evaluated goals, next-round proposals, and Lead disposition |
| Goal team design | GoalDesign evidence or future schema fields for required roles, selected AgentTeam, and role gaps |
| Task graph revisions | graph-change proposals, decisions, added/split/blocked tasks, and follow-up task links |
| Stable React lists | latest-row projection for append-only object ids before rendering |
| Member activity stream | merged timeline of messages, delivery updates, provider sessions, AgentEvents, evidence refs |
| Member page route | URL-addressable selected member id |
| Team roster accuracy | active team members independent of selected task references |
| Runtime readiness | process, endpoint, protocol, and delivery health shown separately |
| Debug drawer | raw snapshot sections outside primary page flow |
| Warning repair | warning-to-action mapping with safe API endpoints |

Implementation order and acceptance evidence are defined in
[acceptance.md](acceptance.md).
