# Docs operating surface matrix

```text
status: canonical Docs operating-surface audit (page layer superseded)
owner_role: Docs Governance Agent
canonical_for: Docs page capabilities, CLI/skill coverage, visual evidence, and remaining implementation gaps
superseded_note: >
  Everything this matrix describes about page/document surfaces (Block-era
  document/block/template commands, BasicDocumentPage, docs workspace) is
  superseded for the parts implemented by the AI-first Docs v2 target: see
  docs/company-os/ai-first-docs-spec.md (incl. the old-surface retirement
  plan) and ADR 0054. At retirement stage R3 the Block-era document/template/
  block CLI command tree and the document.append/block.append API actions were
  deleted; page-layer contracts now live in ADR 0054 and the AI-first Docs
  spec. Record-layer surfaces (modules, typed records, views, relations,
  health) remain tracked here until migrated.
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
| Docs Workspace | Company memory entrypoint, document tree, proposed modules, maintainers, structure health, templates, external product sources, and command affordances. | `DocumentSpace` concept, `Document`, template `Document`, `Block`, `BusinessModule`, `View`, `Relation`, `CustomPageDefinition`, `CustomPagePackage`, source-mapping `TypedRecord`s, maintainer `ActorRef`s. | Implemented projection-backed workspace with root selection, maintainers, structure notes, projection-only filtering for operating areas/templates/recent records, recent records, legacy template library (read-only since R3) with lifecycle badges and ordered Block counts, template → TypedRecord relation policy visibility, and CLI/Skill command panel for governance, page, and record-layer commands plus external software source sync. | `company-docs-operator` covers `harness company docs query`, `search`, `traverse`, `refs`, `related`, `health`, `source sync`, `module create`, `page create`, `page read`, `page write`, `page append`, `page search`, `page rename`, `page move`, `page archive`, `page scaffold`, `page verify`, `page publish`, `page-definition create`, `typed-record append`, `typed-record update`, `typed-record validate`, `view create`, `view update`, `relation link`, `relation unlink`, `relation relink`, `relation repair-missing`, `diff`, `snapshot`, and `change-report`; read commands return latest projection context with no side effects; source sync writes external-project/product-doc snapshot `TypedRecord`s and their `source_for` Relations; page scaffold/verify/publish commands create/verify/publish code-declared page metadata without making UI a second truth; record/view/relation commands remain governed Action wrappers, while page authoring writes through the v2 revision mechanism. | V2/V3 visual contract includes Docs Workspace; dashboard checks cover projection-only Workspace filtering, native template library, template lifecycle visibility, template relation policy visibility, and template command affordances; Store-live Docs CLI acceptance proves query/search/traversal/reference reads, reusable template creation/status update, PageDefinition/PagePackage verification, Document rename/move/archive, Block update/archive/remove, TypedRecord update/validate, View update, Relation unlink/relink/repair dry-run and idempotency, and no unrelated side effects. | SQL-backed global/full-text search index, nested DocumentSpace policies, template versioning, template approval workflow, persistent module field-schema contracts, GitHub webhook transport, source mapping UI, and DocumentSpace/module template governance remain planned. |
| Document Focus | Retired Block-era rich document reading/writing surface (source/result context, relation chips, template provenance, Block composer). | `Document`, `Block`, `Relation` rows remain as read-only legacy data. | Retired at stage R2: `?surface=docs` document deep links and `?surface=docs-v2` both render through DocsV2Surface over the v2 page endpoint; legacy ledger documents render as honest read-only projections (`legacy_projection` banner). `BasicDocumentPage`, `documentTree`, and the Block-era document/block command builders were deleted. | Retired at stage R3: the `document create/rename/move/archive`, `template create/status`, and `block append/update/archive/remove/reorder` CLI commands plus the `document.append`/`block.append` API actions were deleted, not deprecated-forwarded. Page reading, authoring, and structure maintenance now follow the v2 page command surface (`page create/read/write/append/search/rename/move/archive`) under ADR 0054 and `docs/company-os/ai-first-docs-spec.md`; legacy documents remain readable through `page read`. | Historical Block-era acceptance evidence remains valid as evidence of the retired surface (spec §12); the v2 surface earns its own evidence through the docs-v2 smoke and live suites. | No Block-era follow-up work remains: template versioning/approval and Block composer extensions are superseded by the v2 page model and return only if a real v2 template need appears. |
| Business Module Focus / standard module page | Recurring domain page over typed business records, standard Views, module root, relation-aware navigation, and code-declared custom page contracts. | `BusinessModule`, `TypedRecord`, `View`, `Relation`, `CustomPageDefinition`, `CustomPagePackage`, source `Document`, linked Work/Approval/Finance/Actors as references. | Implemented `?surface=docs&module=<id>` route over native BusinessModule TypedRecords with Store-live authoring controls for TypedRecord, View, and Relation. The page now exposes standard View provenance and saved configuration: module scope, native View ref, source kinds, query summary, record count, mode, filters, grouping, sorting, and explicit empty state. | `typed-record append`, `typed-record update`, `typed-record validate`, `view create`, `view update`, `relation link`, `relation unlink`, `relation relink`, `page scaffold`, `page verify`, and `page publish`; module/page-definition creation prepare the governed module and CustomPageDefinition policy bundle. `typed-record validate` is read-only schema checking against explicit JSON; persistent module field schema is still planned. `view update` changes presentation/query config only. | Store-live module action capture proves `typed_record.append`, configured `view.append`, and `relation.append` without Work/Approval/Finance side effects; CLI live acceptance proves typed record update/validate, View update, PageDefinition verify/publish, and relation unlink without Work/Approval/Finance/Organization/Execution side effects; dashboard checks prove native View/query provenance, saved configuration, and empty-state boundaries; visual review covers Business Module Focus. | Calendar/chart modes, richer saved view editing, persistent module field schemas, advanced field configuration, relation migration execution, and full custom page builder remain planned; custom page builder is available as a proposed skill flow but only approved page packages count as implemented pages. |
| Document Health Review | Governed document-architecture audit and repair routing. | `Document`, `Block`, `TypedRecord`, `Relation`, `View`, `BusinessModule`, health findings, cleanup queue entries, optional corrective `WorkItem` refs. | Implemented `?surface=docs&health=structure` review page with counts, findings, high-judgment cleanup queue, policy rail, CLI hints, Store-live corrective WorkItem action, and direct scoped Relation repair for the missing Document ↔ TypedRecord case. | `docs health`; browser Actions can dispatch corrective `work_item.append` or direct `relation.append` when the projection declares policy context. Cleanup queue candidates still route high-judgment rename/split/merge/archive/migration work to corrective WorkItems in the UI; Governance Agents may then use CLI `page rename|move|archive`, `typed-record update`, or `relation unlink` with dry-run/confirmation for low-level Docs maintenance. Health Review does not execute those direct structure/content mutations itself. | Store-live health captures prove corrective WorkItem routing without Finance/Approval/Payment side effects and direct Relation repair without Work/Finance side effects; the retired Block-era CLI live acceptance historically proved Document rename/move/archive, Block update/archive/remove, TypedRecord update, and Relation unlink without cross-system side effects (historical evidence for the retired page surface and still-valid evidence for the record layer); relation unlink also proves archived Relations disappear from active query/health and may resurface the missing-relation finding. Dashboard checks prove high-judgment cleanup routing markers. | Split, merge, delete, migration, bulk archival policy execution, rollback bundles, and Health Review UI dispatch for structure maintenance remain gated until their own Docs Action policies and review evidence exist. |

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
| `harness company docs source sync` | Docs Workspace / external software product source mapping | Governed local Git worktree sync into Docs `TypedRecord`s (`external_project`, `product_doc_source`, `product_doc_snapshot`, and `source_sync_run`) plus idempotent `Document → source_for → TypedRecord` Relations; snapshots preserve path, branch, commit, hash, headings, source class, and observation boundaries. | GitHub webhook authority, WorkItem creation, commercial-truth overwrite, Finance state, Organization mutation, approval, or software delivery execution. |
| `harness company docs module create` | Docs Workspace / Governance Proposal | Admin-created `BusinessModule` plus fallback `View`; optional explicit `relation_rules` via `--relation-rule-json`. | Business approval, Organization authority, custom page approval, concrete TypedRecord creation, or concrete Relation creation. |
| `harness company docs page-definition create` | Docs Workspace / Business Module Focus | Admin-created `CustomPageDefinition`, package, policies, and module refs. | Unlimited page writes or bypassed policy. |
| `harness company docs page scaffold` | Business Module Focus / code-declared custom page | Admin-created `CustomPageDefinition` and `CustomPagePackage` metadata for an Agent-built page over native Docs substrate. | React source implementation, visual acceptance, second data store, or product implementation claim by mock alone. |
| `harness company docs page verify` | Business Module Focus / code-declared custom page | Read-only PageDefinition/PagePackage contract check for module, fallback View, package, data queries, actions, policies, and visual contract refs. | Dispatch, build, deployment, or visual fidelity acceptance. |
| `harness company docs page publish` | Business Module Focus / code-declared custom page | Admin append of candidate `CustomPagePackage` metadata. It does not switch the active `CustomPageDefinition` package pointer in this first slice. | Business data mutation, active package promotion, visual proof, or UI as source of truth. |
| `harness company docs page create` | Pages v2 (ADR 0054) | Creates a page Document plus initial Block rows through the v2 revision mechanism; Markdown content, optional `--space`/`--parent`; top-level roots are created without `--parent`. | WorkItem lifecycle, approval, payment, Organization change, or execution claim. |
| `harness company docs page read` | Pages v2 (ADR 0054) | Read-only page projection with scoped reads (`outline/section/range/keyword`, `simple/with-ids/full`) and revision selection; legacy ledger documents project read-only with `legacy_projection=true`. | Mutation, approval, or execution claim. |
| `harness company docs page write` | Pages v2 (ADR 0054) | Whole-page immutable revision with sha256 digest; `expected_revision` optimistic concurrency; idempotent replay by action id. | In-place mutation, approval, payment, or execution success. |
| `harness company docs page append` | Pages v2 (ADR 0054) | Appends Markdown content as a revision with `--after` anchors (`block-id|-1|end|heading:text`). | Approval, payment, execution success, or private thinking persistence. |
| `harness company docs page search` | Pages v2 (ADR 0054) | Read-only keyword search over pages. | Mutation, ranking guarantee, or SQL index claim. |
| `harness company docs page rename` | Pages v2 (ADR 0054) | Metadata revision changing `Document.title`; dry-run by default without commit flags. | New Document identity, content rewrite, Work routing, approval, payment, or execution success. |
| `harness company docs page move` | Pages v2 (ADR 0054) | Metadata revision changing `Document.parent_document_id` with parent-cycle rejection; supports `root` moves. | Copying/duplicating records, cross-space migration, relation rewrite, Work routing, approval, payment, or execution success. |
| `harness company docs page archive` | Pages v2 (ADR 0054) | Lifecycle revision to `archived`; dry-run without `--confirm`, nothing written until confirmed. | Deletion, data loss, child cleanup, Work closure, approval, payment, or execution success. |
| `harness company docs typed-record append` | Business Module Focus | Governed source-linked `TypedRecord` append. | Work assignment, approval, or finance state. |
| `harness company docs typed-record update` | Business Module Focus | Governed existing TypedRecord update through `typed_record.append`; preserves record id, module, record type, source Document, creator, and creation time; supports field merge and dry-run. | Work assignment, approval, finance state, source migration, or schema evolution by implication. |
| `harness company docs typed-record validate` | Business Module Focus / Agent read surface | Read-only validation against explicit schema JSON for required fields and basic field types. | Persistent module schema, record mutation, approval, or migration. |
| `harness company docs view create` | Business Module Focus | Governed standard `View` append with mode/source/query configuration. | A second source of truth. |
| `harness company docs view update` | Business Module Focus | Governed latest `View` update through `view.append`; preserves View identity and changes presentation/query configuration. | TypedRecord mutation, second data store, approval, or finance state. |
| `harness company docs relation link` | Business Module Focus / Health Review | Governed `Relation` append. | Data duplication or repair of unrelated lifecycle state. |
| `harness company docs relation unlink` | Business Module Focus / Health Review | Governed Relation lifecycle archive through `relation.append`; preserves relation id, endpoints, type, provenance, creator, and creation time; requires dry-run or confirmation and makes active query/health ignore the archived relation. | Physical delete, endpoint migration, data duplication, or lifecycle repair beyond that Relation. |
| `harness company docs relation relink` | Business Module Focus / Health Review | Dry-run-first cleanup plan, or confirmed two-Action archive-plus-link sequence, for relation endpoint correction. | Physical delete, silent migration, Work closure, payment, or broad graph rewrite. |
| `harness company docs relation repair-missing` | Business Module Focus / Health Review | Definition-scoped dry-run of missing active `Document → source_for → TypedRecord` Relations, followed by confirmation-gated governed `relation.append` repairs; repeating after repair is a zero-write no-op. | Cross-module graph inference, physical delete, Work closure, payment, Organization mutation, or broad relation migration. |
| `harness company docs snapshot` | Docs Workspace / review evidence | Read-only current projection bundle for a selected ref and its related records. | Durable backup, rollback execution, or mutation. |
| `harness company docs diff` | Docs Workspace / review evidence | Read-only before/after field comparison for proposed JSON. | Dispatch, rollback, semantic merge, or approval. |
| `harness company docs change-report` | Docs Workspace / review evidence | Read-only report over an ActionCommand or proposed action JSON with before/after and changed fields. | Action authorization, dispatch, rollback, or human approval. |

## Evidence map

| Evidence | What it proves |
| --- | --- |
| `apps/agent-dashboard/tests/company-os-docs-check.mjs` | Docs UI surfaces, projection boundaries, authoring panels, command builders, semantic refs, and fixture/live truth separation. |
| `scripts/check-company-os-docs-cli-smoke.mjs` | Post-R3 CLI command surface (v2 page commands plus record layer), Block-era deletion, `company-docs-operator` skill coverage, and truth-boundary text. |
| `scripts/check-company-os-docs-v2-smoke.mjs` | Store-live v2 page CLI acceptance against the real binary: create, scoped reads, expected-revision write, revision conflict, idempotent replay, anchored append, embed resolution, and pinned revision reads. |
| `scripts/check-company-os-docs-v2-api.mjs` and `apps/agent-dashboard/tests/company-os-docs-v2-store-live-check.mjs` | Store-live v2 page API: revision writes, scoped reads, optimistic concurrency, rename/move/archive, and legacy projection. |
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

1. **Rich document editing:** page authoring moved to the v2 page surface:
   whole-page Markdown revisions with sha256 digests, `expected_revision`
   optimistic concurrency, scoped reads, and anchored append. Structure
   maintenance (`page rename|move|archive`) goes through the same revision
   mechanism with dry-run behavior and archive confirmation. Next CLI/API
   gaps are comment/mention/attachment primitives (the v2 CommentThread
   direction in the spec), richer inline formatting, rollback bundles, and
   richer verification/reporting over structure/content maintenance.
   Drag/drop UI and collaborative editing are later Human-facing conveniences
   over those commands.
2. **Template and page architecture:** templates are legacy-retained. The R3
   retirement deleted the Block-era template authoring/lifecycle commands;
   existing `Document(kind=template)` rows remain readable legacy records,
   and the Workspace shows them read-only with template → TypedRecord
   relation policy visibility from native module rules. A v2 template
   mechanism (versioning, approval workflow, DocumentSpace/module template
   governance) returns only when a real v2 template need exists.
3. **Standard View maturity:** native View/query provenance, saved mode/filter/
   grouping/sorting configuration, and empty-state boundaries are implemented
   for the first table/board/timeline slice. Next CLI/API gaps are `view
   update`, richer query validation, field configuration, and calendar/chart
   modes. Inline saved-view editing is a later UI affordance over those
   commands.
4. **Docs Governance cleanup Actions:** high-judgment cleanup candidates now
   route to corrective WorkItems. Direct `page rename|move|archive` exists
   as v2 metadata revisions with dry-run behavior and archive confirmation.
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
| P0 done / P1 extend | `page rename|move|archive` through the v2 revision mechanism | First v2 structure-maintenance slice is implemented; next extend with richer preflight reports, rollback evidence, and UI review affordances. |
| P0 done / P1 extend | v2 `page write`/`page append` content maintenance | Whole-page revision authoring is implemented without physical delete; next extend with rollback evidence, attachments/comments, and UI review affordances. |
| P0 done / P1 extend | `typed-record update|validate` and `relation unlink|relink` | First governed structured-record maintenance and validation slice is implemented; next extend with persistent module field schemas and relation migration execution. |
| P0 done / P1 extend | `view update` | Lets Agents maintain saved Views and query configuration; next add richer query validation and calendar/chart evidence. |
| P0 done / P1 extend | `docs diff|snapshot|change-report` | Lets Agents and Humans review proposed changes and rollback boundaries before mutation; next add durable rollback bundles. |
| P2 | UI action affordances for the above | Helps Humans trigger/review low-risk actions after CLI/API truth exists. |
| P3 | Rich collaborative editor / drag-drop polish | Useful Human experience, not the core Agent-operated interface. |
