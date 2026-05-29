# Docs Context Page Spec

```text
status: planned
owner_role: product-design
canonical_for: mounted project docs context
route_or_surface: /docs and inspector Docs tab
```

## Purpose

Primary user question: which canonical doc explains the active Vision, Goal,
Task, AgentMember, Evidence, or Decision?

Why it exists: operators need source-linked context without leaving the
Workbench. Docs are mounted context, not copied product truth.

Non-goals:

- do not paste full docs into object pages;
- do not make docs a separate knowledge base detached from workflow;
- do not treat missing docs context as silent absence.

## Objects And Proof

Canonical objects:

- docs registry entries;
- active Vision/Goal/Task/Team/Member;
- Evidence;
- Decision;
- ADR links;
- warnings for missing or broken context.

Workflow proof:

- docs are linked by object reason;
- docs show owner/status/lifecycle/path;
- missing docs context creates a knowledge-routing warning;
- object pages keep compact related-doc blocks.

Source docs:

- [../../README.md](../../README.md)
- [../../registry.json](../../registry.json)
- [../read-model.md](../read-model.md)

Read-model inputs:

- `docsContext(snapshot, objectRef)`;
- registry lookup;
- related docs by object type;
- broken/missing link warnings.

## Page-Level Agent Loop

Designer options:

- inspector docs panel plus `/docs` route;
- external links only;
- full docs embedded inline.

Questioner challenges:

- Does docs context stay connected to active work?
- Does it avoid copying canonical truth?
- Are missing docs visible as work?

Reviewer decision: use inspector panel plus route. Borrow compact inline blocks
for Goal/Task docs.

Rejected options:

- external links only: too disconnected;
- full embedded docs: bloats pages and duplicates truth.

Borrowed ideas:

- compact related-doc hints near Goal/Task sections.

## Information Architecture

Selected IA:

```text
object context
  -> related docs list
  -> owner/status/lifecycle/path
  -> reason this doc matters
  -> missing docs warnings
  -> docs route for browsing/filtering
```

Primary actions: open doc link, filter docs by active object, copy path, open
related ADR.

Secondary actions: request doc follow-up when API supports it.

Empty/loading/error states:

- empty: no related docs found, show routing warning if expected;
- loading: preserve list region;
- error: registry/read failure.

Responsive requirements:

- desktop: inspector tab and docs route;
- tablet: drawer;
- mobile: Docs tab with object filter.

Links to hard layout specs: pending.

## Failure Modes

- docs disconnected from workflow;
- full docs pasted as canonical truth;
- missing docs hidden;
- docs route becomes another raw list.

## Screenshot Acceptance Questions

- Can the reviewer see why each doc is relevant to the selected object?
- Are docs source-linked and not copied?
- Does missing context show as a workflow gap?
