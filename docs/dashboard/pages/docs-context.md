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

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | docs context | active object | search | dbg  |
+-----+----------------------+-----------------------------------+---------------+
| app | docs filter 248      | docs context route 760            | object 400    |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | active object    | | | related docs header 80       | | | selected  | |
|     | | type/id/status   | | | reason, owner, lifecycle      | | | object    | |
|     | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | filters          | | | required docs list 260       | | | why doc   | |
|     | | object type      | | | PRD, concept, workflow, ADR   | | | matters   | |
|     | | lifecycle/status | | +-------------------------------+ | +-----------+ |
|     | +------------------+ | | warnings/missing context 160 | | | broken    | |
|     | | registry health  | | +-------------------------------+ | | links     | |
|     | | broken/missing   | | | browse registry 300          | | +-----------+ |
|     | +------------------+ | | path, owner, canonical_for    | | | actions   | |
|     | rail scroll          | route scroll                       | inspector scr |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- docs filter rail `240px` to `260px`;
- docs route min `720px`;
- object inspector `380px` to `410px`;
- related docs header `72px` to `88px`;
- required docs list target `240px` to `300px`;
- missing-context block target `140px` to `180px`.

First viewport content:

- active object identity and why docs are being filtered;
- related docs with owner, lifecycle, path, and relevance reason;
- missing or broken context warnings as work, not absence;
- registry browse list for the selected filter;
- selected object context without embedding full docs inline.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | docs context | active object | search | dbg   |
+-----+---------------------------------------+--------------------+
| app | docs route 548                       | object panel 288   |
| 56  | +-----------------------------------+| +----------------+ |
|     | | active object + reason            || | object summary  | |
|     | +-----------------------------------+| | selected doc    | |
|     | | filter chips 48                   || | missing links   | |
|     | +-----------------------------------+| +----------------+ |
|     | | related docs list                 | object scroll      |
|     | | missing context                   |                    |
|     | | registry browse                   |                    |
+-----+---------------------------------------+--------------------+
| filter rail becomes drawer; docs never become copied canonical text         |
+------------------------------------------------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: Docs | live/source | debug   |
+--------------------------------------+
| object 88: type/id + reason          |
+--------------------------------------+
| tabs 52: Related Missing Browse Obj  |
+--------------------------------------+
| active tab 604                       |
| Related: doc rows with reason/path   |
| Missing: routing warnings            |
| Browse: registry filter/list         |
| Obj: selected object + doc actions   |
+--------------------------------------+
```

Scroll ownership:

- desktop: filter rail, docs route, and object inspector scroll separately;
- tablet: docs route and object panel scroll separately;
- mobile: only the active tab scrolls.

Screenshot acceptance:

- every visible doc row must explain relevance to the active object;
- missing docs context must be visible as a workflow gap;
- full docs content must not be pasted into object pages as canonical truth;
- docs must feel mounted into work, not a separate knowledge-base list.

## Failure Modes

- docs disconnected from workflow;
- full docs pasted as canonical truth;
- missing docs hidden;
- docs route becomes another raw list.

## Screenshot Acceptance Questions

- Can the reviewer see why each doc is relevant to the selected object?
- Are docs source-linked and not copied?
- Does missing context show as a workflow gap?
