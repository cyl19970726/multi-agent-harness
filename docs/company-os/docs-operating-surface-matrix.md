# Docs operating surface matrix

```text
status: canonical Docs operating-surface audit
owner_role: Docs Governance Agent
canonical_for: Docs page capabilities, CLI/skill coverage, visual evidence, and remaining implementation gaps
```

This matrix answers whether the Company OS Docs surface can be operated as the
intended Agent-operated, Human-reviewed, Notion-like but Agent-native product.
It is narrower than the
[Core page matrix](core-page-matrix.md): it tracks the Docs-owned operating
surfaces and their evidence chain from product object to UI, CLI/skill, visual
contract, Store-live acceptance, and known gaps.

The rule is strict: a capability is not implemented merely because a design
image, fixture, or skill text describes it. It is implemented only when the
schema/store/API or governed Action, UI path, and acceptance evidence prove it.

## Scope boundary

Docs own `Document`, `Block`, `TypedRecord`, `Relation`, `View`, and
`BusinessModule` records. Docs surfaces may show WorkItems, Approvals, Actors,
FinancialRecords, Missions, Waves, Agent Teams, Workflows, and provider runs
only as linked records owned by their respective systems.

```text
Docs owns context and structure.
Work owns commitment, assignment, lifecycle, and approval routing.
Organization owns humans, Standing Agents, roles, permissions, and authority.
Finance owns commitment, invoice, payment, refund, and monetary state.
Execution owns Mission/Wave, Agent Team, Workflow, provider, and host evidence.
```

No Docs page, CLI command, or skill may infer approval, payment, settlement,
organization authority, or execution lifecycle from a document update.

## Interface posture

Docs are operated primarily by Agents through CLI/API and skills. The UI is
primarily for Humans to inspect, review, and supervise company memory and
business structure. UI editing affordances are useful only after the
corresponding CLI/API command, Store effect, and acceptance checks exist.

```text
Agent primary interface: CLI/API + company-docs-operator skill
Human primary interface: Docs UI
Verification: CLI/API first, UI as review evidence
UI editing: secondary, low-risk, and never the only implementation proof
```

ADR 0036 fixes the product center: Docs is not a Notion editor clone. It is an
Agent-operated memory substrate with code-declared custom business pages and
Human-facing review UI. `CustomPageDefinition` and `CustomPagePackage` are
therefore core product contracts for important pages, not decorative metadata.

Therefore this matrix treats CLI/skill coverage as the first operating
surface. UI status is evidence that Humans can understand the state; it does
not replace the Agent-facing command surface.

Storage posture follows [ADR 0035](../decisions/0035-company-os-sql-read-model.md):
canonical writes remain append-only JSONL ledgers and latest projections.
SQL is introduced only as a derived read/query/index layer after CLI/API read
contracts stabilize; it is not the current canonical Docs Store.

## Surface matrix

