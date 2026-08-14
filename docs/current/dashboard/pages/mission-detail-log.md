# Mission Detail and Log Page Spec

```text
status: implemented baseline
owner_role: product-design
canonical_for: Mission context, its Mission-owned AgentTeam, append-only Mission Log,
               Host judgment, replan/recovery decisions, and closeout
route_or_surface: Missions -> Mission
architecture: ADR 0034 + ADR 0037 + ADR 0044 + ADR 0050 + ADR 0051
```

## User Problem

The Host and Human need one readable surface for:

- what the Mission means and how success is judged;
- which flat AgentTeam owns execution for the Mission;
- what the Host currently believes and plans;
- which Works are complete, active, blocked, or intentionally carried forward;
- how the Host recorded judgment, replan, recovery, and closeout; and
- why the Mission advanced or closed.

The page is not a task graph, scheduler, transcript, TeamRun attempt browser,
or a second lifecycle layered beneath Mission.

## Canonical Semantics

```text
Mission -> append-only MissionLogEntry
Mission <-> exactly one flat AgentTeam
AgentTeamRun(agent_team_id, execution_node_id, project_binding_id)
  -> MemberRun -> native session
```

`MissionLogEntry` is an append-only record inside the Mission. It records Host
judgment, plan changes, recovery, and closeout; it is not a lifecycle object,
executor container, synchronization barrier, or permission gate.

Required projections:

- `Mission`: title, objective, Markdown context, status, owning flat AgentTeam
  (derived by `AgentTeam.mission_id`), provenance, and closeout;
- Mission Log: ordered entries with kind, Markdown body, actor, timestamp, and
  immutable history. A body may cite Work, outcome, or artifact identifiers as
  Markdown; `MissionLogEntry` does not currently own typed relations to them;
- owning Team: stable identity, Host, Node placement, composition, latest runs,
  current Supervisor/reconnect health, member/Work status, and open-Team action;
- Works: compact Mission-owned Team summaries for active, blocked, review, and
  carry-over state; detailed scheduling remains in Team Workbench;
- Messages: authored conversation linked to Work when relevant; and
- correlated question Messages and evidence that require Host or Human judgment.

ADR-0051-predecessor Wave rows may remain readable only in a clearly labeled
Legacy/history surface for export and audit. No current authoring control may
create, edit, advance, or gate a Wave.

## Desktop Layout

Use the shared Workbench shell: product sidebar, readable center canvas, and
context rail.

```text
+----------------------+--------------------------------------+------------------+
| Product sidebar      | Mission header                       | Mission brief    |
|                      | status · owning Team · actions       | Needs You        |
| Active context tree  +--------------------------------------+ Owning Team      |
|                      | Mission context (Markdown)           | Current judgment |
|                      +--------------------------------------+ Runtime summary  |
|                      | Mission Log                          |                  |
|                      | · judgment / plan update             |                  |
|                      | · recovery / decision                |                  |
|                      | · closeout                           |                  |
|                      +--------------------------------------+                  |
|                      | Works summary / evidence             |                  |
+----------------------+--------------------------------------+------------------+
```

Keep Mission context readable at long-document length. The center column
scrolls independently and never clips the final Log entry. Markdown headings,
lists, tables, code, links, and artifact references render semantically.

A responsibility table may be authored as ordinary Markdown. It is
explanatory; Work records remain ownership and state truth.

## Context Rail

Compose flexible compact modules:

1. **MissionBrief** — durable context excerpt, status, source, and closeout.
2. **NeedsYou** — unresolved question Messages, blockers, or review requests.
3. **MissionTeam** — the Mission's one immutable flat AgentTeam, member state,
   latest run, and open Team action. Render complete TeamRun history without
   inventing a mutable or one-to-many Mission/Team relation.
4. **CurrentJudgment** — latest relevant Mission Log entry, cited Works,
   artifacts, and history action. Each citation includes version and timestamp
   so a past judgment cannot appear to own current Work state.
5. **LegacyWaveHistory** — optional, read-only, and visible only when
   predecessor data exists.

Cards are quiet structural containers, not a wall of elevated analytics tiles.

## Actions

- Create and edit Mission Markdown context.
- Link the Mission's AgentTeam during canonical Mission/Team creation.
- Open Team War Room or Member Focus from linked member controls.
- Append a Mission Log entry for Host judgment, replan, recovery, or closeout.
- Close the Mission with an explicit outcome. Never archive/delete the Team as
  a side effect.

Appending a Log entry does not finish active Work, end a TeamRun, close a
Member, or authorize a provider effect. Sensitive external actions still
require their own Human Approval.

## Responsive Behavior

- **Tablet:** collapse the product sidebar; move the context rail into an
  accessible sheet/inline region; retain full Mission and Mission Log Markdown.
- **Mobile:** show one readable Mission story; context opens as a bottom sheet;
  responsibility tables scroll within their own container; member controls
  open full pages.
- Preserve browser history, focus, scroll position, and deep-link parameters
  across Mission -> Team -> Member navigation.

## States

- Empty Log: show Mission context and one clear “Record Host judgment” action.
- Missing Team: show an invalid/incomplete-model warning; do not invent a
  mutable Team-linking workflow.
- Active carry-over: show current Work plus the Log entry that cited it without
  moving runtime ownership into that entry.
- Missing native session: retain Harness coordination and label native detail
  unavailable; never invent transcript content.
- Offline/stale: preserve the last projection with timestamp and scoped retry.
- Historical Wave row: readable in Legacy/history, explicit compatibility
  label, no current authoring controls.

## Screenshot And UX Acceptance

At desktop acceptance the first viewport must show the Mission heading/context
start, owning Team, latest Host judgment, Mission Log start, Works summary, and
an available append-Log action. Long Markdown must be reachable without
clipping. Test:

- vertical scrolling to the end of long Mission/Log context;
- every Team/Member control navigates and returns correctly;
- Markdown tables and long text do not overflow;
- active member work survives appended Mission Log entries;
- loading, empty, error, carry-over, Legacy/history, and closeout states; and
- actual screenshot against the approved expected reference.

## Explicit Boundaries

- Mission owns durable intent and lifecycle.
- Mission Log stores append-only Host judgment; it owns no runtime.
- `source_plan_ref` is navigation metadata, not a lifecycle edge.
- TeamRun completion does not close a Mission; a Log entry does not complete a
  TeamRun.
- Wave create/update/advance/gate is retired. Historical Wave data is
  Legacy/read/export only.
- Agent Team and Agent Membership pages may share UI primitives but not identity
  or lifecycle semantics.
