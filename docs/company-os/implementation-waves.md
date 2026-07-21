# Company OS V1 implementation waves

```text
status: engineering and implementation visual gate passed; human visual approval pending
owner_role: product and architecture lead
canonical_for: Company OS V1 sequencing, gates, and completion evidence
```

This plan turns the Company OS product direction into seven ordered Waves.
Parallel work is allowed when owned paths and contracts are independent, but a
later Wave cannot be accepted before the gates it depends on have passed.

## Current prioritization after V2.2

The historical seven-Wave acceptance remains valid for what it proved. The
next product slice does not extend every Agent focus page. It prioritizes:

1. canonical Docs/Organization/Work/Finance responsibility and governance-led
   organization contracts;
2. Organization Overview with compact Actor configuration for responsibility,
   prompt, tools/Skills, permissions, maintained Docs, and linked WorkItems;
3. native Milestone, WorkType, business-line relations, and the multi-view Work
   Operating System — implemented; and
4. Current → Expected → Actual responsive evidence for the Work candidate —
   captured for desktop, tablet, and mobile across all six Work views.

Dedicated Governance or Business Agent workspaces are deferred visual
references. Existing Agent Focus implementation remains valid evidence but is
not the immediate expansion target.

## Completion status

All seven engineering Waves passed their automated and independent critical
review gates on 2026-07-20. The remaining decision is deliberately Human-only:
approval of the generated Expected visual references. Until that decision is
recorded, the implementation is accepted but the visual design contract remains
`pending`.

- permanent frozen-history archive: `~/.harness/archives/multi-agent-harness/legacy-goal-task-v1/2026-07-20T-final-frozen/`;
- archive manifest SHA-256: `f3558302ce7a7b3ae2813d296f5dabc6e2b4be72bb62c00b9fa6d7fe37141e5f`;
- real Store-backed capture: `.visual-evidence/company-os-v1/company-os-v1-live-acceptance/capture-run.json`;
- durable Actual images: [`../design/company-os-v1/actual/`](../design/company-os-v1/actual/);
- three-way comparison: [`../design/company-os-v1/expected-vs-actual.html`](../design/company-os-v1/expected-vs-actual.html);
- acceptance record: [`../design/company-os-v1/implementation-acceptance.md`](../design/company-os-v1/implementation-acceptance.md).

## Shared acceptance scenario

Every Wave uses the same deterministic scenario:

- source document: **Trademark application CN-2026-018**;
- business work: **Trademark filing for Brand A**;
- accountable owner and approver: **Brand Owner · Human**;
- submitted and assigned actor: **Trademark Agent · Standing Agent**;
- finance reviewer: **Finance Agent · Standing Agent**;
- contributor and legal reviewer: **External Lawyer · External**;
- financial state: **Trademark filing fee · Commitment · ¥3,000 · Pending
  approval**;
- there is no Payment or settlement evidence.

The authoritative visual fixture is
[`../design/company-os-v1/fixtures/company-os-trademark-v1.json`](../design/company-os-v1/fixtures/company-os-trademark-v1.json).

## Wave 1 — product contract and visual truth

Deliver:

- canonical PRD, architecture, object boundaries, page hierarchy, and safety
  rules;
- one shared fixture for all core pages;
- twelve expected page images generated from that fixture;
- expected, candidate, current, implemented, and comparison artifacts kept in
  distinct locations;
- superseded product documentation removed from default reading paths.

Gate:

- the twelve images agree on IDs, dates, ownership, approval, and finance;
- no image invents presence, capacity, payment, settlement, or approval;
- expected images remain pending until a human explicitly approves them;
- a critical review finds no P0 product or truth contradiction.

## Wave 2 — verified retirement of the superseded stack

Deliver:

- a versioned, read-only exporter for historical ledgers and linked evidence;
- hashes, line counts, latest projections, edge closure, and archive verifier;
- explicit freeze of old creation paths before type or ledger deletion;
- verified export of every configured project;
- removal of old default navigation, APIs, schemas, runtime types, examples,
  and instructions after export.

