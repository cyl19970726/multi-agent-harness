# Team Workspace Page Spec

```text
status: planned
owner_role: product-design
canonical_for: persistent AgentTeam collaboration workspace
route_or_surface: /teams/:teamId and default /
```

## Purpose

Primary user question: who is working, who is blocked, what messages or
decisions need attention, and what should the operator do next?

Why it exists: AgentTeam is a persistent collaboration space, not a temporary
job runner or graph. The default Workbench surface should feel closer to a
Feishu/Slack team workspace than a metrics dashboard.

Non-goals:

- do not reduce Team to a roster card;
- do not use graph as the default Team view;
- do not make Team a chat-only surface with no Goal/Task proof.

## Objects And Proof

Canonical objects:

- AgentTeam;
- AgentMember;
- Goal;
- Task;
- Message;
- Evidence;
- Proposal;
- Decision;
- AgentEvent;
- ProviderSession;
- warnings.

Workflow proof:

- standing roster remains visible even when a member has no current task;
- role groups, queue counts, last event, runtime status, and current work are
  visible together;
- every activity item maps to Message, Task, Evidence, Proposal, Decision,
  session, event, or warning;
- decision and warning pressure is visible without raw JSON.

Source docs:

- [../../dashboard.md](../../dashboard.md)
- [../../agent-control-plane.md](../../agent-control-plane.md)
- [../design-principles.md](../design-principles.md)

Read-model inputs:

- `teamWorkspace(snapshot, teamId, goalId?)`;
- `memberWorkbench(snapshot, memberId)`;
- message queues by team/member;
- decision and warning queues.

## Page-Level Agent Loop

Designer options:

- collaboration workspace: team rail, central activity/work area, inspector;
- operations console: roster and queue matrix first;
- goal-scoped team panel: team embedded under selected Goal.

Questioner challenges:

- Does Team feel durable across tasks/goals?
- Are messages and current work visible without becoming chat-only?
- Can the operator find the next action in the first viewport?

Reviewer decision: use collaboration workspace. Borrow dense queue affordances
from operations console and a pinned Goal health strip from goal-scoped panel.

Rejected options:

- static operations console: too dashboard-like;
- goal-scoped team panel: hides standing-team continuity.

Borrowed ideas:

- role gaps and queue density from operations console;
- active Goal health from goal-scoped panel.

## Information Architecture

Selected IA:

```text
team rail
  -> role groups and members
central workspace
  -> active Vision/Goal strip
  -> team activity and current work lanes
  -> decision and warning queue
inspector
  -> selected member/task/docs/warnings
```

Primary actions: select member, send message, deliver queued work, open current
task, request review, inspect warning, open docs.

Secondary actions: filter role group, open Goal/Task document, open debug.

Empty/loading/error states:

- empty: show no active team and offer safe source/debug path;
- loading: preserve rail/workspace/inspector geometry;
- error: show API/source failure without replacing the workbench with raw JSON.

Responsive requirements:

- desktop: team rail + workspace + inspector;
- tablet: team rail collapses to drawer, inspector remains second column;
- mobile: Team tab with role groups, queue summary, critical warnings, and
  decision pressure.

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | active Vision/Goal | search | command | debug |
+-----+----------------------+-----------------------------------+---------------+
| app | team rail 280        | team workspace 720                | inspector 376 |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | team switcher    | | | goal strip 76                 | | | member    | |
|     | +------------------+ | | | objective/status/warnings     | | | identity  | |
|     | | Lead group       | | +-------------------------------+ | +-----------+ |
|     | | - member row 56  | | | activity stream 360           | | | inbox     | |
|     | | Critic group     | | | canonical rows:               | | | outbox    | |
|     | | Worker group     | | | msg/task/evidence/decision    | | +-----------+ |
|     | | Observer group   | | +-------------------------------+ | | runtime   | |
|     | +------------------+ | | | work pressure 220             | | | actions   | |
|     | | queues/current   | | | current tasks + decisions     | | +-----------+ |
|     | +------------------+ | +-------------------------------+ | inspector   |
|     | team rail scroll     | workspace scroll below goal strip  | scroll      |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- team rail `280px`;
- workspace min `700px`;
- inspector `360px` to `380px`;
- member row fixed `56px`;
- goal strip `76px`;
- activity row `72px` to `96px`;
- work pressure region target `200px` to `240px`.

First viewport content:

- team switcher, role groups, members, queue counts, current work;
- active Vision/Goal strip;
- activity stream mapped to canonical objects;
- current tasks and decision pressure;
- selected member summary with inbox/outbox/runtime/actions.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | live/source | active Goal | search | debug    |
+-----+---------------------------------------+--------------------+
| app | team workspace 548                   | inspector 288      |
| 56  | +-----------------------------------+| +----------------+ |
|     | | goal strip 76                    || | selected       | |
|     | +-----------------------------------+| | member/task    | |
|     | | activity stream                  || | inbox/outbox   | |
|     | | current work + decisions         || | runtime        | |
|     | | warning queue                    || +----------------+ |
|     | workspace scroll                     | inspector scroll  |
+-----+---------------------------------------+--------------------+
| team rail drawer 320 closed; opens over workspace                 |
+------------------------------------------------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: live/source | search | debug |
+--------------------------------------+
| goal strip 72: active goal + warns   |
+--------------------------------------+
| tabs 52: Team Work Member Warn Docs  |
+--------------------------------------+
| Team tab 672                         |
| +----------------------------------+ |
| | team switcher + role filter      | |
| +----------------------------------+ |
| | role group: Lead                 | |
| | member row: status/queue/task    | |
| | role group: Worker/Critic        | |
| +----------------------------------+ |
| | decision pressure + warnings     | |
| | latest canonical activity rows   | |
+--------------------------------------+
```

Scroll ownership:

- desktop: team rail, workspace, and inspector each scroll internally;
- tablet: team rail is drawer; workspace and inspector scroll separately;
- mobile: only the active tab scrolls.

Screenshot acceptance:

- first impression must be a collaboration workspace, not roster/cards;
- member rows must expose queue/current work/runtime;
- activity must map to canonical objects;
- next operator action must be visible without raw JSON.

## Failure Modes

- Team as roster only;
- activity stream with no canonical object links;
- first viewport becomes metric cards;
- AgentMember only available as a side card;
- raw/debug controls become default content.

## Screenshot Acceptance Questions

- Does the first viewport look like a team workspace?
- Can the reviewer identify active team, member state, current work, queues,
  warnings, and decisions without raw JSON?
- Is the selected Member reachable as a teammate, not a card?
- Would an operator know the next action?
