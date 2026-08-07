---
name: company-docs-operator
description: Operate Company OS Docs through governed CLI and Action contracts. Use when a Governance Agent or business Agent needs to audit document/page architecture, define page contracts, author pages through the v2 page surface, create typed records, create views, link relations, or prepare a module/page-definition operation while preserving Docs/Work/Org/Finance truth boundaries.
---

# Company Docs Operator

Operate the Company OS Docs surface. This skill is a procedural capability, not
product authority; it helps Agents choose governed CLI commands and verify records.

Docs are Agent-operated and Human-reviewed. CLI/API is the primary Agent
interface; UI is Human review context, not the authoritative machine interface.

## Load the contracts

Before writing or proposing a durable Docs change, read:

- `docs/current/company-os/document-system.md`
- `docs/current/company-os/skill-contracts.md`
- `docs/current/company-os/implementation-truth-matrix.md`
- `docs/current/company-os/governance.md`

When the change touches a recurring business domain or custom page, also read:

- `docs/current/company-os/module-design.md`
- `docs/current/company-os/agent-programmable-pages.md`
- `docs/current/company-os/frontend-information-architecture.md`

When observing GitHub repositories, Issues, PRs, checks, or reviews, use
`$connect-github-company-os` for the external-system boundary. This Skill owns
only the Docs records, snapshots, and Relations used by that connector.

Do not use this skill to override those contracts. If a repository document,
schema, API, or acceptance check conflicts with this skill, the canonical
contract wins.

## Load focused references when needed

- Read `references/page-contract.md` before creating or changing a
  business-critical page.
- Read `references/business-page-archetypes.md` when shaping a commercial
  project, operating module, or multi-page document space.
- Read `references/store-authoring-patterns.md` before writing page contracts,
  page content, TypedRecords, Relations, or Views into a real Store.
- Read `references/anti-patterns.md` before final handoff for a commercial
  Docs change or when the result risks becoming generic prose.

## Operating rule

Docs own company memory, document structure, typed records, relations, views,
and module entrypoints. Docs may reference Work, Organization, Finance, and
Execution records, but this skill must not mutate those systems unless the
called command explicitly does so through that system's governed Action.

In practice:

- `Document`, `Block`, `TypedRecord`, `Relation`, `View`, and `BusinessModule`
  are Docs-owned objects.
- `WorkItem`, `Assignment`, and `Approval` remain Work-owned objects.
- `HumanMember`, `AgentMember`, `OrgUnit`, role, permission, and reporting
  changes remain Organization-owned objects.
- `Commitment`, `Payment`, invoice, refund, and monetary metrics remain
  Finance-owned objects.
- `Mission`, `Wave`, provider runs, workflow runs, and Agent Team runs remain
  execution truth.

Never infer approval, payment, settlement, organization authority, or executor
lifecycle from a document update.

## Command selection

Use the smallest command that preserves the source of truth. Current commands:
`harness company docs query`, `harness company docs search`,
`harness company docs traverse`, `harness company docs refs`,
`harness company docs related`, `harness company docs health`,
`harness company docs source sync`, `harness company docs module create`,
`harness company docs page create`, `harness company docs page read`,
`harness company docs page write`, `harness company docs page append`,
`harness company docs page search`, `harness company docs page rename`,
`harness company docs page move`, `harness company docs page archive`,
`harness company docs page scaffold`, `harness company docs page verify`,
`harness company docs page publish`,
`harness company docs page-definition create`,
`harness company docs typed-record append`,
`harness company docs typed-record update`,
`harness company docs typed-record validate`,
`harness company docs view create`, `harness company docs view update`,
`harness company docs relation link`, `harness company docs relation unlink`,
`harness company docs relation relink`,
`harness company docs relation repair-missing`, `harness company docs diff`,
`harness company docs snapshot`, and
`harness company docs change-report`.

