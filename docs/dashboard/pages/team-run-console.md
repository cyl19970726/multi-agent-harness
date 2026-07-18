# Team Run Console Page Spec

```text
status: planned
owner_role: product-design
canonical_for: AgentTeamRun live observation console (ADR 0025)
route_or_surface: /team-console and /team-console/:runId
```

## Purpose

Primary user question: what is every member of this run doing right now —
runtime, execution status, current task, current action — what is blocked
or waiting on authorization, and what should the operator do next?

Why it exists: what the page shows is decided by the problem it solves. An
`AgentTeamRun` is the living version of a checkpoint report: the task
graph, the assignment/handoff/blocker messages, the evidence ledger, and
the ordered event stream of one wave of execution. The console must make
those observable without raw JSON and without opening provider transcripts.

The console is also where a run is born. Creating a Team Run must make
member configuration explicit: for every Agent Member the operator sees
and edits role, provider, model, `ownedPaths`, and budget before launch.
"How do I configure each member" must never be a hidden prompt detail.

Non-goals:

- not a standing-team workspace or org directory (that older direction
  stays in [team-workspace.md](team-workspace.md); a run is ephemeral and
  ends read-only);
- not a metrics wall; summary counts never push the proof chain below the
  fold;
- raw provider stream is diagnostics, never default content;
- no hidden model reasoning anywhere on the page.

## Objects And Proof

Canonical objects:

- AgentTeamRun;
- MemberRun;
- TeamMessage (with per-recipient delivery records);
- MemberAction;
- DelegationRun;
- TeamRunEvent;
- Task, Evidence, Goal, GoalPhase (the wave the run attaches to).

Workflow proof:

- member state is derived (`MemberLiveState` = runtime + latest
  MemberAction + message queue + delegation + heartbeat), never provider
  self-reported `running`;
- every visible element maps back to a canonical object above, or is
  explicitly marked advisory/debug;
- handoff and key-task messages show ACK state; un-ACKed deliveries past
  threshold surface as alerts;
- authorization gates (deploy / merge / remote deletion) render as
  first-class blocked tasks with an operator decision, not as log lines;
- capability degradation is honest: `unverified` / `unsupported` badges
  and degraded `dynamic_workflow` delegations are labeled, never
  presented as unified capability;
- a completed run renders the same page in read-only history mode.

Source docs:

- [../../decisions/0025-agent-team-run-control-plane.md](../../decisions/0025-agent-team-run-control-plane.md)
- [../../concept-model.md](../../concept-model.md)
- [../../dashboard.md](../../dashboard.md)
- [../design-principles.md](../design-principles.md)

Read-model inputs:

- `teamRunConsole(runId)` — the single read model shared by the Codex App
  console, the Browser Dashboard, and the CLI text view;
- the run's `TeamRunEvent` stream over SSE (monotonic `seq`, resume via
  Last-Event-ID);
- the message/delivery ledger for the run.

## Page-Level Agent Loop

Designer options:

- mission-control console: member rail, cockpit table + task graph +
  live timeline, member inspector;
- chat-centric team room: message stream first, members as participants;
- graph-centric run map: task DAG as the full page, members on nodes.

Questioner challenges:

- Can the operator read all members' four-axis state in the first
  viewport?
- Is the next operator action (authorize, re-deliver, inject, interrupt)
  reachable without opening a transcript?
- Does the page show degradation honestly instead of pretending unified
  capability?

Reviewer decision: use mission-control console. The cockpit table answers
"everyone's state at a glance"; the task graph carries owner / reviewer /
deps / barrier / authorization gates; the inspector carries the drill-in.

Rejected options:

- chat-centric team room: hides the task graph and gates behind
  conversation;
- graph-centric run map: the DAG cannot show per-member live action and
  message pressure.

Borrowed ideas:

- message pressure and queue density from the chat-centric option (folded
  into cockpit columns and inspector tabs);
- dependency emphasis from the graph-centric option (kept as the center
  Task Graph panel, not a full canvas).

