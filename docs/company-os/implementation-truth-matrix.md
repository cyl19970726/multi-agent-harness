# Company OS implementation truth matrix

```text
status: canonical implementation audit
owner_role: Lead Agent with four System Governance Agents
canonical_for: Docs, Organization, Work, Finance contract-to-acceptance status and the trademark closure gap
```

This matrix answers one question: what can the product prove today from native
records and executable code? A design image, fixture, seed script or stable
document is never counted as implementation evidence by itself. The
machine-readable companion is
[`implementation-truth-matrix.json`](implementation-truth-matrix.json).

## System matrix

| System | Product contract | Schema | Store | API / Action | Store-live UI | Acceptance | Honest state |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Docs | `document-system.md`; `docs-operating-surface-matrix.md`; Document, Block, TypedRecord, Relation, View, BusinessModule; optional `company-docs-operator` skill; SQL read-model direction in ADR 0030; Agent-operated/code-declared page direction in ADR 0031 | `schemas/company-os/knowledge.schema.json`, `schemas/company-os/programmable-page.schema.json` | append-only ledgers and latest projections in `harness-store/src/company_os.rs`; SQL is only a planned derived read/query/index layer, not the current canonical write Store | read/direct administrative append plus governed document/block/typed-record/relation/view Actions; root-document updates preserve identity and provenance; `harness company docs query`, `search`, `traverse`, `refs`, `related`, `health`, `snapshot`, `diff`, `change-report`, `typed-record validate`, and `page verify` are read-only; `harness company docs module create`, `page-definition create`, `page scaffold`, and `page publish` use Human-admin governance authoring, with `page publish` currently recording candidate package metadata only; `module create` can preserve explicit `relation_rules`; `document create`, `document rename`, `document move`, `document archive`, `template create`, `template status`, `block append`, `block update`, `block archive`, `block remove`, `block reorder`, `typed-record append`, `typed-record update`, configured `view create`, `view update`, `relation link`, `relation unlink`, and confirmed `relation relink` dispatch governed Actions; document structure maintenance preserves identity, existing Blocks, and references, supports dry-run, rejects parent cycles, and requires archive confirmation; block content maintenance preserves Block identity, owning Document, and creation metadata, supports dry-run, avoids physical delete, and requires archive/remove confirmation; typed-record maintenance preserves record identity, module/type/source and creation metadata while allowing title/field/lifecycle updates; relation unlink archives the latest Relation row without physical delete and active query/health ignore archived Relations; relation relink is archive-plus-link cleanup, not physical migration; `document create` can preserve `template_ref` provenance and opt into template Block instantiation; `template create` creates explicit `Document(kind=template)` rows and can copy ordered source Blocks without mutating the source Document; `template status` updates only `Document.lifecycle_status` for existing template Documents and preserves existing child `template_ref`s; `block append` supports structured Block kind/content plus text shorthand; `block reorder` preserves the exact existing `Document.block_ids` set | Docs Workspace, document page, standard module page and Document Health Review consume the labelled Company OS projection; `?surface=docs&module=<id>` routes to the Docs-owned standard module page; Store-live Document Focus can create child Documents with optional template provenance, and both CLI and Store-live UI can instantiate template Blocks through governed `block.append` + `document.append` Actions; Store-live Document Focus can append `rich_text`, `heading`, `callout`, and simple `table` Blocks through Action transport while fixture/read-only modes stay disabled; Document Focus renders actual Store Blocks when present, exposes a governed Block composer with type affordances, slash-menu Block selection, native block order display, governed Up/Down reorder controls, authoring permission/error feedback, and template → TypedRecord relation boundary, and preserves existing template refs during Block append; Docs Workspace exposes a native template library with lifecycle badges, ordered Block counts, provenance-vs-instantiation boundary copy, reusable template creation/status command affordances, template → TypedRecord relation policy visibility, plus projection-only filtering for operating areas, templates, and recent records; Document Health Review exposes a high-judgment cleanup queue that routes rename/split/merge/archive/migration candidates to corrective WorkItems instead of direct UI mutation; standard module page exposes native View/query provenance plus saved mode/filter/group/sort configuration and can create source-linked TypedRecords, configured Views, and Document ↔ TypedRecord Relations through Store-live Action transport; Docs Workspace lists the complete CLI/Skill command set for query/search/traverse/refs/related, health, module, page scaffold/verify/publish, page-definition, document create/rename/move/archive, template create/status, block append/update/archive/remove/reorder, typed-record append/update/validate, view create/update, relation link/unlink/relink, diff, snapshot, and change-report | core/store/API tests, dashboard Docs checks, CLI smoke/live acceptance, fixture browser capture, Docs module route capture, Store-live standard module authoring capture, Store-live Health-to-WorkItem capture, Store-live direct Relation repair capture, and Docs operating surface matrix audit | **partial overall; verified for trademark return, fixture health review, CLI-backed query/search/traverse/refs/related/health/module/page/page-definition/document/template/block/typed-record/view/relation/diff/snapshot/change-report primitives, CLI-backed PageDefinition/PagePackage scaffold/verify/publish candidate metadata, CLI-backed Document rename/move/archive with dry-run, archive confirmation, parent-cycle rejection, preserved Blocks/relations, and no Work/Finance/Organization/Execution side effects, CLI-backed Block update/archive/remove with dry-run, archive/remove confirmation, preserved Block rows, no physical delete, and no Work/Finance/Organization/Execution side effects, CLI-backed TypedRecord update/validate, View update, Relation unlink/relink dry-run with field merge, preserved structured record identity/source, archived Relation latest rows, active query/health filtering, and no Work/Finance/Organization/Execution side effects, template provenance via `Document.template_ref`, CLI-backed reusable template creation and lifecycle status, CLI-backed and Store-live opt-in template Block instantiation, native Workspace template library, projection-only filter, and template → TypedRecord relation policy visibility, optional `company-docs-operator` procedural skill, Store-live Document Focus child-document/structured-block composer controls with slash-menu, governed block reorder, block order, and authoring error boundaries, routed standard module page over native TypedRecords with View/query provenance and saved View configuration, Store-live standard module browser authoring for TypedRecord/configured View/Relation, Health Review cleanup queue routing to corrective WorkItems, corrective WorkItem routing, direct scoped Relation repair, Docs operating-surface evidence matrix, ADR 0030 SQL-as-derived-read-model decision, and ADR 0031 Agent-operated Docs plus code-declared page decision** — `docs query/search/traverse/refs/related/snapshot/diff/change-report` return read-only projection or review context and declare no Work/Finance/Organization/Execution side effects; the accepted trademark result appends a result Block and updates the source Document and application TypedRecord through Actions; health review can flag structure, route high-judgment cleanup candidates to scoped corrective WorkItems, repair missing Document ↔ TypedRecord Relations, and ignore archived Relations without Work/Finance/Approval/Payment side effects; SQL-backed full-text search/deeper query traversal/view/health/diff/export remains planned until read-model rebuild acceptance proves it; active package promotion for code-declared pages, collaborative rich editor, drag/drop layout editing over the governed reorder Action, global full-text search index, template versioning, template approval workflow, persistent module field-schema governance, DocumentSpace/module template governance, calendar/chart View modes, advanced field configuration, inline saved-view editing, comments/mentions/attachments, durable rollback bundles, relation migration execution, split/merge/delete/migration cleanup Actions, and Health Review UI dispatch for structure/content maintenance remain gated |
| Organization | `organization-and-actors.md`; Human, Standing Agent, External, Service, OrgUnit and Membership | `schemas/company-os/actors.schema.json` | typed actor and organization ledgers with reference validation | resource reads and administrative authoring; no governed Org/HR lifecycle proposal/approval Action family yet | current Store-live organization and actor projections exist; governance-led hierarchy remains Expected only | core/store/API reference tests and navigation checks | **partial** — identity and membership truth exist; organization evolution and approved governance target do not |
| Work | `work-items-and-approvals.md`, `work-operating-system.md`; WorkItem, Milestone, Assignment, Approval | `schemas/company-os/work.schema.json` | append-only ledgers, WorkQuery and projections | governed WorkItem creation from a source Document, Assignment creation, lifecycle transitions, Approval request/decision and idempotent audit | six responsive Work views and WorkItem/Approval action surfaces consume Store-live projection | core/store/API tests, Work checks and browser action scripts | **partial overall; verified for trademark flow** — creation, ownership, review and accountable completion are native Actions |
| Finance | `financial-relations.md`; Commitment and Payment stay separate | `schemas/company-os/finance.schema.json` | separate Commitment and Payment ledgers with monotonic validation | governed Commitment proposal from linked Work, transition to approval, Human decision and separately governed Payment | Finance and Approval views show Store-live monetary state and explicitly distinguish commitment from payment | core/store/API financial boundary tests and browser approval checks | **partial overall; verified for trademark commitment** — the ¥3,000 proposal and approval are native; no Payment is inferred |