Module and page-definition creation are administrative governance operations
and require a Human with `company_os.admin`. Typed-record, view, and relation
writes require a matching `CustomPageDefinition` policy and the normal Company
OS write capability. Page create/read/write/append/search/rename/move/archive
are the v2 page surface and write through the revision mechanism; they need no
PageDefinition policy bundle.

The Block-era command tree (`document create/rename/move/archive`,
`template create/status`, `block append/update/archive/remove/reorder`) and the
`document.append`/`block.append` API actions were retired in stage R3 of the
AI-first Docs retirement plan. Do not script against them; legacy ledger
documents remain readable through `page read` as honest legacy projections.

## Safe workflow

1. Inspect current truth through CLI/API. Use `harness company docs query` as
   the first read command for one Document or module operating context, then use
   `harness company docs health` for broader structural audit. Prefer native
   Store projection reads over fixture or mock data.
2. For any business-critical page, define the page contract before mutation:
   primary question, intended audience, required sections, source
   TypedRecords, relation panels, standard Views, sibling navigation, Work /
   Org / Finance boundaries, and whether a custom page is justified. Do not
   append generic prose until this shape is explicit.
3. Identify the owning object and actor. A write must name the source Document,
   target module/record when applicable, and the accountable Human or Agent.
4. Choose the command. Use standard Docs commands before proposing custom code.
5. Prepare idempotent, durable content. Do not include private reasoning,
   secrets, raw transcripts, or policy claims that the records cannot prove.
6. Run the command through the governed CLI/API path. Do not append ledgers
   directly.
7. Verify the result. Confirm the expected native row exists and unrelated
   ledgers did not change.
8. Use UI only for Human review and supplemental visible evidence. A UI-only
   change is not sufficient proof of a Docs capability.
9. Report evidence and remaining gaps. Distinguish `verified`, `partial`,
   `planned`, and `design-only`.

## Root Company hierarchy and archive boundary

Operate one selected Company Store as the durable company boundary. The root
Company Docs tree is its navigation and memory hierarchy; it is not a new
`Project` record and must not be inferred from a repository or Execution
Space.

Keep the active hierarchy shallow and responsibility-shaped:

```text
Company Home
  -> Governance and company-wide operating areas
  -> Domain / business-line homes owned by Domain Leads
  -> Current procedures, source records, decisions, and accepted results
```

The active tree should answer what the Company is doing now, who owns each
area, what work is open, and where results return. Human requests enter as
durable source context: the Human Principal / Constitution Owner remains the
requester, the Supervisor may faithfully capture and route provenance without
creating Company authority, and the Company Lead decides whether to promote
the request into Docs or Work. Do not use an inbox block, meeting note, or chat
transcript as a shadow task queue.

When a Document describes the Company Constitution or a delegation envelope,
label current versus target truth explicitly. A draft or mutable Document is
policy context, not authority by itself. Do not call it active/canonical until
its exact version/digest, supersession relation, authority-bearing subject
(for example a native `ScopedPermissionGrant` or governed fallback
`AuthorityConstitution` TypedRecord), required Approval/Action evidence, and
Store readback all exist. Recursive attenuating delegation remains target-only
until schemas, Actions, authenticated transport, and acceptance prove it.

Archive a Document or subtree when it is superseded, closed, duplicated, or no
longer part of current operating context. Before archival:

1. query and traverse the candidate subtree;
2. inspect refs, related records, open WorkItems, maintained-document owners,
   and active module/page entrypoints;
3. move any still-current fact or accepted result to the active owner through
   an explicit update or relation;
4. preview with `page archive` (dry-run without `--confirm`), obtain required
   review, then confirm; and
5. verify the archived content is absent from active navigation while its ids,
   relations, evidence, and audit history remain resolvable.

Archival is a lifecycle projection, not deletion. Never copy the whole old tree
under an "Archive" page merely to hide it, never archive the only source/result
for open Work, and never mark stale content active because it still exists in
the append-only Store. A successor Document must link and supersede the prior
version without erasing the old constitution, Approval, result, or evidence
chain.

