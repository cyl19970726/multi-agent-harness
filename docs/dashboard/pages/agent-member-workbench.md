# AgentMember Workbench Page Spec

```text
status: planned
owner_role: product-design
canonical_for: focused AgentMember runtime and message workspace
route_or_surface: /members/:memberId and inspector Member tab
```

## Purpose

Primary user question: what is this member doing, what did it receive, what did
it send, and what evidence supports its claims?

Why it exists: AgentMember should feel like a durable teammate with identity,
responsibility, memory of messages, runtime state, and current work. It is not
a disposable provider turn and not a static side panel.

Non-goals:

- do not show only avatar, status, and current task;
- do not hide inbox/outbox behind raw messages;
- do not treat provider sessions as separate from member identity.

## Objects And Proof

Canonical objects:

- AgentMember;
- inbox/outbox Message;
- MessageDelivery;
- Task;
- Proposal;
- Evidence;
- ProviderSession;
- ProviderChildThread;
- AgentEvent;
- prompt refs, skill refs, permissions.

Workflow proof:

- chronological timeline merges task assignment, reports, sessions, events,
  evidence, proposals, and delivery state;
- inbox/outbox are distinct and countable;
- runtime health separates process, endpoint, protocol, and delivery;
- current task/proposal and prompt/skills are visible.

Source docs:

- [../../agent-runtime.md](../../agent-runtime.md)
- [../../agent-control-plane.md](../../agent-control-plane.md)
- [../read-model.md](../read-model.md)

Read-model inputs:

- `memberWorkbench(snapshot, memberId)`;
- `memberTimeline(snapshot, memberId)`;
- queued/delivered/failed messages;
- provider sessions and child threads by member.

## Page-Level Agent Loop

Designer options:

- person workbench: identity, current work, inbox/outbox, timeline, runtime;
- chat-first: messages as the primary surface;
- runtime console: sessions and process health first.

Questioner challenges:

- Does the member feel like a person with a workstation?
- Can the operator see what was assigned before reports/evidence?
- Are runtime and provider details connected to messages?

Reviewer decision: use person workbench. Borrow readable message grouping from
chat-first and health layering from runtime console.

Rejected options:

- chat-first: hides protocol/evidence proof;
- runtime console: too operational and not enough collaboration context.

Borrowed ideas:

- message grouping;
- runtime health layers.

## Information Architecture

Selected IA:

```text
identity and role header
  -> current task/proposal and actions
  -> inbox/outbox summary
  -> chronological activity timeline
  -> runtime health and sessions
  -> prompt/skills/permissions
```

Primary actions: send message, deliver queued work, retry failed delivery,
request report, open current task, open evidence.

Secondary actions: reconcile session, inspect prompt/skills, close member with
explicit destructive confirmation.

Empty/loading/error states:

- empty: member exists but has no messages/sessions yet;
- loading: stable identity/action/timeline skeleton;
- error: show failed member read or API error with retry.

Responsive requirements:

- desktop: full route uses identity/current work, timeline, and runtime columns;
- inspector: compact summary plus timeline preview;
- mobile: Member tab with identity, action row, current work, timeline,
  runtime, inbox/outbox, prompt/skills.

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | selected member | search | command | debug    |
+-----+----------------------+-----------------------------------+---------------+
| app | member rail 280      | member workspace 720              | runtime 376   |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | identity         | | | current work 120              | | | process   | |
|     | | role/team/status | | | task/proposal/actions          | | | endpoint  | |
|     | +------------------+ | +-------------------------------+ | | protocol  | |
|     | | inbox 96         | | | inbox/outbox split 160        | | | delivery  | |
|     | | outbox 96        | | | latest assignment/report      | | +-----------+ |
|     | +------------------+ | +-------------------------------+ | | sessions  | |
|     | | prompt/skills    | | | timeline 480                  | | | child     | |
|     | | permissions      | | | msg/session/event/evidence    | | | threads   | |
|     | +------------------+ | +-------------------------------+ | +-----------+ |
|     | rail scroll          | timeline scroll                    | runtime scroll|
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- member rail `280px`;
- workspace min `700px`;
- runtime panel `360px` to `380px`;
- current work `120px`;
- inbox/outbox split `160px`;
- timeline owns remaining height with rows `72px` to `104px`.

First viewport content:

- identity, role, team, prompt/skills, permission signal;
- current task/proposal and safe actions;
- inbox/outbox counts and latest messages;
- chronological timeline;
- runtime health split by process, endpoint, protocol, delivery;
- sessions and child threads.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | selected member | search | debug             |
+-----+---------------------------------------+--------------------+
| app | member workspace 548                 | runtime 288        |
| 56  | +-----------------------------------+| +----------------+ |
|     | | identity + current work/actions   || | health stack    | |
|     | +-----------------------------------+| | sessions       | |
|     | | inbox/outbox summary              || | child threads  | |
|     | +-----------------------------------+| +----------------+ |
|     | | timeline                          | runtime scroll     |
|     | | canonical activity rows           |                    |
+-----+---------------------------------------+--------------------+
| member meta drawer closed                                      |
+------------------------------------------------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: member | live/source | debug |
+--------------------------------------+
| identity 104: role/status/current    |
+--------------------------------------+
| action row 48: Send Deliver Retry    |
+--------------------------------------+
| tabs 52: Timeline Inbox Runtime Meta |
+--------------------------------------+
| active tab 592                       |
| Timeline: msg/session/event rows     |
| Inbox: queued/delivered/failed msgs  |
| Runtime: health + sessions           |
| Meta: prompt/skills/permissions      |
+--------------------------------------+
```

Scroll ownership:

- desktop: member rail, timeline workspace, and runtime panel scroll
  separately;
- tablet: workspace and runtime panel scroll separately;
- mobile: only the active tab scrolls.

Screenshot acceptance:

- member must look like a durable teammate with a workstation;
- inbox/outbox, timeline, runtime, current work, and actions are visible;
- assignment appears before report/evidence in the timeline;
- provider/session details stay under member identity.

## Failure Modes

- member is only a status card;
- inbox/outbox absent;
- timeline is decorative and not linked to canonical objects;
- runtime health detached from delivery state;
- actions simulate local state instead of creating messages/API calls.

## Screenshot Acceptance Questions

- Does the member look like a durable teammate with a workbench?
- Are inbox, outbox, timeline, runtime, current work, and actions visible?
- Can a reviewer trace assignment before report/evidence?
- Does the route avoid becoming a raw provider-session dump?
