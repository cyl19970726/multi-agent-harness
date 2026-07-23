# Mission / Wave Canvas Page Spec

```text
status: implemented
owner_role: product-design
canonical_for: Mission context, linked Agent Teams, ordered Host-plan Waves,
               Wave revision history, advance decisions, and closeout
route_or_surface: Missions -> Mission -> selected Wave
architecture: ADR 0034
```

## User Problem

The Host and Human need one readable surface for:

- what the Mission means and how success is judged;
- which independent Agent Teams are available to the Mission;
- what the Host currently believes and plans;
- which assignments are complete, active, blocked, or intentionally carried
  forward; and
- why the Host advanced or closed the Mission.

The page is not a task graph, scheduler, transcript, or TeamRun attempt browser.

## Canonical Semantics

```text
Mission -> ordered Host-plan Wave
Mission <-> independent AgentTeam
AgentTeamRun(mission_id, agent_team_id) -> MemberRun -> native session
```

Required projections:

- `Mission`: title, objective, Markdown context, status, linked
  `agent_team_ids`, provenance, and closeout;
- ordered `Wave`: title, objective, Markdown context, revision, updated actor,
  advance outcome, artifacts, and history;
- linked teams: stable identity, composition, latest Mission-scoped runs,
  member/assignment status, and open-Team action;
- messages: assignment correlation and optional `origin_wave_id` for
  explanation/carry-over;
- pending interactions and evidence that require Host or Human judgment.

Legacy direct-Wave-executor rows remain readable with a visible compatibility
label. They are not the default authoring path.

## Desktop Layout

Use the shared Workbench shell: product sidebar, readable center canvas, and
context rail.

```text
+----------------------+--------------------------------------+------------------+
| Product sidebar      | Mission header                       | Mission brief    |
|                      | status · linked teams · actions      | Needs You        |
| Active context tree  +--------------------------------------+ Linked teams     |
|                      | Mission context (Markdown)           | Selected Wave    |
|                      +--------------------------------------+ Runtime summary  |
|                      | Wave 1 · advanced (compact)          |                  |
|                      +--------------------------------------+                  |
|                      | Wave 2 · selected                     |                  |
|                      | full Markdown Host plan              |                  |
|                      | responsibility table                 |                  |
|                      | assignments / carry-over / evidence  |                  |
|                      +--------------------------------------+                  |
|                      | Wave 3 · planned (compact)           |                  |
+----------------------+--------------------------------------+------------------+
```

Keep the Mission context readable at long-document length. The center column
scrolls independently and never clips the final Wave. Markdown headings, lists,
tables, code, links, and artifact references render semantically.

The selected Wave expands in place. A responsibility table may be authored as
ordinary Markdown:

```markdown
| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| Builder | Primary builder | Integrate the baseline | Patch and checks |
| Reviewer | Reviewer | Continue interaction validation | Review report |
```

This table is explanatory. Assignment messages remain ownership truth.

## Context Rail

Compose flexible compact modules:

1. **MissionBrief** — durable context excerpt, status, source, and closeout.
2. **NeedsYou** — real PendingInteractions, blockers, or approval requests.
3. **MissionTeams** — linked Agent Teams, member state, latest run, and open
   Team action. This is Mission-scoped, not nested under one Wave.
4. **SelectedWave** — revision, updated actor, judgment excerpt, carry-over,
   artifacts, and history action.
5. **LegacyExecutor** — only for historical direct-executor data.

Cards are quiet structural containers, not a wall of elevated analytics tiles.

## Actions

- Create and edit Mission Markdown context.
- Link an existing Agent Team or create and link a new one.
- Open Team War Room or Member Focus from any linked member control.
- Create, edit, and inspect history for ordered Waves.
- Advance the selected Wave with an explicit Host outcome even while unrelated
  members remain active.
- Create Wave N+1 and keep the same TeamRun, MemberRun, assignments, and native
  sessions where the Host chooses carry-over.
- Close the Mission with an explicit outcome. Never archive/delete teams as a
  side effect.

Advance confirmation summarizes active carry-over but does not require it to
finish. Sensitive external actions still require their own Human Approval.

## Responsive Behavior

- **Tablet:** collapse the product sidebar; move the context rail into an
  accessible sheet/inline region; retain full Mission and Wave Markdown.
- **Mobile:** one expanded Wave at a time; context opens as a bottom sheet;
  responsibility tables scroll within their own container; member controls
  open full pages.
- Preserve browser history, focus, scroll position, and deep-link parameters
  across Mission -> Team -> Member navigation.

## States

- No Waves: show Mission context and one clear “Create first Wave” action.
- No linked team: explain that Host work may remain direct and offer
  link/create; do not imply a team is mandatory.
- Active carry-over: show origin Wave and current assignment state without
  moving runtime ownership into the selected Wave.
- Missing native session: retain Harness coordination and label native detail
  unavailable; never invent transcript content.
- Offline/stale: preserve the last projection with timestamp and scoped retry.
- Historical legacy row: readable, explicit compatibility label, no new legacy
  authoring controls.

## Screenshot And UX Acceptance

At desktop acceptance the first viewport must show Mission context, linked
teams, ordered Waves, one expanded full Markdown Wave, responsibility table,
and an available Host advance decision. Test:

- vertical scrolling to the end of long Mission/Wave context;
- every Team/Member control navigates and returns correctly;
- Markdown tables and long text do not overflow;
- active member work survives Wave advance in the projection;
- loading, empty, error, carry-over, and closeout states;
- actual screenshot against the approved expected reference.

## Explicit Boundaries

- Wave stores Host plan and judgment, not task/runtime ownership.
- `origin_wave_id` is navigation metadata, not a lifecycle edge.
- TeamRun completion does not advance a Wave; Wave advance does not complete a
  TeamRun.
- Agent Team and Standing Agent pages may share UI primitives but not identity
  or lifecycle semantics.