| Surface | Product responsibility | Native objects | UI status | CLI / skill coverage | Visual / Store-live evidence | Current gaps |
| --- | --- | --- | --- | --- | --- | --- |
| Docs Workspace | Company memory entrypoint, document tree, proposed modules, maintainers, structure health, templates, and command affordances. | `DocumentSpace` concept, `Document`, template `Document`, `Block`, `BusinessModule`, `View`, `Relation`, `CustomPageDefinition`, `CustomPagePackage`, maintainer `ActorRef`s. | Implemented projection-backed workspace with root selection, maintainers, structure notes, projection-only filtering for operating areas/templates/recent records, recent records, native template library with lifecycle badges, ordered Block counts and provenance/instantiation boundary copy, template → TypedRecord relation policy visibility, and complete CLI/Skill command panel including reusable template creation and lifecycle updates. | `company-docs-operator` covers `harness company docs query`, `search`, `traverse`, `refs`, `related`, `health`, `module create`, `page scaffold`, `page verify`, `page publish`, `page-definition create`, `document create`, `document rename`, `document move`, `document archive`, `template create`, `template status`, `block append`, `block update`, `block archive`, `block remove`, `block reorder`, `typed-record append`, `typed-record update`, `typed-record validate`, `view create`, `view update`, `relation link`, `relation unlink`, `relation relink`, `diff`, `snapshot`, and `change-report`; read commands return latest projection context with no side effects; page commands create/verify/publish code-declared page metadata without making UI a second truth; document/block/record/view/relation commands remain governed Action wrappers. | V2/V3 visual contract includes Docs Workspace; dashboard checks cover projection-only Workspace filtering, native template library, template lifecycle visibility, template relation policy visibility, and template command affordances; Store-live Docs CLI acceptance proves query/search/traversal/reference reads, reusable template creation/status update, PageDefinition/PagePackage verification, Document rename/move/archive, Block update/archive/remove, TypedRecord update/validate, View update, Relation unlink/relink dry-run, and no unrelated side effects. | SQL-backed global/full-text search index, nested DocumentSpace policies, template versioning, template approval workflow, persistent module field-schema contracts, and DocumentSpace/module template governance remain planned. |
| Document Focus | Rich document reading/writing, source/result context, relation chips, linked work/finance/actors, template provenance, and result return surface. | `Document`, template `Document`, `Block`, `Relation`, source/result refs, linked `WorkItem`, `Approval`, `FinancialRecord`, `MetricObservation`, `ActorRef`. | Implemented projection-backed page with structured sections, tables, relation chips, source/result links, mobile-safe layout, Store-live child Document creation with optional `template_ref`, explicit Store-live template Block instantiation, template → TypedRecord relation boundary, empty Document placeholder, template provenance state, a governed Block composer with type affordances, slash-menu Block selection, native `Document.block_ids` order display, governed Up/Down reorder controls, and authoring permission/error feedback. | `document create`, `document rename`, `document move`, `document archive`, `block append`, `block update`, `block archive`, `block remove`, and `block reorder`; `document create` preserves `template_ref` provenance and can opt into template Block copying with `--instantiate-template`; `document rename/move/archive` update the latest Document row through `document.append`, preserve existing Blocks/relations, support dry-run, and require archive confirmation; Store-live UI can emit the same governed `block.append` + `document.append` sequence from template snapshots; template relation policy still requires later explicit `typed-record append` and `relation link`; `block append` supports `rich_text`, `heading`, `callout`, and simple `table` content while preserving `Document.block_ids`; `block update` updates an existing Block latest row without changing Document order; `block remove` removes a Block from visible `Document.block_ids` while preserving the Block row; `block archive` adds archived metadata to `Block.content` and removes it from visible order; `block archive/remove` require confirmation unless dry-run; `block reorder` is a scoped `document.append` update that must preserve exactly the existing Block set. | CLI live acceptance proves child Document with template provenance, opt-in template Block instantiation, declared module relation rule preservation, structured Blocks, governed Block update/archive/remove, no physical delete, governed `Document.block_ids` reorder, governed Document rename/move/archive, cycle rejection, archive/remove confirmation, and no Work/Approval/Finance side effects; dashboard checks cover browser template Block instantiation controls, template relation boundary, composer affordances, slash-menu, reorder controls, error/permission boundary, and empty/template states; visual contract covers Document Focus. | Collaborative rich editor, inline comments, drag/drop UI over the reorder Action, attachments/media, mentions, lightweight formatting, reusable template browser, rollback bundles, and advanced template management remain planned. |
| Business Module Focus / standard module page | Recurring domain page over typed business records, standard Views, module root, relation-aware navigation, and code-declared custom page contracts. | `BusinessModule`, `TypedRecord`, `View`, `Relation`, `CustomPageDefinition`, `CustomPagePackage`, source `Document`, linked Work/Approval/Finance/Actors as references. | Implemented `?surface=docs&module=<id>` route over native BusinessModule TypedRecords with Store-live authoring controls for TypedRecord, View, and Relation. The page exposes standard View provenance and saved configuration: module scope, native View ref, source kinds, query summary, record count, mode, filters, grouping, sorting, and explicit empty state. It also renders a Custom Page contract card showing active package, candidate package, fallback View, declared queries, declared Actions, visual contract, artifact/digest, and the boundary that page code is presentation rather than a second truth. | `typed-record append`, `typed-record update`, `typed-record validate`, `view create`, `view update`, `relation link`, `relation unlink`, `relation relink`, `page scaffold`, `page verify`, and `page publish`; module/page-definition creation prepare the governed module and CustomPageDefinition policy bundle. `page publish` records a candidate package metadata row and does not silently switch the active definition pointer. `typed-record validate` is read-only schema checking against explicit JSON; persistent module field schema is still planned. `view update` changes presentation/query config only. | Store-live module action capture proves `typed_record.append`, configured `view.append`, and `relation.append` without Work/Approval/Finance side effects; CLI live acceptance proves typed record update/validate, View update, PageDefinition verify/publish, candidate-package publish without active-pointer switch, and relation unlink without Work/Approval/Finance/Organization/Execution side effects; dashboard checks prove native View/query provenance, saved configuration, CustomPageDefinition/Package contract projection, and empty-state boundaries; visual review covers Business Module Focus. | Calendar/chart modes, richer saved view editing, persistent module field schemas, advanced field configuration, relation migration execution, sandboxed custom page runtime, active package promotion command, generated React package execution, and full visual acceptance gates remain planned; `company-page-builder` is available as an optional skill flow but only verified package metadata and approved implemented captures count as product evidence. |
| Document Health Review | Governed document-architecture audit and repair routing. | `Document`, `Block`, `TypedRecord`, `Relation`, `View`, `BusinessModule`, health findings, cleanup queue entries, optional corrective `WorkItem` refs. | Implemented `?surface=docs&health=structure` review page with counts, findings, high-judgment cleanup queue, policy rail, CLI hints, Store-live corrective WorkItem action, and direct scoped Relation repair for the missing Document ↔ TypedRecord case. | `docs health`; browser Actions can dispatch corrective `work_item.append` or direct `relation.append` when the projection declares policy context. Cleanup queue candidates still route high-judgment rename/split/merge/archive/migration work to corrective WorkItems in the UI; Governance Agents may then use CLI `document rename|move|archive`, `block update|archive|remove`, `typed-record update`, or `relation unlink` with dry-run/confirmation for low-level Docs maintenance. Health Review does not execute those direct structure/content mutations itself. | Store-live health captures prove corrective WorkItem routing without Finance/Approval/Payment side effects and direct Relation repair without Work/Finance side effects; CLI live acceptance proves Document rename/move/archive, Block update/archive/remove, TypedRecord update, and Relation unlink without cross-system side effects; relation unlink also proves archived Relations disappear from active query/health and may resurface the missing-relation finding. Dashboard checks prove high-judgment cleanup routing markers. | Split, merge, delete, migration, bulk archival policy execution, rollback bundles, and Health Review UI dispatch for structure maintenance remain gated until their own Docs Action policies and review evidence exist. |

