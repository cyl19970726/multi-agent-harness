---
name: company-docs-operator
description: Operate Company OS Docs through governed CLI and Action contracts. Use when a Governance Agent or business Agent needs to audit document structure, create child documents or reusable templates, append structured blocks, create typed records, create views, link relations, or prepare a module/page-definition operation while preserving Docs/Work/Org/Finance truth boundaries.
---

# Company Docs Operator

Operate the Company OS Docs surface. This skill is a procedural capability, not
product authority. It helps an Agent choose the right governed CLI command,
prepare safe inputs, and verify the resulting native records.

Docs are Agent-operated and Human-reviewed. Use CLI/API as the primary Agent
interface for reading, editing, governing, and verifying document truth. Treat
the UI as Human review context: useful for understanding structure, status, and
risk, but not the authoritative machine interface.

## Load the contracts

Before writing or proposing a durable Docs change, read:

- `docs/company-os/document-system.md`
- `docs/company-os/skill-contracts.md`
- `docs/company-os/implementation-truth-matrix.md`
- `docs/company-os/governance.md`

When the change touches a recurring business domain or custom page, also read:

- `docs/company-os/module-design.md`
- `docs/company-os/agent-programmable-pages.md`

Do not use this skill to override those contracts. If a repository document,
schema, API, or acceptance check conflicts with this skill, the canonical
contract wins.

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

Use the smallest command that preserves the source of truth.

| Need | Command |
| --- | --- |
| Read one Document or module operating context | `harness company docs query` |
| Search Docs projection context | `harness company docs search` |
| Traverse a Document tree | `harness company docs traverse` |
| Inspect references around one object | `harness company docs refs` |
| Inspect related objects | `harness company docs related` |
| Audit document structure | `harness company docs health` |
| Create a governed business module | `harness company docs module create` |
| Scaffold a code-declared custom page contract | `harness company docs page scaffold` |
| Verify a custom page contract | `harness company docs page verify` |
| Publish custom page package metadata | `harness company docs page publish` |
| Install a page definition and policy bundle | `harness company docs page-definition create` |
| Create a child document | `harness company docs document create` |
| Rename a document | `harness company docs document rename` |
| Move a document in the tree | `harness company docs document move` |
| Archive a document | `harness company docs document archive` |
| Create a reusable template | `harness company docs template create` |
| Activate, pause, or archive a template | `harness company docs template status` |
| Append a document block | `harness company docs block append` |
| Update a document block | `harness company docs block update` |
| Archive a document block | `harness company docs block archive` |
| Remove a block from document order | `harness company docs block remove` |
| Reorder document blocks | `harness company docs block reorder` |
| Create a source-linked typed record | `harness company docs typed-record append` |
| Update a typed record | `harness company docs typed-record update` |
| Validate a typed record against explicit schema JSON | `harness company docs typed-record validate` |
| Create a standard module view with saved presentation config | `harness company docs view create` |
| Update a standard module view configuration | `harness company docs view update` |
| Link a document to a typed record | `harness company docs relation link` |
| Unlink a document/record relation | `harness company docs relation unlink` |
| Relink a relation through archive-plus-link cleanup | `harness company docs relation relink` |
| Produce review diff evidence | `harness company docs diff` |
| Export a scoped projection snapshot | `harness company docs snapshot` |
| Explain an ActionCommand before/after | `harness company docs change-report` |

Module and page-definition creation are administrative governance operations
and require a Human with `company_os.admin`. Ordinary document, block,
typed-record, view, and relation writes require a matching
`CustomPageDefinition` policy and the normal Company OS write capability.

## Safe workflow

1. Inspect current truth through CLI/API. Use `harness company docs query` as
   the first read command for one Document or module operating context, then use
   `harness company docs health` for broader structural audit. Prefer native
   Store projection reads over fixture or mock data.
2. Identify the owning object and actor. A write must name the source Document,
   target module/record when applicable, and the accountable Human or Agent.
3. Choose the command. Use standard Docs commands before proposing custom code.
4. Prepare idempotent, durable content. Do not include private reasoning,
   secrets, raw transcripts, or policy claims that the records cannot prove.
5. Run the command through the governed CLI/API path. Do not append ledgers
   directly.
6. Verify the result. Confirm the expected native row exists and unrelated
   ledgers did not change.
7. Use UI only for Human review and supplemental visible evidence. A UI-only
   change is not sufficient proof of a Docs capability.
8. Report evidence and remaining gaps. Distinguish `verified`, `partial`,
   `planned`, and `design-only`.

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

The response is the Agent-facing operating context over the current latest
projection: selected/root Document, ordered Blocks, child Documents, templates,
source-linked TypedRecords, Relations, module Views, BusinessModule,
CustomPageDefinition and ActionPolicyDefinition context, scoped health
findings, available commands, and explicit boundaries.

`docs query` does not create WorkItems, Approvals, Finance records,
Organization changes, execution runs, or UI-only state. The canonical write
store remains append-only JSONL ledgers plus latest projections. SQL is a
future derived read/query/index layer that must serve the same contract without
becoming write authority.

`search`, `traverse`, `refs`, and `related` are also read-only latest
projection commands. They help Agents find context without scraping the UI.
They do not prove a SQL index exists and they do not infer approval, payment,
authority, or execution state.

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

## Block content maintenance

Use explicit Block commands for content edits instead of replacing the whole
Document:

```bash
harness company docs block update \
  --definition <custom-page-definition-id> \
  --document <document-id> \
  --block <block-id> \
  --content-json '{"text":"updated"}' \
  --actor <human-or-agent-id> \
  --dry-run

harness company docs block archive \
  --definition <custom-page-definition-id> \
  --document <document-id> \
  --block <block-id> \
  --actor <human-or-agent-id> \
  --dry-run

harness company docs block remove \
  --definition <custom-page-definition-id> \
  --document <document-id> \
  --block <block-id> \
  --actor <human-or-agent-id> \
  --dry-run
```

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

```bash
harness company docs typed-record update \
  --definition <custom-page-definition-id> \
  --record <typed-record-id> \
  --fields-json '{"status":"accepted"}' \
  --merge-fields \
  --actor <human-or-agent-id> \
  --dry-run

harness company docs relation unlink \
  --definition <custom-page-definition-id> \
  --relation <relation-id> \
  --actor <human-or-agent-id> \
  --dry-run

harness company docs relation relink \
  --definition <custom-page-definition-id> \
  --relation <relation-id> \
  --to-record <typed-record-id> \
  --actor <human-or-agent-id> \
  --dry-run

harness company docs typed-record validate \
  --record <typed-record-id> \
  --schema-json '{"required":["status"],"properties":{"status":{"type":"string"}}}'
```

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
- `--kind table --content-json <json>` for simple table content when the data
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
`view update` writes a latest `view.append` row for presentation/query config
only. It must not mutate TypedRecords or create a second source of truth.
The first supported configuration slice is table/board/timeline mode, source
kinds, simple filters, grouping, and sorting stored in `View.query`. Calendar,
chart, advanced field layout, and inline saved-view editing remain planned
until their own UI and acceptance evidence exist.

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