## Information Architecture

Selected IA:

```text
left rail
  -> MemberRuns grouped by wave / role (an execution roster, not an
     address book)
center
  -> Goal strip (objective, wave index, budget, elapsed)
  -> team cockpit: one row per member, four-axis state
  -> Task Graph (owner / reviewer / deps / barrier / authorization gate)
  -> Live Timeline (ordered TeamRunEvent stream)
right inspector
  -> selected MemberRun: MemberLiveState summary
  -> current action card
  -> tabs: action timeline / delegations / messages / raw provider stream
```

Three-layer observation, in increasing detail:

1. **Team cockpit** — one table row per member: runtime, execution
   status, current task, current action, plus change/test counts,
   delegation count, heartbeat, and unread deliveries.
2. **Member action timeline** — the member's `MemberAction` list; each
   row expands to input/output summaries, commands, file changes, test
   results, linked messages, and evidence refs.
3. **Raw provider stream** — the provider's own event frames, collapsed
   by default, for adapter diagnostics only; sanitized before storage.

Primary actions: create team run (opens the member configuration
composer), select member, approve or reject an authorization gate, inject
a message into the current turn, interrupt the current turn, re-deliver
an un-ACKed message, pause/resume a member run, end the team run.

Secondary actions: filter rail by wave/role, expand raw provider stream,
open the run's dashboard URL from host software, open the linked Goal or
Task document, open debug.

Member configuration composer (create flow):

- one slot editor per member: role, provider (codex|claude|kimi), model,
  `ownedPaths`, budget;
- run-level fields: objective, wave index, budget limit, optional team
  definition reference;
- capability hints per provider shown from the adapter's declared
  snapshot, with `unverified` / `unsupported` labels where applicable;
- launch is blocked until every slot's required fields are explicit.

Empty/loading/error states:

- empty: no active run — offer the create composer and the read-only
  history list of past runs;
- loading: preserve rail/center/inspector geometry;
- error: show SSE/store failure with the last good `seq` cursor, never
  replace the console with raw JSON;
- degraded: SSE disconnected -> banner with reconnect/resume state; the
  store stays the source of truth.

Responsive requirements:

- desktop: member rail + center console + inspector;
- tablet: rail collapses to a drawer, inspector becomes a second column;
- mobile: cockpit-first single column — member rows with four-axis
  state, blocked/authorization alerts, and the latest timeline entries.

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | run + wave | budget | search | debug         |
+-----+----------------------+-----------------------------------+---------------+
| app | member rail 264      | team run console 736              | inspector 376 |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | run switcher     | | | goal strip 64                 | | | member    | |
|     | +------------------+ | | | objective/wave/budget       | | | live state| |
|     | | Wave 1           | | +-------------------------------+ | +-----------+ |
|     | | - member row 60  | | | team cockpit 220              | | | current   | |
|     | | Lead / Worker /  | | | 4-axis rows + heartbeat       | | | action    | |
|     | | Reviewer groups  | | +-------------------------------+ | | card 96   | |
|     | | Wave 2 (planned) | | | task graph 200                | | +-----------+ |
|     | +------------------+ | | owner/reviewer/deps/gates     | | | tabs:     | |
|     | | alerts: blocked  | | +-------------------------------+ | | timeline/ | |
|     | | un-ACKed / gates | | | live timeline 220             | | | delegates | |
|     | +------------------+ | +-------------------------------+ | | messages/ | |
|     | rail scroll          | center scroll below goal strip    | | raw       | |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- member rail `264px`;
- center min `700px`;
- inspector `360px` to `380px`;
- member row fixed `60px` with four-axis chips;
- goal strip `64px`;
- cockpit row `40px` to `48px`;
- task graph target `180px` to `220px`;
- live timeline target `200px` to `240px`.

First viewport content:

- run identity, wave, status, budget used/limit, elapsed;
- every member's four-axis state (runtime / execution status / current
  task / current action) with live pulse on active members;