## Business page quality gate

A page is not acceptable merely because it has text blocks. Before and after
editing a real commercial project page, check:

- Does the page answer one clear operating question?
- Does it show where the reader goes next through the document tree or related
  records?
- Are stable facts modeled as `TypedRecord`s and `Relation`s rather than copied
  only into prose?
- Are WorkItems, Assignments, Approvals, Finance records, and Organization
  actors referenced through their owning systems instead of implied by text?
- Are module boundaries explicit so an Agent knows which CLI/skill to use next?
- If the page is a core surface, is there a standard View fallback and a custom
  page candidate only when the standard composition is not enough?
- If a custom page is planned, is its visual/front-end shape recorded as a page
  contract and handed to `$company-page-builder` rather than embedded as
  ungoverned HTML in Docs?

If the answer is no, stop and produce the page contract or module design before
writing more content.

## Query before mutation

Use the read-only query command before deciding where or how to write:

```bash
harness company docs query --document <document-id>
harness company docs query --module <business-module-id>
harness company docs search --query "商标" --module <business-module-id>
harness company docs traverse --document <document-id> --depth 2
harness company docs refs --document <document-id>
harness company docs related --record <typed-record-id>
```

The response is the Agent-facing operating context over the latest projection:
selected/root Document, ordered Blocks, children, templates, source-linked
TypedRecords, Relations, Views, module/page policy context, health findings,
available commands, and explicit boundaries.

`docs query` does not create WorkItems, Approvals, Finance records,
Organization changes, execution runs, or UI-only state. The canonical write
store remains append-only JSONL ledgers plus latest projections. SQL is a
future derived read/query/index layer that must serve the same contract without
becoming write authority.

`search`, `traverse`, `refs`, and `related` are read-only projection commands
for finding context without scraping UI. They do not infer approval, payment,
authority, or execution state.

## External software product sources

Use `source sync` when a Company OS Docs module needs to observe PRDs, ADRs,
architecture docs, or design contracts from an external Git worktree such as a
software product repository:

```bash
harness company docs source sync \
  --definition <custom-page-definition-id> \
  --module <business-module-id> \
  --source-document <document-id> \
  --actor <agent-or-human-id> \
  --repo-path <local-git-worktree> \
  --repo <owner/repo> \
  --branch <branch> \
  --path docs/current/product/prd \
  --path docs/current/architecture/architecture \
  --dry-run
```

The command writes `TypedRecord` rows for `external_project`,
`product_doc_source`, `product_doc_snapshot`, and `source_sync_run`, and links
each row to its source Document with an idempotent `source_for` Relation. In v0
this is the native Docs substrate for external PRD mapping; later dedicated
schema or SQL read models must remain rebuildable from these Company OS records.

`source sync` observes software product truth. It does not overwrite Company OS
commercial truth, create WorkItems, approve spending, update Finance, mutate
Organization, execute GitHub actions, or treat a GitHub webhook as authority.
When synced sources drift materially, create or route a separate WorkItem for
Docs Governance review through `$company-work-operator`. Issue/PR/check/review
sync and DeliveryRef reconciliation belong to `$connect-github-company-os`,
not to this Docs command.

## Code-declared custom pages

Core business pages should be code-declared pages over the Docs substrate, not
Human-assembled Notion pages. Use these commands for PageDefinition/PagePackage
metadata:

```bash
harness company docs page scaffold \
  --module <business-module-id> \
  --fallback-view <view-id> \
  --title "Trademark Console" \
  --authority <human-admin-id>

harness company docs page verify \
  --definition <custom-page-definition-id>

harness company docs page publish \
  --definition <custom-page-definition-id> \
  --version <semver> \
  --artifact-ref <source-or-build-artifact-path> \
  --authority <human-admin-id>
```

