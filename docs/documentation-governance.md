# Documentation Governance

```text
status: canonical repository contract
owner_role: Docs Governance
canonical_for: documentation modules, authority, lifecycle, context packs, maintenance workflow, and archive policy
```

Documentation exists to reduce the context needed for a correct decision. More
documents are not automatically more knowledge. A document is justified only
when its authority, reader, lifecycle, and relationship to executable truth are
clear.

## Documentation modules

The repository has seven documentation modules. These are knowledge boundaries,
not seven independent sources of product truth.

| Module | Location | Owns | Default context? |
| --- | --- | --- | --- |
| Product | `docs/prd.md`, `docs/company-os/` | product mission, system ownership, object meaning, governance and UX contracts | yes, through a small context pack |
| Architecture | `docs/architecture*.md`, `docs/concept-model.md`, `docs/data-model.md`, `docs/decisions/` | implemented boundaries, durable decisions, source-of-truth and migration rules | selected files only |
| Execution | `docs/dashboard/`, `docs/integration/`, runtime/workflow docs | Mission/Wave, executors, providers, operator surfaces and runbooks | only for execution work |
| Design evidence | `docs/design/<workstream>/` | versioned Expected, Actual, prompts, specs, overlays, comparisons and reviews | only for the selected workstream |
| Operations | `docs/getting-started.md`, `docs/operations.md`, `docs/schemas.md`, `docs/governance-engine.md` | commands, release and governance gates | only for implementation/operations |
| Research | `docs/research/` | external observations and unresolved exploration | never product authority |
| Historical evidence | verified external archives and Git history | provenance needed to interpret still-existing records or decisions | never default context |

Within Company OS, product contracts divide by truth-owning system:

- **Docs**: Document, Block, TypedRecord, Relation, View and BusinessModule;
- **Organization**: Actors, OrgUnits, reporting, permissions and authority;
- **Work**: WorkItem, Milestone, Assignment, lifecycle and result routing;
- **Finance**: monetary records, controls and evidence;
- **cross-system governance**: Approval, module/organization evolution and the
  four Governance Agent decision contracts;
- **execution foundation**: Mission/Wave and the selected executor, linked as
  evidence rather than company structure.

Business domains such as Trademark Management are `BusinessModule`s. They link
records from the four systems; they do not create a second document hierarchy
or duplicate those systems' truth.

## Authority classes

Every maintained document belongs to one class:

1. **Entry/index** — navigation only; it does not restate detailed contracts.
2. **Canonical contract** — owns a named product or architecture decision.
3. **Implementation reference** — explains current code, API, transport or
   operations; code/schema/store remains executable truth.
4. **Design intent** — versioned Expected direction, never implementation proof.
5. **Actual evidence** — reproducible browser/runtime evidence with provenance.
6. **Research** — input to a decision, never the decision itself.
7. **Historical evidence** — retained only when a live record, compliance need,
   or unresolved decision requires reconstruction; excluded from active planning.

Two active documents may not both claim the same `canonical_for` scope. A
summary links to the owning contract instead of copying its rules.

## Default context packs

Agents must not load all repository docs. Start with the smallest pack that can
answer the current decision.

### Company OS product pack

1. `docs/company-os/product-system-map.md`
2. the one system contract being changed;
3. `docs/company-os/four-system-collaboration.md` only for a cross-system change;
4. the relevant ADR or schema;
5. the selected visual workstream index when UI is in scope.

### Execution foundation pack

1. `AGENTS.md`
2. `docs/architecture-map.md`
3. ADR 0026 and the selected executor contract;
4. the specific page, provider or runtime reference being changed.

### Module-design pack

1. source Document or request;
2. `docs/company-os/module-design.md`;
3. the four-system ownership map;
4. relevant Organization, Work, Finance and Approval policy;
5. the domain example or adapter, if one exists.

Research, old visual candidates, completion audits and historical evidence are loaded only
to answer a historical or comparative question.

## Creating or changing documentation

For every new request, Docs Governance follows this sequence:

1. **Classify the fact.** Identify its owning system and whether it is product
   intent, executable contract, implementation reference, evidence or history.
2. **Search the authority.** Extend the existing canonical document when its
   scope already owns the fact. A new file is not a substitute for editing the
   authority.
3. **Design relations.** Name linked WorkItems, Actors, Approvals, Finance
   records, schemas, Actions and result Documents without copying their truth.
4. **Choose lifecycle.** Record owner, status, canonical scope, review trigger,
   replacement and archive policy.
5. **Create governed Work when needed.** Material restructuring, new product
   authority or cross-system changes require a WorkItem and proportional review.
6. **Update entry points and registry.** One new authority must have one visible
   route and machine-readable governance metadata.
7. **Validate and return the result.** Run governance checks, record evidence,
   and update the originating Document or decision.

## Extend, split, merge or archive

Extend an existing document when the owner, reader and lifecycle are the same.
Split only when at least one of these changes materially. Merge when multiple
files answer the same operator question or repeat the same object rules.

Archive or replace a document when:

- its canonical scope moved elsewhere;
- it teaches a retired object or workflow as current;
- it is a dated audit or implementation plan no longer needed for normal use;
- its useful facts are now enforced by schema, code, CLI or tests;
- a new Agent cannot tell whether it is current without reading another file.