- task graph with blocked/authorization-gate emphasis;
- alerts: blocked gates, un-ACKed deliveries, degraded capabilities;
- selected member's live state and current action.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | live/source | run | budget | search | debug   |
+-----+---------------------------------------+--------------------+
| app | team run console 556               | inspector 280        |
| 56  | +-----------------------------------+| +----------------+ |
|     | | goal strip 64                    || | member live    | |
|     | +-----------------------------------+| | current action | |
|     | | team cockpit (4-axis rows)       || +----------------+ |
|     | +-----------------------------------+| | tabs (compact) | |
|     | | task graph                        || | timeline/msgs  | |
|     | +-----------------------------------+| +----------------+ |
|     | | live timeline                     || inspector scroll  |
|     | +-----------------------------------+|                    |
|     | center scroll                       |                    |
+-----+---------------------------------------+--------------------+
| member rail drawer 300 closed; opens over console                 |
+------------------------------------------------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: live/source | run | debug    |
+--------------------------------------+
| goal strip 64: objective + budget    |
+--------------------------------------+
| alerts 56: gates / un-ACKed / degraded|
+--------------------------------------+
| tabs 48: Cockpit Graph Timeline Msgs |
+--------------------------------------+
| Cockpit tab 620                      |
| +----------------------------------+ |
| | member row: 4-axis state chips   | |
| | member row: action + heartbeat   | |
| +----------------------------------+ |
| | blocked gate card w/ approve CTA | |
| +----------------------------------+ |
| | latest timeline entries          | |
+--------------------------------------+
```

Scroll ownership:

- desktop: member rail, center console, and inspector each scroll
  internally; goal strip is pinned above center scroll;
- tablet: rail is a drawer; center and inspector scroll separately;
- mobile: only the active tab scrolls; alerts stay pinned.

Visual system (per [../design-principles.md](../design-principles.md)):

- live pulses on running members and active delegations;
- explicit state colors for running / waiting / blocked / reviewing /
  idle / completed / failed;
- dense but readable; dark technical theme allowed only with strong
  contrast;
- capability badges (`verified` / `unverified` / `unsupported`) are
  state, not decoration.

Screenshot acceptance:

- first impression is a mission-control console for one run, not a roster
  or a chat room;
- cockpit rows expose the four-axis state without drill-in;
- task graph shows owner/reviewer/deps and at least one gate state;
- a degraded capability is visibly labeled as degraded;
- member configuration composer shows per-member role/provider/model/
  ownedPaths/budget;
- no raw JSON and no raw provider stream in the default viewport.

## Failure Modes

- Console as member roster or card wall with no task graph or gates;
- member state taken from provider self-report instead of derived
  `MemberLiveState`;
- capability degradation hidden or rendered as if unified;
- raw provider stream promoted to default content;
- member configuration buried so users cannot perceive how each member is
  set up;
- message list with no ACK/delivery state or evidence refs;
- a completed run still rendered as mutable;
- metric cards pushing cockpit, graph, and timeline below the fold.

## Screenshot Acceptance Questions

- Does the first viewport answer "what is every member doing right now"
  on all four axes?
- Can the reviewer find the next operator action (authorize, re-deliver,
  inject, interrupt) without opening a transcript?
- Are blocked authorization gates and un-ACKed messages visible as
  first-class alerts?
- Is degraded provider capability labeled honestly (`unverified` /
  `unsupported`)?
- Does creating a run make each member's role, provider, model,
  ownedPaths, and budget explicit before launch?
- Does every visible element map back to AgentTeamRun, MemberRun,
  TeamMessage, MemberAction, DelegationRun, or TeamRunEvent?

## Open Questions

- Whether the member configuration composer is shared with future
  TeamDefinition editing, or stays run-local in v0.
- How much of the cockpit table the Codex App in-app console renders
  versus linking out to the Browser Dashboard.
- Whether the task graph panel reuses the existing goal/task graph
  projection or takes a run-scoped read model of its own.