These commands do not generate business data and do not make a visual mock an
implemented product claim. Current `page publish` records candidate package
metadata only; it does not switch the active definition package pointer. A
custom page may be beautiful and purpose-built, but it remains presentation
over native Documents, TypedRecords, Relations, Views, WorkItems, Approvals,
FinancialRecords, and Actors.

## Page authoring and structure (v2 page surface)

Page-level authoring uses the v2 page surface. Content is authored as Markdown;
the store maps it to the page revision mechanism, so every write is an explicit
versioned change instead of an in-place mutation.

```bash
harness company docs page create \
  --title <title> \
  --actor <human-or-agent-id> \
  [--markdown <text> | --markdown-file <path>] \
  [--id <document-id>] [--space <document-space-id>] [--parent <document-id>]

harness company docs page read --doc <document-id> \
  [--scope outline|section|range|keyword] [--detail simple|with-ids|full] \
  [--revision <n|-1>] [--format json|markdown]

harness company docs page write --doc <document-id> \
  --expected-revision <n> \
  (--markdown <text> | --markdown-file <path>) \
  [--title <title>] [--summary <change-summary>]

harness company docs page append --doc <document-id> \
  (--markdown <text> | --markdown-file <path>) \
  [--after <block-id|-1|end|heading:text>] [--expected-revision <n>] \
  [--summary <change-summary>]

harness company docs page search --keyword <text> [--limit <n>]
```

Structure maintenance goes through the same revision mechanism:

```bash
harness company docs page rename --doc <document-id> --title <new-title> \
  [--expected-revision <n>]

harness company docs page move --doc <document-id> \
  --parent <new-parent-document-id|-1|root> [--expected-revision <n>]

harness company docs page archive --doc <document-id> [--expected-revision <n>]
```

Boundary rules:

- `page write` requires `--expected-revision`; a stale revision is rejected
  instead of silently overwriting newer work. Read the current revision first.
- `page rename`/`page move` record metadata revisions; move rejects parent
  cycles. `page archive` requires `--confirm` to commit; without it the command
  returns a dry-run preview and writes nothing.
- A top-level operating area or DocumentSpace root is created with
  `page create` without `--parent` (plus `--space` when the store has several
  spaces). This replaces the retired Block-era root bootstrap; it still creates
  only the page, not a BusinessModule, PageDefinition, WorkItem, Relation,
  Finance row, Organization row, or source sync record.
- Page commands do not create WorkItems, Approvals, Finance records,
  Organization changes, or execution records, and they never physically delete
  pages or revisions.

Legacy Block-era documents remain readable: `page read` projects them
read-only (`legacy_projection=true`) with best-effort block-kind mapping. To
evolve a legacy document, rewrite it through `page write` rather than editing
Block rows directly.

## Typed records and relations

Use TypedRecord and Relation commands for structured business truth. Do not
hide structured changes inside prose Blocks.

Commands: `harness company docs typed-record append`,
`harness company docs typed-record update`,
`harness company docs relation link`,
`harness company docs relation unlink`,
`harness company docs relation relink`,
`harness company docs relation repair-missing`, and
`harness company docs typed-record validate`.

`typed-record append` creates a source-linked TypedRecord under a module;
`typed-record update` dispatches a governed `typed_record.append` update for an
existing record. Update may change title, fields, and lifecycle status; it must
not change record id, module, record type, source Document, creator, or created
time. `--merge-fields` overlays the supplied JSON object on existing fields;
without it, `--fields-json` replaces the full fields object.

`relation link` creates an active Relation through a governed `relation.append`
Action. `relation unlink` dispatches a governed `relation.append` update that
marks the latest Relation row `lifecycle_status=archived`. It does not
physically delete the Relation or alter endpoints, type, provenance, creator,
or created time. Unlink requires `--confirm` unless it is a dry-run. Active
Docs query and health checks ignore archived Relations, so unlinking a required
Document ↔ TypedRecord relation may surface a missing-relation finding until a
new active relation is linked.