## Command coverage

The current implemented Docs command surface is complete for the verified
first operating slice:

| Command | Owning surface | Native effect | Must not imply |
| --- | --- | --- | --- |
| `harness company docs query` | Docs Workspace / Document Focus / Business Module Focus | Read-only Agent operating context over latest projections: selected/root Document, ordered Blocks, children, templates, TypedRecords, Relations, Views, BusinessModule, health findings, available commands, and boundaries. | Mutation, search index existence, Work/Finance/Organization/Execution side effects, or UI-only state. |
| `harness company docs search` | Docs Workspace / Agent read surface | Projection-backed search over Documents, Blocks, TypedRecords, Views, BusinessModules, and CustomPageDefinitions. | SQL index existence, mutation, ranking guarantee, or hidden-store access. |
| `harness company docs traverse` | Docs Workspace / Document Focus | Read-only Document tree with ordered Blocks and bounded child traversal. | Mutation, recursive cleanup, or permission bypass. |
| `harness company docs refs` | Docs Workspace / Document Focus / Business Module Focus | Read-only references around one Document, TypedRecord, or BusinessModule, including active Relations and linked Work/Approval/Finance refs. | Ownership transfer, approval, payment, or execution claim. |
| `harness company docs related` | Docs Workspace / Agent read surface | Read-only related refs derived from active Relations. | Relation creation, graph database claim, or inferred authority. |
| `harness company docs health` | Docs Workspace / Health Review | Read-only health projection. | Cleanup, deletion, merge, or migration. |
| `harness company docs module create` | Docs Workspace / Governance Proposal | Admin-created `BusinessModule` plus fallback `View`; optional explicit `relation_rules` via `--relation-rule-json`. | Business approval, Organization authority, custom page approval, concrete TypedRecord creation, or concrete Relation creation. |
| `harness company docs page-definition create` | Docs Workspace / Business Module Focus | Admin-created `CustomPageDefinition`, package, policies, and module refs. | Unlimited page writes or bypassed policy. |
| `harness company docs page scaffold` | Business Module Focus / code-declared custom page | Admin-created `CustomPageDefinition` and `CustomPagePackage` metadata for an Agent-built page over native Docs substrate. | React source implementation, visual acceptance, second data store, or product implementation claim by mock alone. |
| `harness company docs page verify` | Business Module Focus / code-declared custom page | Read-only PageDefinition/PagePackage contract check for module, fallback View, package, data queries, actions, policies, and visual contract refs. | Dispatch, build, deployment, or visual fidelity acceptance. |
| `harness company docs page publish` | Business Module Focus / code-declared custom page | Admin append of candidate `CustomPagePackage` metadata. It does not switch the active `CustomPageDefinition` package pointer in this first slice; the Docs module UI surfaces active versus candidate status for Human review. | Business data mutation, active package promotion, visual proof, or UI as source of truth. |
| `harness company docs document create` | Document Focus | Governed child `Document` append; optional `template_ref` provenance; opt-in template Block copying with `--instantiate-template`. | WorkItem lifecycle, TypedRecord creation, relation migration, or business acceptance. |
| `harness company docs document rename` | Document Focus / Docs Governance | Governed `Document.title` update through `document.append`; supports `--dry-run`. | New Document identity, content rewrite, Work routing, approval, payment, or execution success. |
| `harness company docs document move` | Document Focus / Docs Governance | Governed `Document.parent_document_id` update through `document.append`; supports `--dry-run`, root move, parent existence check, and parent-cycle rejection. | Copying/duplicating records, cross-space migration, relation rewrite, Work routing, approval, payment, or execution success. |
| `harness company docs document archive` | Document Focus / Docs Governance | Governed `Document.lifecycle_status=archived` update through `document.append`; supports `--dry-run` and requires `--confirm` to dispatch. | Deletion, data loss, child cleanup, Work closure, approval, payment, or execution success. |
| `harness company docs template create` | Docs Workspace / Document Focus | Governed reusable `Document(kind=template)` append; optional ordered Block copying from `--from-document` through Docs Actions. | Mutation of the source Document, TypedRecord creation, Relation creation, WorkItem lifecycle, approval, or business acceptance. |
| `harness company docs template status` | Docs Workspace / Document Focus | Governed `Document.lifecycle_status` update for an existing `Document(kind=template)`. | Mutating ordinary pages, changing existing child `template_ref`s, approval, Work routing, Finance state, or template version creation. |
| `harness company docs block append` | Document Focus | Governed `Block` append plus source `Document.block_ids` update. | Approval, payment, execution success, or private thinking persistence. |
| `harness company docs block update` | Document Focus | Governed existing Block update through `block.append`; preserves Block identity, owning Document, creation metadata, and Document order. | Document structure rewrite, physical delete, Work routing, approval, payment, or execution success. |
| `harness company docs block archive` | Document Focus / Docs Governance | Governed Block update with archived metadata plus `Document.block_ids` removal from visible order; supports `--dry-run` and requires `--confirm` to dispatch. | Physical delete, Document archival, Work closure, approval, payment, or execution success. |
| `harness company docs block remove` | Document Focus / Docs Governance | Governed `Document.block_ids` removal from visible order while preserving the Block row; supports `--dry-run` and requires `--confirm` to dispatch. | Physical delete, Block content rewrite, Work closure, approval, payment, or execution success. |
| `harness company docs block reorder` | Document Focus | Governed `Document.block_ids` reorder through `document.append` while preserving the exact Block set. | Block content edits, deletion, merge/split, approval, payment, or execution success. |
| `harness company docs typed-record append` | Business Module Focus | Governed source-linked `TypedRecord` append. | Work assignment, approval, or finance state. |
| `harness company docs typed-record update` | Business Module Focus | Governed existing TypedRecord update through `typed_record.append`; preserves record id, module, record type, source Document, creator, and creation time; supports field merge and dry-run. | Work assignment, approval, finance state, source migration, or schema evolution by implication. |
| `harness company docs typed-record validate` | Business Module Focus / Agent read surface | Read-only validation against explicit schema JSON for required fields and basic field types. | Persistent module schema, record mutation, approval, or migration. |
| `harness company docs view create` | Business Module Focus | Governed standard `View` append with mode/source/query configuration. | A second source of truth. |
| `harness company docs view update` | Business Module Focus | Governed latest `View` update through `view.append`; preserves View identity and changes presentation/query configuration. | TypedRecord mutation, second data store, approval, or finance state. |
| `harness company docs relation link` | Business Module Focus / Health Review | Governed `Relation` append. | Data duplication or repair of unrelated lifecycle state. |
| `harness company docs relation unlink` | Business Module Focus / Health Review | Governed Relation lifecycle archive through `relation.append`; preserves relation id, endpoints, type, provenance, creator, and creation time; requires dry-run or confirmation and makes active query/health ignore the archived relation. | Physical delete, endpoint migration, data duplication, or lifecycle repair beyond that Relation. |
| `harness company docs relation relink` | Business Module Focus / Health Review | Dry-run-first cleanup plan, or confirmed two-Action archive-plus-link sequence, for relation endpoint correction. | Physical delete, silent migration, Work closure, payment, or broad graph rewrite. |
| `harness company docs snapshot` | Docs Workspace / review evidence | Read-only current projection bundle for a selected ref and its related records. | Durable backup, rollback execution, or mutation. |
| `harness company docs diff` | Docs Workspace / review evidence | Read-only before/after field comparison for proposed JSON. | Dispatch, rollback, semantic merge, or approval. |
| `harness company docs change-report` | Docs Workspace / review evidence | Read-only report over an ActionCommand or proposed action JSON with before/after and changed fields. | Action authorization, dispatch, rollback, or human approval. |

