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

Links to hard layout specs: pending.

## Failure Modes

- raw JSON appears in first viewport;
- snapshot textarea always visible;
- debug source looks live when offline;
- debug page used as workflow acceptance proof.

## Screenshot Acceptance Questions

- Is debug closed by default in primary screenshots?
- Does debug have explicit source/live labeling?
- Can raw state be inspected without replacing the Workbench?