## Cross-system trademark truth

The API acceptance now proves real Store records, latest-row-wins projection,
governed creation, assignment ownership, WorkItem lifecycle, a ¥3,000
Commitment, Human Approval, result evidence, Document/TypedRecord writeback,
audit events, idempotency and the no-Payment-before-settlement boundary.

The verified closure slice is:

```text
existing source Document
  -> governed work_item.append
  -> governed assignment.append
  -> governed commitment.propose
  -> governed approval.request
  -> governed commitment transition to pending_approval
  -> Human approval.decide
  -> assigned Standing Agent executes and submits evidence
  -> accountable Human completes WorkItem
  -> governed block/document/typed_record append returns result
  -> Store-live projection shows the same linked truth
```

The scenario asserts that no fixture contributes business records and that
no Payment is inferred from the approved Commitment. Administrative bootstrap
creates the Human root, BusinessModule, page declaration and initial source
Document; it may not create the scenario's WorkItem, Assignment, Commitment,
Approval or returned result.

## Product gates

- `product_truth`: every displayed relationship resolves to native Store rows;
  the complete scenario is reproducible through governed Actions and tests.
- `visual_fidelity`: the three P0 trademark pages now pass exact-size
  Expected/Store-live Actual review through
  [`trademark-native-closure-v1`](../design/company-os-v3/trademark-native-closure-v1/review.html),
  whose status is sourced from the adjacent machine-readable visual contract.
  Product truth cannot waive visual defects and visual similarity cannot waive
  missing records. The Work board's six native records are an explicit,
  truth-preserving deviation from the 24-card concept image.
- Organization lifecycle and rich governance-agent workspaces remain planned
  after this trademark slice; the UI must label them as Expected rather than
  Actual until their own schema, Action and acceptance chains exist.
- Docs Structure Health is implemented as a projection-backed review page,
  read-only CLI audit, Store-live corrective WorkItem router, and a narrow
  Store-live direct Relation repair for missing Document ↔ TypedRecord links
  whose endpoints are inside the declared module scope. Broader direct cleanup
  from that page remains planned until each command has its own action
  transport, policy checks, and acceptance evidence.
