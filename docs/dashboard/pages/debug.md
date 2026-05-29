# Debug Page Spec

```text
status: planned
owner_role: frontend-architecture
canonical_for: raw snapshot, import/export, and low-level object inspection
route_or_surface: /debug and closed debug drawer
```

## Purpose

Primary user question: how can an operator inspect or load raw state without
turning the Workbench into a raw dashboard?

Why it exists: debug state is still necessary for local development, offline
snapshot review, and low-level investigation. It must remain secondary.

Non-goals:

- do not show raw JSON in the primary viewport;
- do not make snapshot paste the default experience;
- do not use debug state as proof that workflow is understandable.

## Objects And Proof

Canonical objects:

- DashboardSnapshot;
- raw goals, teams, members, tasks, messages, sessions, evidence, decisions;
- source labels;
- live/offline load state.

Workflow proof:

- debug is closed by default;
- debug route/drawer clearly labels source and live/offline state;
- loading pasted/file snapshots disables live polling;
- raw objects are available for diagnosis only.

Source docs:

- [../runbook.md](../runbook.md)
- [../acceptance.md](../acceptance.md)
- [../frontend-architecture.md](../frontend-architecture.md)

Read-model inputs:

- raw snapshot;
- source/loading state;
- API URL and errors.

## Page-Level Agent Loop

Designer options:

- collapsed drawer plus `/debug` route;
- raw route primary;
- modal-only console.

Questioner challenges:

- Does debug stay secondary?
- Can offline snapshots be loaded without confusing live state?
- Does the UI avoid raw-first drift?

Reviewer decision: use collapsed drawer and explicit `/debug` route.

Rejected options:

- raw route primary: repeats old dashboard failure;
- modal-only console: too cramped for real debugging.

Borrowed ideas:

- shareable debug route for deep inspection.

## Information Architecture

Selected IA:

```text
closed debug affordance
  -> source/live state
  -> snapshot import/export
  -> raw object lists
  -> copied CLI/API refs
```

Primary actions: paste/load snapshot, stop live polling, export snapshot, copy
API/CLI refs.

Secondary actions: filter raw object type, inspect raw record.

Empty/loading/error states:

- empty: no snapshot loaded, show safe load paths;
- loading: show source and preserve debug geometry;
- error: show parse/API error.

Responsive requirements:

- desktop: closed drawer plus route;
- tablet: overlay drawer/route;
- mobile: Debug tab, never default.

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | current route | debug affordance closed      |
+-----+--------------------------------------------------------------------------+
| app | primary Workbench route 936                         | inspector 400      |
| 64  |                                                                          |
|     | Debug drawer closed by default: 40px handle at lower right                |
|     |                                                                          |
+-----+--------------------------------------------------------------------------+
| optional /debug route, opened explicitly:                                      |
+-----+----------------------+-----------------------------------+---------------+
| app | source rail 248      | raw snapshot workspace 760        | record 400    |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | live/offline     | | | import/export 120             | | | selected  | |
|     | | API/source/error | | | paste/file/load/pause/export   | | | record    | |
|     | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | object filters   | | | raw object lists 520          | | | JSON view  | |
|     | | goals/tasks/etc  | | | grouped records, counts       | | | copied ref |
|     | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | CLI/API refs     | | | parse/API errors              | | | warnings  | |
|     | rail scroll          | workspace scroll                   | record scroll|
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- primary route keeps normal Workbench layout; debug handle `40px` wide or
  less;
- explicit `/debug` source rail `240px` to `260px`;
- debug workspace min `720px`;
- record inspector `380px` to `410px`;
- import/export block `112px` to `136px`;
- raw object list owns remaining height.

First viewport content:

- in primary routes, debug is closed and cannot dominate the viewport;
- `/debug` route shows source/live/offline state before raw objects;
- snapshot paste/file load, pause live polling, export, and parse/API errors;
- raw objects are grouped and filterable, never used as acceptance proof.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| normal routes: debug handle closed; Workbench route remains first |
+------------------------------------------------------------------+
| /debug explicit route                                             |
+-----+---------------------------------------+--------------------+
| app | raw snapshot workspace 548           | record 288         |
| 56  | +-----------------------------------+| +----------------+ |
|     | | source/live/offline + import      || | selected record| |
|     | +-----------------------------------+| | JSON/ref/error | |
|     | | object filters row                || +----------------+ |
|     | | raw object grouped lists          | record scroll      |
+-----+---------------------------------------+--------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| normal routes: no debug body visible |
+--------------------------------------+
| /debug explicit route                 |
+--------------------------------------+
| top 48: Debug | source/live/offline  |
+--------------------------------------+
| controls 96: import/export/pause     |
+--------------------------------------+
| tabs 52: Objects Record Errors Refs  |
+--------------------------------------+
| active tab 556                       |
| Objects: grouped raw records         |
| Record: selected JSON + copy refs    |
| Errors: parse/API/source failures    |
| Refs: CLI/API snippets               |
+--------------------------------------+
```

Scroll ownership:

- primary routes: debug drawer is closed and does not own scroll;
- desktop `/debug`: source rail, workspace, and record inspector scroll
  separately;
- tablet `/debug`: workspace and record inspector scroll separately;
- mobile `/debug`: only the active tab scrolls.

Screenshot acceptance:

- primary Workbench screenshots must not show raw JSON, snapshot textarea, or
  debug as first content;
- explicit `/debug` screenshots must show source/live/offline state before raw
  object records;
- offline snapshots must be clearly labeled so they cannot be mistaken for live
  provider/runtime state.

## Failure Modes

- raw JSON appears in first viewport;
- snapshot textarea always visible;
- debug source looks live when offline;
- debug page used as workflow acceptance proof.

## Screenshot Acceptance Questions

- Is debug closed by default in primary screenshots?
- Does debug have explicit source/live labeling?
- Can raw state be inspected without replacing the Workbench?