Historical evidence belongs in an immutable external export when it must travel
with retired runtime data, or in a versioned design evidence workstream when it
is still used for visual comparison. Active indexes must not place it in the
default reading order. Git history is sufficient for abandoned prose that has
no ongoing audit, compliance, compatibility or record-interpretation value.

## Retention and redundancy audit

A document remains in the repository only when it satisfies at least one
retention test:

1. it is the current authority for a named scope;
2. it explains implemented code, schema, store, API or an operator procedure;
3. it is an active Expected/Actual design contract used for implementation or
   acceptance;
4. it is required to reproduce a current compliance, migration or acceptance
   claim;
5. it is unresolved research attached to an active decision or WorkItem;
6. a live record or supported compatibility path still references it and cannot
   be migrated safely.

“It may be useful later”, “it took effort to write”, and “it is already in an
archive folder” are not retention reasons. If none of the tests pass, delete the
file; Git history already preserves recovery. A forwarding note is justified
only while real inbound references still require that path.

The Docs Governance audit combines machine signals with an ownership review:

| Signal | Governance question | Default action |
| --- | --- | --- |
| no inbound links and no registry entry | Is this an undiscoverable authority or an orphan? | register and route it, or delete it |
| duplicated `canonicalFor` scope | Which document owns the rule? | merge into one authority and delete the copy |
| high text/heading overlap | Are two documents serving the same reader and lifecycle? | merge or make one a narrow implementation reference |
| archival/process status after implementation | Is executable truth now sufficient? | delete unless reconstruction is required |
| stale review date or broken dependency | Does the owner still stand behind it? | review, downgrade or delete |
| unreferenced Expected/Actual asset | Is it part of an active visual contract? | delete the asset and manifest entry |
| active document missing from registry/index | Is important product behavior absent from governance? | register it or merge it into an existing authority |

Run the audit after a product-model change, a large feature lands, a design
workstream closes, or the active document count grows materially. The result is
a bounded cleanup WorkItem, not a permanent archive-building activity.

## Governance roles

- **Docs Governance Agent** proposes placement, merging, metadata, link repair,
  review dates and archive actions. It does not change another system's product
  truth by itself.
- **System Governance Agent** for Docs, Work, Finance or Org/HR owns the content
  decision within that system.
- **Lead Agent** resolves cross-system conflicts and prioritizes restructuring.
- **Human Owner** approves changes to product authority, high-risk policy,
  permissions, legal/financial meaning or organization governance when policy
  requires it.

The registry and checks enforce consistency; they do not replace these decision
rights.

## The Docs Governance operating loop

Docs Governance is an Organization capability, not a background formatter. It
maintains four visible Work queues:

| Queue | Trigger | Output |
| --- | --- | --- |
| Intake and placement | a new business activity, policy, module or result has no obvious home | owning system, canonical parent, record type, relations and initial owner |
| Authority conflicts | two active documents claim the same rule or disagree | one retained authority, repaired references, explicit replacement/decision |
| Structural maintenance | a document becomes too large, a module gains new readers, or navigation no longer reveals the business shape | split/merge proposal, updated module/index/views and migration notes |
| Retirement | a schema, product model, design or runbook is superseded | preserved evidence where required, forwarding note where old references exist, archival registry status and removal plan |

Each queue item is a `WorkItem`, with the Docs Governance Agent assigned for
information architecture. The Governance Agent of the affected system remains
accountable for meaning. The Lead resolves cross-system ownership; the Human
Owner approves protected policy or authority changes. Docs Governance may move
and link information but cannot silently redefine legal, financial, permission
or organization truth.

For a new business module such as Trademark Management, the operating sequence
is:

```text
new request / source Document
  -> Docs Governance proposes BusinessModule, document tree and TypedRecords
  -> Org/HR Governance assigns accountable Actors and authority
  -> Work Governance creates WorkItems, Milestones and result routing
  -> Finance Governance links commitments/payments when money is affected
  -> owning System Governance Agents approve their records
  -> Lead or Human handles the cross-system gate when required
  -> execution runs
  -> result records return to Docs without duplicating Work or Finance truth
```

The document UI provides ordinary pages, blocks and tables by default. A core
page may use a code-declared composed View when it must present several linked
systems. That custom presentation still reads the same governed records; it
does not create a private database hidden inside HTML.

Docs Governance should publish a small health view rather than a second task
system: unowned canonical documents, conflicting scopes, broken relations,
stale reviews, archival candidates, unresolved placement requests and recent
structural decisions. All remediation remains normal WorkItems.

## Required metadata and review

Canonical and implementation-critical documents must be registered in
`docs/registry.json` with owner, status, lifecycle, canonical scope,
dependencies, review date, verification and reorganization trigger. Review is
event-driven as well as date-driven. A document must be reviewed when its
schema, store, API, UI, ADR, owning module or acceptance scenario changes.

The governance gate must prevent broken links, missing registered authorities,
stale review dates and retired product vocabulary in active authority. Explicit
compatibility, migration and archive contexts may mention retired terms, but
must label them as such.

## Definition of healthy documentation

A future Agent can answer, without loading the repository:

- what the product is and which system owns the fact;
- which document is authoritative;
- what is implemented, planned, evidence or history;
- which records and modules are linked;
- what changed, why, who approved it and when it should be reviewed again;
- which older direction must not be reused.