Gate:

- no historical byte is deleted before its archive verifies;
- no historical execution record is coerced into a Company OS WorkItem;
- fresh projects expose only the current Company OS and Mission/Wave model;
- active product search and navigation contain no second coordination model.

## Wave 3 — semantic substrate

Deliver:

- Document, Block, TypedRecord, Relation, View, and BusinessModule contracts;
- Human, StandingAgent, External, and Service actor lifecycles;
- WorkItem, Assignment, Approval, Commitment, Payment, and evidence relations;
- custom-page definitions, packages, scoped queries, governed Actions, and
  fallback contracts;
- schemas, validation, persistence, APIs, and read projections.

Gate:

- invalid responsibility, approval, and finance transitions are rejected;
- a Commitment cannot silently create a Payment;
- Human and Standing Agent identities cannot be confused with run-scoped
  execution members;
- every custom page can fall back to standard document and view rendering.

## Wave 4 — basic Docs and standard views

Deliver:

- ordinary rich documents with paragraphs, headings, lists, callouts, links,
  embeds, and basic tables;
- typed records and relation chips embedded inside documents;
- standard table, board, timeline, and detail views;
- source/result document links and governed document updates;
- responsive Docs workspace, document focus, and standard fallback pages.

Gate:

- a useful page can be built without custom code;
- the same record remains one source of truth across every view;
- keyboard, responsive, empty, loading, permission-denied, and error states are
  usable;
- document actions are audited and cannot bypass approval.

## Wave 5 — mixed organization and company operations

Deliver:

- organization map and compact Human/Agent configuration surfaces;
- Work Overview, Board, All Work, Milestones, Timeline, Workload, and WorkItem
  focus with requester, submitter, assignee, owner,
  reviewer, evidence, and result provenance;
- Approval, Finance, Governance Proposal, and Business Module focus pages;
- reusable compact controls for documents, actors, work, approvals, finance,
  Agent Teams, and Waves.

Gate:

- unknown actor state stays unknown and is never rendered as online or
  available;
- Human pages contain no provider or runtime telemetry;
- Agent configuration does not inherit run-scoped Member semantics;
- the pending ¥3,000 commitment and its human gate remain consistent on every
  page.

## Wave 6 — governed agent-programmable pages

Deliver:

- a module-design capability that proposes records, relations, views,
  permissions, finance links, and approval policy;
- a page-builder capability that generates a reviewable page package from an
  approved design;
- scoped query access, declared Action Commands, policy evaluation, audit
  metadata, package versioning, preview, rollback, and standard-view fallback;
- no arbitrary server-side code execution or direct ledger mutation.

Gate:

- undeclared queries and actions are denied;
- sensitive actions stop at a named human approval boundary;
- a broken or removed package opens the same records through standard views;
- generated code and its permissions can be inspected before activation.

## Wave 7 — end-to-end company loop and visual acceptance

Deliver:

- one deterministic flow from source document to WorkItem, Assignment, agent
  submission, human Approval, pending finance Commitment, and result writeback;
- current, expected, implemented, and comparison images for all twelve core
  pages;
- desktop coverage for all pages and tablet/mobile coverage for critical focus
  pages;
- automated truth, console, overflow, accessibility, and regression checks;
- an independent critical review and explicit human visual approval record.

Gate:

- the complete loop is reconstructable from durable records and links;
- visual evidence uses the same fixture and viewport contract;
- no Payment or settlement is shown before a later authorized action records
  it;
- all P0 findings are closed and deviations are documented rather than hidden.

## Definition of complete

Company OS V1 is complete only when all seven Wave gates pass. A generated
image is not an implementation, a browser screenshot is not proof of business
truth by itself, and a successful agent run is not a human approval. The final
handoff must link the contracts, code, fixtures, tests, expected images, actual
captures, comparisons, review findings, and remaining P1 work.