## Evidence map

| Evidence | What it proves |
| --- | --- |
| `apps/agent-dashboard/tests/company-os-docs-check.mjs` | Docs UI surfaces, projection boundaries, authoring panels, command builders, semantic refs, and fixture/live truth separation. |
| `scripts/check-company-os-docs-cli-smoke.mjs` | CLI command surface, `company-docs-operator` skill coverage, structured Block guidance, and truth-boundary text. |
| `scripts/check-company-os-docs-cli-live.mjs` | Store-live CLI authoring for module, page-definition, child Document, reusable template creation/status, structured Block, TypedRecord, View, Relation, and zero unrelated side effects. |
| `.visual-evidence/company-os-v2/company-os-docs-module-route-v1/capture-run.json` | Store-live module route opens the Docs-owned standard module page. |
| `.visual-evidence/company-os-v2/company-os-docs-module-action-v1/capture-run.json` | Browser module authoring creates native TypedRecord, View, and Relation records without Work/Approval/Finance side effects. |
| `.visual-evidence/company-os-v2/company-os-docs-health-action-v1/capture-run.json` | Docs Health can route a corrective WorkItem while leaving Finance/Approval/Payment untouched. |
| `.visual-evidence/company-os-v2/company-os-docs-health-relation-v1/capture-run.json` | Docs Health can repair a scoped Document ↔ TypedRecord Relation without Work/Finance side effects. |
| `docs/design/company-os-v3/trademark-native-closure-v1/review.html` | Current approved native visual review for the Docs Workspace, Business Module, and Work board trademark slice. |

