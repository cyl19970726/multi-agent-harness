# Company OS implementation truth matrix

```text
status: canonical implementation audit
owner_role: AgentOS Lead with direct Team Hosts
canonical_for: Docs, Organization, Work, Finance, and Company authority contract-to-acceptance status and the trademark closure gap
```

> **Target versus current truth:** ADR 0052 adopts AgentMember as the durable
> agent identity, recursive AgentTeams as Organization, and one shared Work
> responsibility kernel. The Organization and Work rows below intentionally
> describe the currently shipped `StandingAgent` / Company `WorkItem`
> compatibility implementation. They do not prove the ADR 0052 target. The
> target now has an additive identity/topology foundation: a slim
> `DurableAgentMember` ledger, explicit root Lead bootstrap, deterministic
> compatibility mapping, and a refusal-first Host cutover audit. The shipped
> Organization and Work product still uses the compatibility rows below;
> HTTP/MCP/UI, Work-kernel convergence, full migration, and the Lead -> CTO ->
> child Team dogfood acceptance remain pending.
>
> ADR 0051 consolidates Mission and Wave: Mission absorbs Wave as an append-only
> Mission Log. The standalone Wave object and \`harness mission wave\` commands
> are retired (cutover PR #318). \`Mission/Wave\` as a conceptual execution
> reference is replaced by \`Mission\` with its Mission Log entries throughout
> all product docs, schemas, and skills. Work row and Work Operating System doc
> references below reflect this transition.

This matrix answers one question: what can the product prove today from native
records and executable code? A design image, fixture, seed script or stable
document is never counted as implementation evidence by itself. The
machine-readable companion is
[`implementation-truth-matrix.json`](implementation-truth-matrix.json).

## System matrix

| System | Product contract | Schema | Store | API / Action | Store-live UI | Acceptance | Honest state |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Identity / routing | ADR 0042; Company Store, Execution Space, and Project Binding are independent identities | Native `ExecutionSpace` and `ProjectBinding` core objects; Company and Execution Space registry metadata; `AgentTeamRun.project_binding_id`, `WorkflowRun.project_binding_id`, and Member workspace binding snapshot | Company truth under `<HARNESS_HOME>/companies/<id>/`; coordination under `<HARNESS_HOME>/execution-spaces/<id>/`; Project registry metadata and legacy compatibility stores under `<HARNESS_HOME>/projects/`; separate active markers | Independent `harness company ...`, `harness space ...`, and `harness project ...` commands plus `--company`, `--space`, and `--project`; serve exposes list/current/switch endpoints and independent `?company`, `?space`, and `?project` routing; execution migration is explicit, copy-only, verified, excludes Company/provider-native data, and preserves rollback | TopBar independently selects Company Store, Execution Space, and Project Binding; snapshots/SSE are space-scoped, Company pages are company-scoped, and provider/native activity uses the selected Project Binding | `execution_space_cli`, multi-project, serve/SSE, TeamRun delivery-cwd, Workflow cwd, Company Store, schema, and Dashboard checks prove selector independence, pinned cwd, discovery/worktree boundary, migration exclusion, and UI integrity | **implemented routing slice** — all three identities and selectors are native; `ProjectContext` and project-derived stores remain labelled compatibility infrastructure pending explicit migration/retirement |
| Company authority | [Human-rooted Company Constitution](company-constitution.md), [Scoped Company Authority Broker](scoped-authority-broker.md), [ADR 0047](../decisions/0047-scoped-company-authority-broker.md), [ADR 0048](../decisions/0048-human-rooted-company-constitution.md), and [Governance](governance.md) | No `ScopedPermissionGrant`, `CapabilityLeaseReceipt`, Constitution, grant-lineage, reservation, routine-audit-digest, exception-queue, approved Standing Agent template, or scoped-grant Approval subject schema exists | No corresponding Store ledgers or atomic reservation/fencing transactions exist; Company writes retain the service-side root token plus current Actor/Action policy records | Current Action dispatch checks Actor status, policy, permission refs, scope, risk, and Approval after root-token transport; Human administrative Organization authoring is bootstrap-only and cannot broker Agent authority | No Store-truth Constitution, grant lineage, reservations, exception queue, or authority digest UI exists | Existing Action/Organization tests prove only the current policy and bootstrap-admin boundaries; ADR 0047 one-command proof and all constitutional delegation/race/recovery/denial/UI acceptance remain pending | **design only / unimplemented** — current `HARNESS_COMPANY_OS_TOKEN` remains service-side; Constitution, recursive attenuation, temporary-member/approved-template rules, exact ProjectBinding fencing, atomic budget/concurrency/depth reservations, audit digest, exception-only queue, and one root R3 activation are contracts only |
| Docs | `document-system.md`; `docs-operating-surface-matrix.md`; Document, Block, TypedRecord, Relation, View, BusinessModule; optional `company-docs-operator` skill; SQL read-model direction in ADR 0035; Agent-operated/code-declared page direction in ADR 0036 | `schemas/company-os/knowledge.schema.json`, `schemas/company-os/programmable-page.schema.json` | append-only ledgers and latest projections in `harness-store/src/company_os.rs`; SQL is only a planned derived read/query/index layer, not the current canonical write Store | read/direct administrative append plus governed typed-record/relation/view Actions and v2 page revisions; root-document updates preserve identity and provenance; `harness company docs query`, `search`, `traverse`, `refs`, `related`, `health`, `snapshot`, `diff`, `change-report`, `typed-record validate`, and `page verify` are read-only; `harness company docs module create`, `page-definition create`, `page scaffold`, and `page publish` use Human-admin governance authoring, with `page publish` currently recording candidate package metadata only; `module create` can preserve explicit `relation_rules`; `page create`, `page read`, `page write`, `page append`, `page search`, `page rename`, `page move`, and `page archive` author pages through whole-page revisions with `expected_revision` optimistic concurrency (ADR 0054); `typed-record append`, `typed-record update`, configured `view create`, `view update`, `relation link`, `relation unlink`, and confirmed `relation relink` dispatch governed Actions; page structure maintenance (`page rename|move|archive`) preserves Document identity, rejects parent cycles, and requires `--confirm` to commit archive; the Block-era document/template/block command tree and the `document.append`/`block.append` Actions were deleted at retirement stage R3; typed-record maintenance preserves record identity, module/type/source and creation metadata while allowing title/field/lifecycle updates; relation unlink archives the latest Relation row without physical delete and active query/health ignore archived Relations; relation relink is archive-plus-link cleanup, not physical migration; `document create` can preserve `template_ref` provenance and opt into template Block instantiation; `template create` creates explicit `Document(kind=template)` rows and can copy ordered source Blocks without mutating the source Document; `template status` updates only `Document.lifecycle_status` for existing template Documents and preserves existing child `template_ref`s; `block append` supports structured Block kind/content plus text shorthand; `block reorder` preserves the exact existing `Document.block_ids` set | Docs Workspace, document page, standard module page and Document Health Review consume the labelled Company OS projection; `?surface=docs&module=<id>` routes to the Docs-owned standard module page; Store-live Document Focus can create child Documents with optional template provenance, and both CLI and Store-live UI can instantiate template Blocks through governed `block.append` + `document.append` Actions; Store-live Document Focus can append `rich_text`, `heading`, `callout`, and simple `table` Blocks through Action transport while fixture/read-only modes stay disabled; Document Focus renders actual Store Blocks when present, exposes a governed Block composer with type affordances, slash-menu Block selection, native block order display, governed Up/Down reorder controls, authoring permission/error feedback, and template → TypedRecord relation boundary, and preserves existing template refs during Block append; Docs Workspace exposes a native template library with lifecycle badges, ordered Block counts, provenance-vs-instantiation boundary copy, reusable template creation/status command affordances, template → TypedRecord relation policy visibility, plus projection-only filtering for operating areas, templates, and recent records; Document Health Review exposes a high-judgment cleanup queue that routes rename/split/merge/archive/migration candidates to corrective WorkItems instead of direct UI mutation; standard module page exposes native View/query provenance plus saved mode/filter/group/sort configuration and can create source-linked TypedRecords, configured Views, and Document ↔ TypedRecord Relations through Store-live Action transport; Docs Workspace lists the complete CLI/Skill command set for query/search/traverse/refs/related, health, module, page scaffold/verify/publish, page-definition, document create/rename/move/archive, template create/status, block append/update/archive/remove/reorder, typed-record append/update/validate, view create/update, relation link/unlink/relink, diff, snapshot, and change-report | core/store/API tests, dashboard Docs checks, CLI smoke/live acceptance, fixture browser capture, Docs module route capture, Store-live standard module authoring capture, Store-live Health-to-WorkItem capture, Store-live direct Relation repair capture, and Docs operating surface matrix audit | **partial overall; verified for trademark return, fixture health review, CLI-backed query/search/traverse/refs/related/health/module/page/page-definition/document/template/block/typed-record/view/relation/diff/snapshot/change-report primitives, CLI-backed PageDefinition/PagePackage scaffold/verify/publish candidate metadata, CLI-backed Document rename/move/archive with dry-run, archive confirmation, parent-cycle rejection, preserved Blocks/relations, and no Work/Finance/Organization/Execution side effects, CLI-backed Block update/archive/remove with dry-run, archive/remove confirmation, preserved Block rows, no physical delete, and no Work/Finance/Organization/Execution side effects, CLI-backed TypedRecord update/validate, View update, Relation unlink/relink dry-run with field merge, preserved structured record identity/source, archived Relation latest rows, active query/health filtering, and no Work/Finance/Organization/Execution side effects, template provenance via `Document.template_ref`, CLI-backed reusable template creation and lifecycle status, CLI-backed and Store-live opt-in template Block instantiation, native Workspace template library, projection-only filter, and template → TypedRecord relation policy visibility, optional `company-docs-operator` procedural skill, Store-live Document Focus child-document/structured-block composer controls with slash-menu, governed block reorder, block order, and authoring error boundaries, routed standard module page over native TypedRecords with View/query provenance and saved View configuration, Store-live standard module browser authoring for TypedRecord/configured View/Relation, Health Review cleanup queue routing to corrective WorkItems, corrective WorkItem routing, direct scoped Relation repair, Docs operating-surface evidence matrix, ADR 0035 SQL-as-derived-read-model decision, and ADR 0036 Agent-operated Docs plus code-declared page decision** — `docs query/search/traverse/refs/related/snapshot/diff/change-report` return read-only projection or review context and declare no Work/Finance/Organization/Execution side effects; the accepted trademark result appends a result Block and updates the source Document and application TypedRecord through Actions; health review can flag structure, route high-judgment cleanup candidates to scoped corrective WorkItems, repair missing Document ↔ TypedRecord Relations, and ignore archived Relations without Work/Finance/Approval/Payment side effects; SQL-backed full-text search/deeper query traversal/view/health/diff/export remains planned until read-model rebuild acceptance proves it; active package promotion for code-declared pages, collaborative rich editor, drag/drop layout editing over the governed reorder Action, global full-text search index, template versioning, template approval workflow, persistent module field-schema governance, DocumentSpace/module template governance, calendar/chart View modes, advanced field configuration, inline saved-view editing, comments/mentions/attachments, durable rollback bundles, relation migration execution, split/merge/delete/migration cleanup Actions, and Health Review UI dispatch for structure/content maintenance remain gated |
| Organization | `organization-and-actors.md`; Human, Standing Agent, External, Service, OrgUnit and Membership; optional `company-org-operator` skill; `nested-agent-team-organization.md`; [ADR 0052](../decisions/0052-nested-agent-teams-are-the-agent-organization.md) (accepted target contract; shipped row describes `StandingAgent` compatibility implementation); [ADR 0045](../decisions/0045-company-owned-standing-agent-execution-relation.md) superseded by 0052 | `schemas/company-os/actors.schema.json` adds optional `StandingAgent.execution_agent_member_ref`; optional `MemberRun.agent_member_id` remains in `schemas/member-run.schema.json` | typed actor and organization ledgers with reference validation; Agent Team participation remains in execution ledgers and is joined read-only only through the explicit Company-owned ref | resource reads plus flat `harness company org list/query/create-human/create-agent/create-unit/add-membership/transition-actor/update-permissions` and nested `harness company org actor/unit/membership ...` administrative authoring; Company snapshot derives lossless explicit `standing_assignments`; no governed Org/HR lifecycle proposal/approval Action family yet | Store-live organization plus shared Standing Agent and MemberRun focus surfaces show bidirectional explicit identity, assignment-less participation, bounded chronological assignment history, and read-only lifecycle facts; equal ids never bind | core/store/API reference tests, identity-integrity negative tests, Dashboard type/build checks, navigation checks, schema fixtures, skill parity, `check-company-os-org-cli-smoke`, and operator CLI smoke | **partial; Organization CLI v1, Standing Agent profile, and explicit Agent Team participation projection verified** — native prompt/tool/skill/Docs/WorkType/escalation refs, OrgUnit membership, declared status, permission/capability ref maintenance, explicit StandingAgent/AgentMember/MemberRun identity join, and distinct execution boundary exist; availability/capacity truth, Workflow/direct durable-agent participation, durable cross-client Supervisor health, and governed organization evolution proposal/approval do not |
| Work | `work-items-and-approvals.md`, `work-operating-system.md`; WorkItem, Milestone, Assignment, Approval; [ADR 0050](../decisions/0050-agent-team-work-board-and-message-boundary.md); [ADR 0051](../decisions/0051-single-intent-spine.md) (Mission absorbs Wave as MissionLog); [ADR 0052](../decisions/0052-nested-agent-teams-are-the-agent-organization.md) (unified Work kernel target) | `schemas/company-os/work.schema.json` | append-only ledgers, WorkQuery and projections | governed WorkItem creation from a source Document, Assignment creation, lifecycle transitions, shared Approval request/decision, baseline `harness company work milestone ...` administrative lifecycle, and idempotent audit | six responsive Work views, Standing Agent work/activity and WorkItem/Approval action surfaces consume Store-live projection | core/store/API tests, Work checks, browser action scripts and operator CLI smoke | **partial overall; verified for financial and non-financial loops plus baseline Milestone CLI** — Lead request, Work Governance submission/routing, Business Agent execution, review, completion and Docs result return are native Actions |
| Finance | `financial-relations.md`; Commitment and Payment stay separate; **retired per ADR 0053** (contract layer retired 2026-08-05, code dormant) | `schemas/company-os/finance.schema.json` | separate Commitment and Payment ledgers with monotonic validation (dormant) | flat CLI preserved but not active in contract layer; budget/invoice/refund/reporting and deeper settlement transition remain planned | Finance and Approval views show Store-live monetary state and explicitly distinguish commitment from payment | core/store/API financial boundary tests preserved; `check-company-os-finance-cli-smoke` retained as historical evidence (header marked RETIRED 2026-08-05) and still runs ungated in CI against the dormant flat CLI | **retired (contract) / dormant (code)** — ADR 0053: contract layer retired 2026-08-05; Commitment/Payment code dormant (append-only ledgers may hold rows); finance operator skill parked |

## Verified conditional operating loops

The API acceptance now proves real Store records, latest-row-wins projection,
governed creation, assignment ownership, WorkItem lifecycle, a ¥3,000
Commitment, Human Approval, result evidence, Document/TypedRecord writeback,
audit events, idempotency and the no-Payment-before-settlement boundary.

The same acceptance also proves a non-financial merchant-outreach path:

```text
Human-owned source Document
  -> Lead Agent requests work
  -> Work Governance Agent creates WorkItem and Assignment
  -> Sales Business Agent executes and submits evidence
  -> accountable Human completes WorkItem
  -> Sales Agent appends result Block and updates source Document
  -> zero Commitment and zero Approval records
