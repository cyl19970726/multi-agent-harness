---
name: company-docs-operator
description: Operate Company OS Docs through governed CLI and Action contracts. Use when a Governance Agent or business Agent needs to audit document/page architecture, define page contracts, create child documents or templates, append structured blocks, create typed records, create views, link relations, or prepare a module/page-definition operation while preserving Docs/Work/Org/Finance truth boundaries.
---

# Company Docs Operator

Operate the Company OS Docs surface. This skill is a procedural capability, not
product authority; it helps Agents choose governed CLI commands and verify records.

Docs are Agent-operated and Human-reviewed. CLI/API is the primary Agent
interface; UI is Human review context, not the authoritative machine interface.

## Load the contracts

Before writing or proposing a durable Docs change, read:

- `docs/company-os/document-system.md`
- `docs/company-os/skill-contracts.md`
- `docs/company-os/implementation-truth-matrix.md`
- `docs/company-os/governance.md`

When the change touches a recurring business domain or custom page, also read:

- `docs/company-os/module-design.md`
- `docs/company-os/agent-programmable-pages.md`
- `docs/company-os/frontend-information-architecture.md`

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
  structured Blocks, TypedRecords, Relations, or Views into a real Store.
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
`harness company docs page scaffold`, `harness company docs page verify`,
`harness company docs page publish`,
`harness company docs page-definition create`,
`harness company docs document create`, `harness company docs document rename`,
`harness company docs document move`, `harness company docs document archive`,
`harness company docs template create`, `harness company docs template status`,
`harness company docs block append`, `harness company docs block update`,
`harness company docs block archive`, `harness company docs block remove`,
`harness company docs block reorder`,
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
and require a Human with `company_os.admin`. Ordinary document, block,
typed-record, view, and relation writes require a matching
`CustomPageDefinition` policy and the normal Company OS write capability.

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
writing more blocks.

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
  --path docs/prd \
  --path docs/architecture \
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

## Document structure maintenance

Use explicit structure commands instead of creating duplicate pages:

```bash
harness company docs document create \
  --root \
  --id <root-document-id> \
  --space <document-space-id> \
  --title <root-title> \
  --actor <human-or-agent-id> \
  --authority <human-admin-id>

harness company docs document create \
  --definition <custom-page-definition-id> \
  --parent-document <document-id> \
  --id <child-document-id> \
  --space <document-space-id> \
  --title <child-title> \
  --actor <human-or-agent-id>

harness company docs document rename \
  --definition <custom-page-definition-id> \
  --document <document-id> \
  --title <new-title> \
  --actor <human-or-agent-id> \
  --dry-run

harness company docs document move \
  --definition <custom-page-definition-id> \
  --document <document-id> \
  (--parent-document <new-parent-document-id> | --root) \
  --actor <human-or-agent-id> \
  --dry-run

harness company docs document archive \
  --definition <custom-page-definition-id> \
  --document <document-id> \
  --actor <human-or-agent-id> \
  --dry-run
```

`rename`, `move`, and `archive` all dispatch governed `document.append`
updates when `--dry-run` is omitted. Dry-run returns the proposed before/after
and Action body without dispatching. Archive requires `--confirm` unless it is a
dry-run. These commands must preserve `Document.id`, `space_id`, `kind`,
`created_by`, `created_at`, existing `block_ids`, and existing
`reference_refs`; move must not create a parent cycle. They do not create
WorkItems, Approvals, Finance records, Organization changes, or execution
records.

`document create --root` is a bootstrap escape hatch for a new DocumentSpace or
top-level operating area inside an existing Company Store. It requires a Human
admin `--authority` and appends only a root `Document`; it does not create a
BusinessModule, PageDefinition, WorkItem, Relation, Finance row, Organization
row, or source sync record. After creating the root, create the module, fallback
View, PageDefinition, and any TypedRecords/Relations through their own commands.

## Block content maintenance

Use explicit Block commands for content edits instead of replacing the whole
Document:

Commands: `harness company docs block update`,
`harness company docs block archive`, and
`harness company docs block remove`.

`block update` dispatches a governed `block.append` update for the existing
Block and keeps `Document.block_ids` unchanged. `block remove` dispatches only
`document.append` to remove the Block from the visible order while preserving
the Block row. `block archive` dispatches `block.append` with archived metadata
inside `Block.content` and then `document.append` to remove it from the visible
order. `archive` and `remove` require `--confirm` unless they are dry-runs.
None of these commands physically delete records or imply Work, Approval,
Finance, Organization, or Execution effects.

## Typed records and relations

Use TypedRecord and Relation commands for structured business truth. Do not
hide structured changes inside prose Blocks.

Commands: `harness company docs typed-record update`,
`harness company docs relation unlink`,
`harness company docs relation relink`,
`harness company docs relation repair-missing`, and
`harness company docs typed-record validate`.

`typed-record update` dispatches a governed `typed_record.append` update for an
existing record. It may change title, fields, and lifecycle status; it must not
change record id, module, record type, source Document, creator, or created
time. `--merge-fields` overlays the supplied JSON object on existing fields;
without it, `--fields-json` replaces the full fields object.