## Remaining product gaps

The current surface is sufficient for the first governed Docs operating slice,
but it is not a complete Notion replacement. The next gaps should be closed in
CLI-first order:

0. **Agent query/read surface first slice:** `harness company docs query` now
   gives Agents one Document or module machine-readable operating context:
   selected/root Document, ordered Blocks, child Documents, templates,
   TypedRecords, Relations, Views, BusinessModule, health findings, available
   commands, and explicit side-effect boundaries. Remaining read gaps are deeper
   traversal, search, diff/export, and serving the same contract from the future
   SQL read model.

1. **Basic rich document editing:** the first governed composer slices exist
   for block type selection, slash-menu Block selection, empty state, template
   provenance display, durable-action hinting, native `Document.block_ids`
   order display, governed Up/Down reorder controls, and explicit authoring
   permission/error feedback. Safe document `rename|move|archive` now exists
   as governed CLI/API structure maintenance with dry-run and archive
   confirmation. Governed `block update`, `block archive`, and `block remove`
   now cover the first content-maintenance slice without physical delete. Next
   CLI/API gaps are comment/mention/attachment primitives, richer inline
   formatting, rollback bundles, and richer verification/reporting over
   structure/content maintenance. Drag/drop UI and
   collaborative editing are later Human-facing conveniences over those
   commands.