`relation relink` is a dry-run-first cleanup helper. A confirmed relink is two
governed `relation.append` Actions: archive the existing Relation latest row,
then create a replacement active Relation. It never physically deletes relation
history.

`relation repair-missing` inspects one CustomPageDefinition's module and plans
only missing active `Document → source_for → TypedRecord` Relations for records
whose source Document still exists. Use `--dry-run` first; dispatch requires
`--confirm`. Repeating it after a successful repair plans zero writes. It does
not repair unrelated modules or create Work, Finance, Organization, Approval,
or Execution state.

`typed-record validate` is read-only. It checks an explicit schema JSON against
the current `TypedRecord.fields` for required fields and simple field types.
Persistent module field-schema governance remains a later object-model slice.

If a template-like pattern should correspond to a TypedRecord type, declare the
module policy with `harness company docs module create --relation-rule-json
'{"relation_type":"source_for","from_kind":"document","to_kind":"typed_record","required":true,"cross_module":false}'`,
then create/link the concrete TypedRecord through `harness company docs
typed-record append` and `harness company docs relation link`.

## Legacy template Documents

Block-era template authoring (`template create`, `template status`, template
instantiation during document create) was retired with the R3 deletion of the
Block-era command tree. Existing `Document(kind=template)` rows remain readable
legacy records: query/health still report them, and their lifecycle state is
historical data, not an authoring surface. New reusable patterns are authored
as ordinary v2 pages plus TypedRecords/Relations, not as template Documents.
A v2 template mechanism returns only when a real v2 template need exists.

## Standard view configuration

`harness company docs view create` creates a native `View` record. Use it for
saved presentation over existing module records, not as a second data store.

```bash
harness company docs view create \
  --definition <custom-page-definition-id> \
  --module <business-module-id> \
  --title "Trademark filing board" \
  --mode board \
  --source-kind typed_record \
  --query-json '{"filters":[{"field":"record_type","value":"trademark_application"}],"group_by":"lifecycle_status","sort_by":"updated_at"}' \
  --actor <human-or-agent-id>

harness company docs view update \
  --definition <custom-page-definition-id> \
  --view <view-id> \
  --query-json '{"group_by":"lifecycle_status"}' \
  --actor <human-or-agent-id> \
  --dry-run
```

The first supported configuration slice is table/board/timeline mode, source
kinds, simple filters, grouping, and sorting stored in `View.query`. Calendar,
chart, advanced field layout, and inline saved-view editing remain planned
until their own UI and acceptance evidence exist.
`view update` writes a latest `view.append` row for presentation/query config
only. It must not mutate TypedRecords or create a second source of truth.

## Review evidence

Use review commands before risky cleanup:

```bash
harness company docs snapshot --document <document-id>
harness company docs diff --document <document-id> --proposed-json <json>
harness company docs change-report --action-json <action-command-json>
```

These commands are report-only. They do not authorize, dispatch, approve,
rollback, or mutate company memory.

## When to escalate

Stop and request module design, human review, or a first-class Approval when
the requested operation would:

- add a new recurring business domain;
- create or change permission, reporting, role, or organization structure;
- spend money, approve a commitment, settle a payment, or change financial
  state;
- make a legal submission or external filing;
- delete, merge, split, rename, or migrate important company memory;
- require a custom page because standard documents and views are insufficient;
- require data or commands not declared by the page/module contract.

## Verification

Minimum checks after changing this skill or the Docs operating surface:

```bash
npx pnpm@9.15.4 check:company-os
npx pnpm@9.15.4 check:docs-v2-live
git diff --check
```

Use broader checks when code paths outside Docs changed:

```bash
npx pnpm@9.15.4 check:dashboard
```

Completion requires native evidence, not just a generated page or successful
mock fixture.