```

The absence of Finance is asserted before the financial trademark path begins;
it is not inferred from missing fixture data.

## AgentOS self-hosting loop

The Company Store now contains an AgentOS Lead, Docs Governance Agent, Work
Governance Agent, Platform Development Agent, AgentOS Docs, and a real autonomy
WorkItem. The Store-live Organization, Docs, Work, Standing Agent, WorkItem,
and Document pages can project those identities and records.

This is a **partial dogfood foundation**, not a completed autonomous company.
The current verified slice is durable identity and readable linked work. The
blocking gaps are:

- relation-correct selected pages that never attach another business line's
  first Approval, finance record, typed record, or Actor;
- stable Standing Agent Inbox transport with busy, idle, offline/recovered,
  and closed delivery semantics;
- governed Organization proposal/provisioning and module-owned permission
  catalogs;
- durable Runtime Supervisor ownership across clients and service restarts;
- needs-attention projections and several real multi-directional Docs/Work/Org
  cycles with accepted result promotion.

The canonical operating model and staged visual contract are
[AgentOS self-hosting dogfood loop](agentos-self-hosting-loop.md) and
[`company-os-v5/agentos-self-hosting-loop-v1`](../design/company-os-v5/agentos-self-hosting-loop-v1/README.md).

The verified closure slice is:

```text
existing source Document
  -> governed work_item.append
  -> governed assignment.append
  -> proposed Commitment via current Human administrative import boundary
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
- Baseline Organization CLI authoring is Actual, but governed Organization
  lifecycle proposals and role-specific governance queues remain planned. The
  shared Standing Agent workspace and its explicit Agent Team participation
  projection are Actual; promotion, retirement, permission-change, Workflow
  participation, direct durable-agent messaging, and cross-client runtime
  health must remain planned until their Action and acceptance chains exist.