2. **Template and page architecture:** Workspace now exposes a native template
   library with ordered Block counts and the provenance-vs-instantiation
   boundary plus template-to-typed-record relation policy visibility from
   native module rules. Current CLI can preserve module relation rules with
   `--relation-rule-json`, can create a reusable `Document(kind=template)` with
   `harness company docs template create`, can copy ordered Blocks from a
   source Document into that new template without mutating the source, and can
   update template lifecycle state with `harness company docs template status`
   while leaving existing `template_ref` users untouched. Current CLI/Store-live
   browser support preserves `Document.template_ref` provenance and can opt
   into copying ordered template Blocks into the new Document through governed
   Docs Actions. Remaining gaps are template versioning, template approval
   workflow, and DocumentSpace/module template governance, not basic reusable
   template creation, lifecycle status, or relation-policy visibility.
3. **Standard View maturity:** native View/query provenance, saved mode/filter/
   grouping/sorting configuration, and empty-state boundaries are implemented
   for the first table/board/timeline slice. Next CLI/API gaps are `view
   update`, richer query validation, field configuration, and calendar/chart
   modes. Inline saved-view editing is a later UI affordance over those
   commands.
4. **Docs Governance cleanup Actions:** high-judgment cleanup candidates now
   route to corrective WorkItems. Direct `document rename|move|archive` exists
   as CLI/API structure maintenance with dry-run and archive confirmation.
   Split/merge/delete/migration, bulk archival policy execution, rollback
   bundles, and Health Review UI dispatch for structure maintenance remain
   gated until their own Docs Action policy and review evidence exist.
5. **Visual refresh evidence:** expected/actual captures for the richer Docs
   editor and governance surfaces, using the same screenshot-first discipline
   as the current Company OS visual contracts.

## CLI-first backlog

The next Docs implementation waves should prefer this order:

| Priority | Capability | Why it matters |
| --- | --- | --- |
| P0 done / P1 extend | `harness company docs query/search/traverse/refs/related` | First projection-backed Agent read contract is implemented; next serve richer search/traversal from the future SQL read model. |
| P0 done / P1 extend | `page scaffold|verify|publish` | First code-declared custom page contract is implemented at metadata level; next add governed active package promotion, generated React packages, and stronger visual-contract publish gates. |
| P0 done / P1 extend | `document rename|move|archive` with dry-run/verification | First governed structure-maintenance slice is implemented; next extend with richer preflight reports, rollback evidence, and UI review affordances. |
| P0 done / P1 extend | `block update` and scoped `block archive/remove` | First governed content-maintenance slice is implemented without physical delete; next extend with rollback evidence, attachments/comments, and UI review affordances. |
| P0 done / P1 extend | `typed-record update|validate` and `relation unlink|relink` | First governed structured-record maintenance and validation slice is implemented; next extend with persistent module field schemas and relation migration execution. |
| P0 done / P1 extend | `view update` | Lets Agents maintain saved Views and query configuration; next add richer query validation and calendar/chart evidence. |
| P0 done / P1 extend | `docs diff|snapshot|change-report` | Lets Agents and Humans review proposed changes and rollback boundaries before mutation; next add durable rollback bundles. |
| P2 | UI action affordances for the above | Helps Humans trigger/review low-risk actions after CLI/API truth exists. |
| P3 | Rich collaborative editor / drag-drop polish | Useful Human experience, not the core Agent-operated interface. |