`relation unlink` dispatches a governed `relation.append` update that marks the
latest Relation row `lifecycle_status=archived`. It does not physically delete
the Relation or alter endpoints, type, provenance, creator, or created time.
Unlink requires `--confirm` unless it is a dry-run. Active Docs query and
health checks ignore archived Relations, so unlinking a required
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

## Template provenance

Create reusable templates explicitly instead of changing an existing page's
`Document.kind` in place:

```bash
harness company docs template create \
  --definition <custom-page-definition-id> \
  --parent-document <document-id> \
  --title "Vendor onboarding template" \
  --from-document <source-document-id> \
  --actor <human-or-agent-id>
```

Without `--from-document`, this creates an empty `Document(kind=template)`.
With `--from-document`, it copies the source Document's ordered native Blocks
into the new template through governed `block.append` and `document.append`
updates. The source Document keeps its original identity, kind, blocks, and
relations. Template creation does not create TypedRecords, Relations,
WorkItems, Approvals, or Finance effects.

Change reusable template lifecycle state explicitly:

```bash
harness company docs template status \
  --definition <custom-page-definition-id> \
  --template <template-document-id> \
  --status active|paused|archived \
  --actor <human-or-agent-id>
```

This updates only `Document.lifecycle_status` for a `Document(kind=template)`
through governed `document.append`. It refuses ordinary pages and does not
mutate existing Documents that already recorded the template through
`template_ref`.

`harness company docs document create` may carry a template provenance pointer:

```bash
harness company docs document create \
  --definition <custom-page-definition-id> \
  --parent-document <document-id> \
  --title "Vendor onboarding note" \
  --template <template-document-id> \
  --instantiate-template \
  --actor <human-or-agent-id>
```

New DocumentSpace roots use an explicit administrative bootstrap path rather
than borrowing another module's PageDefinition:

```bash
harness company docs document create \
  --root \
  --authority <human-admin-id> \
  --id <root-document-id> \
  --space <space-id> \
  --title "AgentOS / Star Harness" \
  --actor <human-or-agent-id>
```

This creates only a root `Document` with `parent_document_id=null`. It does not
create a `BusinessModule`, page definition, WorkItem, Approval, Finance record,
Organization member, execution space, or Project Binding. Create the module
and page definition as separate governed/admin operations after the root
exists.

Without `--instantiate-template`, this records `Document.template_ref` only.
With `--instantiate-template`, it copies the template Document's ordered native
Blocks into the child through governed `block.append` and `document.append`
updates. It still does not create TypedRecords, Relations, WorkItems,
Approvals, or Finance effects. If the operation needs canonical records or
follow-up work, create those through their own governed commands. If a template
should correspond to a TypedRecord type, first declare the module policy with
`harness company docs module create --relation-rule-json
'{"relation_type":"source_for","from_kind":"document","to_kind":"typed_record","required":true,"cross_module":false}'`,
then create/link the concrete TypedRecord through `harness company docs
typed-record append` and `harness company docs relation link`.

## Structured block authoring

`harness company docs block append` supports plain text shorthand and structured
content:

```bash
harness company docs block append \
  --definition <custom-page-definition-id> \
  --document <document-id> \
  --kind callout \
  --content-json '{"title":"Decision needed","text":"Founder approval is required before filing.","tone":"warning"}' \
  --actor <human-or-agent-id>
```

Use:

- `--kind rich_text --text <body>` for ordinary paragraphs;
- `--kind heading --text <heading>` for section headings;
- `--kind callout --content-json <json>` for durable notes, decisions, risks,
  or warnings;
- `--kind simple_table --content-json <json>` for simple table content when the data
  is document-local prose. Use `typed-record append` plus a `view create` when
  rows are canonical business records.

Appending a Block must preserve `Document.block_ids`. If the Block row exists
but the Document navigation list does not reference it, treat the operation as
incomplete.

In the Document Focus UI, slash commands such as `/paragraph`, `/heading`,
`/callout`, and `/table` are only a safer way to choose the same governed Block
kind. They do not create local page truth. Block order is displayed from native
`Document.block_ids`; use the governed reorder command when the only intended
effect is changing that order:

```bash
harness company docs block reorder \
  --definition <custom-page-definition-id> \
  --document <document-id> \
  --block-order <block-id-2,block-id-1> \
  --actor <human-or-agent-id>
```

The order must contain exactly the existing `Document.block_ids` set. It must
not edit Block content, delete Blocks, merge/split Documents, or imply approval
of linked Work, Finance, Organization, or Execution state. Drag/drop UI may be
layered on this command later.

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
npx pnpm@9.15.4 acceptance:company-os:docs-cli
git diff --check
```

Use broader checks when code paths outside Docs changed:

```bash
npx pnpm@9.15.4 check:dashboard
```

Completion requires native evidence, not just a generated page or successful
mock fixture.
