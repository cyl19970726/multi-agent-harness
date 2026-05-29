# Agent Workbench Shell Hard Layout Spec v2

```text
spec_id: agent-workbench-shell-v2
status: deprecated
implements_page_specs:
  - docs/dashboard/pages/team-workspace.md
  - docs/dashboard/pages/agent-member-workbench.md
  - docs/dashboard/pages/goal-document.md
  - docs/dashboard/pages/task-document.md
  - docs/dashboard/pages/graph-kanban.md
  - docs/dashboard/pages/docs-context.md
  - docs/dashboard/pages/evidence-review-decision.md
  - docs/dashboard/pages/warnings-repair.md
  - docs/dashboard/pages/debug.md
source_of_truth_boundary:
  historical only. Does not own current shell geometry, first viewport, scroll
  ownership, responsive placement, screenshot matrix, page purpose, or canonical
  object meaning. Current layout contracts live in docs/dashboard/pages/*.md.
reviewer_decision: deprecated after page-local layout contracts moved into docs/dashboard/pages/*.md
```

This spec is retained only as historical context after the rejected PR #6
implementation. It is no longer the current implementation contract. Current
desktop/tablet/mobile ASCII diagrams, region dimensions, first-viewport
content, scroll ownership, and screenshot acceptance live inside each page spec
under `docs/dashboard/pages/<page>.md`.

## Desktop Wireframe

Viewport: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: product | source/live | active vision/goal | search | command | debug   |
+-----+------------------+--------------------------------------+----------------+
| app | team rail 280    | workspace 704                        | inspector 380  |
| 64  | team switcher    | +----------------------------------+ | selected       |
|     | role groups      | | goal strip 80                    | | member/task    |
|     | member rows      | +----------------------------------+ | docs/warnings   |
|     | inbox/outbox     | | team activity 360                | | evidence       |
|     | queue pressure   | | - message/work rows              | | decision       |
|     | current work     | | - current task context           | |                |
|     |                  | | - decision pressure              | |                |
|     |                  | +----------------------------------+ |                |
|     |                  | | work context 300                 | |                |
|     |                  | | Goal/Task doc preview + lanes    | |                |
|     |                  | +----------------------------------+ |                |
+-----+------------------+--------------------------------------+----------------+
| debug drawer closed by default; opens as overlay and never pushes workspace     |
+--------------------------------------------------------------------------------+
```

Fixed dimensions:

- top bar: `56px`;
- app rail: `64px`;
- team rail: `280px`;
- inspector: `380px`, collapsible to `52px`;
- workspace minimum: `640px`;
- goal strip: `80px`;
- team activity region first viewport target: `340px` to `380px`;
- work context region first viewport target: `260px` to `320px`.

First viewport content:

- app rail shows Workbench navigation, not metrics;
- team rail shows role groups, member rows, inbox/outbox pressure, current work;
- workspace starts with active Vision/Goal strip, then team activity, then
  Goal/Task work context;
- inspector defaults to selected AgentMember workbench summary, not raw data;
- debug is closed.

Scroll containers:

- body has no horizontal scroll;
- team rail scrolls internally;
- workspace scrolls only below the top/goal strip;
- inspector scrolls internally;
- graph focus owns pan/zoom only inside its region;
- debug drawer scrolls internally only when opened.

## Tablet Wireframe

Viewport: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: product | live | vision/goal | search | debug            |
+-----+---------------------------------------+--------------------+
| app | workspace 548                         | inspector 288      |
| 56  | +-----------------------------------+ | selected member    |
|     | | goal strip                        | | or warning/docs    |
|     | +-----------------------------------+ |                    |
|     | | team activity and current work    | |                    |
|     | | work context tabs                 | |                    |
|     | | Kanban default, graph focus link  | |                    |
+-----+---------------------------------------+--------------------+
| team rail drawer closed; opens over left side                       |
+------------------------------------------------------------------+
```

Collapsed regions:

- team rail becomes drawer;
- inspector remains visible at `900px`, collapses below `820px`;
- graph uses segmented Graph/Kanban control and defaults to Kanban/list.

## Mobile Wireframe

Viewport: `390x844`.

```text
+--------------------------------------+
| top 48: source/live | search | debug |
+--------------------------------------+
| vision/goal strip 72                 |
+--------------------------------------+
| tabs 52: Team Work Member Warn Docs  |
|          Debug                       |
+--------------------------------------+
| active tab 672                       |
| Team: roster + queues + decisions    |
| Work: Goal/Task doc + Kanban         |
| Member: activity + inbox/outbox      |
| Warn: affected object + repair       |
| Docs: related docs                   |
+--------------------------------------+
```

Tab order:

1. Team
2. Work
3. Member
4. Warnings
5. Docs
6. Debug

Hidden or deferred regions:

- app rail becomes tab bar;
- team rail becomes Team tab content;
- inspector becomes Member/Warn/Docs tabs;
- graph opens as focus mode with list fallback, never default.

## State Matrix

| State | Required UI | Rejected UI |
| --- | --- | --- |
| Empty | Explain missing Team/Goal/Member and provide safe source/debug path. | Blank page or fake sample data. |
| Loading | Preserve shell geometry with skeleton rows. | Layout shift that changes columns/tabs. |
| Loaded | Team, Member, Work, Docs, Warnings, and Decision context visible. | Status cards without workflow proof. |
| Warning | Local object callout plus global queue item. | Detached toast or color-only badge. |
| Error | Source/API error with retry and last-good source state. | Raw JSON dump as fallback. |

## Forbidden Primary Surfaces

- metrics wall;
- card dump;
- raw JSON or always-visible snapshot textarea;
- graph-first Team route;
- roster-only Team view;
- side-card-only AgentMember view;
- tab collection with no visible workflow hierarchy;
- long stacked mobile report.

## Screenshot Acceptance

Required viewport screenshots:

- desktop `1440x1000`;
- tablet `900x1180`;
- mobile `390x844`.

For each screenshot, reviewer must answer:

```text
first_impression:
workbench_or_dashboard:
matched_ascii_spec:
team_as_collaboration_space:
agent_member_as_workspace:
goal_task_docs_connected:
debug_secondary:
decision: pass | fix | reject
```

## Rejected When

Reject immediately when:

- the screenshot first impression is dashboard, report, card dump, or raw
  object viewer;
- Team is only a roster;
- AgentMember is only a card or side panel;
- Goal/Task is only a status list;
- Docs, Evidence, Decision, and Warnings are disconnected tabs;
- mobile collapses into a long report;
- old dashboard components define the first viewport.
