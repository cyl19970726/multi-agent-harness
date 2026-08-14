# Mission / Wave Canvas Page Spec

```text
status: implemented baseline; Works projection redesign pending ADR 0050
owner_role: product-design
canonical_for: Mission context, its Mission-owned AgentTeam, ordered Host-plan Waves,
               Wave revision history, advance decisions, and closeout
route_or_surface: Missions -> Mission -> selected Wave
architecture: ADR 0034 + ADR 0037 + ADR 0044 + ADR 0050
```

## User Problem

The Host and Human need one readable surface for:

- what the Mission means and how success is judged;
- which flat AgentTeam owns execution for the Mission;
- what the Host currently believes and plans;
- which Works are complete, active, blocked, or intentionally carried
  forward; and
- why the Host advanced or closed the Mission.

The page is not a task graph, scheduler, transcript, or TeamRun attempt browser.

## Canonical Semantics

```text
Mission -> ordered Host-plan Wave
Mission -> exactly one flat AgentTeam
AgentTeamRun(agent_team_id, execution_node_id, project_binding_id)
  -> MemberRun -> native session
```

Required projections:

- `Mission`: title, objective, Markdown context, status, linked
  its owning flat AgentTeam (derived by `AgentTeam.mission_id`), provenance, and closeout;
- ordered `Wave`: title, objective, Markdown context, revision, updated actor,
  advance outcome, artifacts, and history;
- owning Team: stable identity, Host, Node placement, composition, latest runs,
  current Supervisor/reconnect health, member/Work status, and open-Team
  action;
- Works: compact Mission-owned Team summaries for active, blocked, review and
  carry-over state; detailed scheduling remains in Team Workbench;
- messages: authored conversation linked to Work when relevant;
- correlated question Messages and evidence that require Host or Human judgment.

Legacy direct-Wave-executor rows remain readable with a visible compatibility
label. They are not the default authoring path.

## Desktop Layout

Use the shared Workbench shell: product sidebar, readable center canvas, and
context rail.

```text
+----------------------+--------------------------------------+------------------+
| Product sidebar      | Mission header                       | Mission brief    |
|                      | status · owning Team · actions       | Needs You        |
| Active context tree  +--------------------------------------+ Owning Team      |
|                      | Mission context (Markdown)           | Selected Wave    |
|                      +--------------------------------------+ Runtime summary  |
|                      | Wave 1 · advanced (compact)          |                  |
|                      +--------------------------------------+                  |
|                      | Wave 2 · selected                     |                  |
|                      | full Markdown Host plan              |                  |
|                      | responsibility table                 |                  |
|                      | Works summary / carry-over / evidence|                  |
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

This table is explanatory. Work records remain ownership and state truth.

## Context Rail

Compose flexible compact modules:

1. **MissionBrief** — durable context excerpt, status, source, and closeout.
2. **NeedsYou** — unresolved question Messages, blockers, or review requests.
3. **MissionTeam** — the Mission's one immutable flat AgentTeam, member state,
   latest run, and open Team action. Render its complete TeamRun history without
   inventing a mutable or one-to-many Mission/Team relation. This is
   Mission-scoped, not nested under one historical Wave.
4. **SelectedWave** — revision, updated actor, judgment excerpt, carry-over,
   artifacts, and history action. When the Wave is advanced, show its immutable
   `as recorded at advance` Work citations separately from `current execution
   now`; each includes version and timestamp so live Works do not appear owned
   by historical Wave state.
5. **LegacyExecutor** — only for historical direct-executor data.

Cards are quiet structural containers, not a wall of elevated analytics tiles.

## Actions

- Create and edit Mission Markdown context.
- Link an existing Agent Team or create and link a new one.
- Open Team War Room or Member Focus from any linked member control.
- Create, edit, and inspect history for ordered Waves.
- **Update plan** edits the current Wave Markdown and appends a revision. It is
  for a small adjustment that stays inside the same judgment boundary.
- Advance the selected Wave with an explicit Host outcome even while unrelated
  members remain active.
- Create Wave N+1 and keep the same TeamRun, MemberRun, Works, and native
  sessions where the Host chooses carry-over.
- Close the Mission with an explicit outcome. Never archive/delete teams as a
  side effect.

Advance confirmation summarizes active carry-over but does not require it to
finish. Sensitive external actions still require their own Human Approval.
Use Advance, rather than Update plan, when the plan, member composition,
responsibility, risk, or decision boundary changes materially.

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
- Active carry-over: show current Work state and the Wave judgment that cited
  it, without
  moving runtime ownership into the selected Wave.
- Missing native session: retain Harness coordination and label native detail
  unavailable; never invent transcript content.
- Offline/stale: preserve the last projection with timestamp and scoped retry.
- Historical legacy row: readable, explicit compatibility label, no new legacy
  authoring controls.

## Screenshot And UX Acceptance

At desktop acceptance the first viewport must show the Mission heading/context
start, the owning Team, ordered Waves, one selected Wave heading/context start,
responsibility table, and an available Host advance decision. Long Markdown is
not required to fit in one viewport; it must be reachable without clipping.
Pre-Works baselines do not prove ADR 0050. New expected/actual cases must also
separate historical Wave snapshots from live Works. Test:

- vertical scrolling to the end of long Mission/Wave context;
- every Team/Member control navigates and returns correctly;
- Markdown tables and long text do not overflow;
- active member work survives Wave advance in the projection;
- loading, empty, error, carry-over, and closeout states;
- actual screenshot against the approved expected reference.

## Explicit Boundaries

- Wave stores Host plan and judgment, not task/runtime ownership.
- `source_plan_ref` is navigation metadata, not a lifecycle edge.
- TeamRun completion does not advance a Wave; Wave advance does not complete a
  TeamRun.
- Agent Team and Agent Membership pages may share UI primitives but not identity
  or lifecycle semantics.
