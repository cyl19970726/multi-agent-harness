# Documentation Governance

```text
status: canonical repository contract
owner_role: Docs Governance
canonical_for: documentation modules, authority, lifecycle, context packs, maintenance workflow, and retirement policy
```

Documentation exists to reduce the context needed for a correct decision. More
documents are not automatically more knowledge. A document is justified only
when its authority, reader, lifecycle, and relationship to executable truth are
clear.

## Authority boundary: Notion vs repository

Notion is the product/development authority: product intent, Specs,
decisions-of-record for the product model, task execution state, and review
records live there (AgentFirm Home → 01 · Docs System → Canonical Docs; the
DEV-40 flip of 2026-08-18 promoted DOC-105..108 to the four Current
Canonical rows under the now-Accepted AF-ADR-014). The repository owns
versioned implementation
truth bound to code and validated by governance gates — schemas, CLI/API
surfaces, and the concrete facts a developer needs next to the code. When the
two disagree, Notion is the product authority and the repository must either
implement the change or record the gap in the Implementation Crosswalk and
Development Work — never fork doctrine silently in repository prose.

## Documentation modules

The repository has seven documentation modules. These are knowledge boundaries,
not seven independent sources of product truth.

| Module | Location | Owns | Default context? |
| --- | --- | --- | --- |
| Product | `docs/current/product/` | product mission, system ownership, object meaning, governance and UX contracts | yes, through a small context pack |
| Architecture | `docs/current/architecture/architecture*.md`, `docs/current/architecture/concept-model.md`, `docs/current/architecture/data-model.md`, `docs/decisions/` | implemented boundaries, durable decisions, source-of-truth and migration rules | selected files only |
| Execution | `docs/current/dashboard/`, `docs/current/integration/`, runtime/workflow docs | Agent Teams, Work and Messages, executors, providers, operator surfaces and runbooks | only for execution work |
| Design evidence | `design/<workstream>/` (git history) | versioned Expected, Actual, prompts, specs, overlays, comparisons and reviews | only for the selected workstream |
| Operations | `docs/current/operations/getting-started.md`, `docs/current/operations/operations.md`, `docs/current/architecture/schemas.md`, `docs/current/operations/governance-engine.md` | commands, release and governance gates | only for implementation/operations |
| Research | `research/` (git history) | unresolved evidence, comparisons and bounded proposals attached to an active decision or TeamWork | never default context |
| Historical evidence | verified external archives and Git history | provenance needed to interpret still-existing records or decisions | never default context |

The legacy Company OS system split (Docs / Organization / Work / Finance /
Approval) was retired with DOC-108; see the superseded ADRs and Git history
for that model. Current product contracts divide by execution-foundation
truth-owning system:

- **Agent Teams**: durable flat AgentTeam, TeamMembership, AgentMember;
- **Work**: Work, WorkOperation, WorkEvent, WorkDelivery, lifecycle and result
  routing;
- **Messages**: Message, MessageSubscription, CanonicalMessageDelivery;
- **runtime**: AgentSession, NativeSessionRef, NodeDaemon, RuntimeCommand,
  Execution Space and Project Binding.

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

### Product pack

1. `docs/current/product/prd.md`
2. `docs/current/product/agent-team-works.md` for Work questions;
3. the relevant ADR or schema.

### Execution foundation pack

1. `AGENTS.md`
2. `docs/current/architecture/architecture-map.md`
3. ADR 0050 for the Work/Message boundary, ADR 0056 when Message is in scope,
   and the selected executor contract; ADR 0026/0034/0051 are historical
   Mission/Wave evidence only;
4. the specific page, provider or runtime reference being changed.

Old visual candidates, completion audits and historical evidence are loaded
only to answer a historical or comparative question. Unresolved research must
be attached to an active TeamWork or decision; abandoned standalone studies are
deleted and remain recoverable from Git history.

## Creating or changing documentation

For every new request, Docs Governance follows this sequence:

1. **Classify the fact.** Identify its owning system and whether it is product
   intent, executable contract, implementation reference, evidence or history.
2. **Search the authority.** Extend the existing canonical document when its
   scope already owns the fact. A new file is not a substitute for editing the
   authority.
3. **Design relations.** Name linked TeamWorks, Actors, Approvals, Finance
   records, schemas, Actions and result Documents without copying their truth.
4. **Choose lifecycle.** Record owner, status, canonical scope, review trigger,
   replacement and retirement policy.
5. **Create governed Work when needed.** Material restructuring, new product
   authority or cross-system changes require a TeamWork and proportional review.
6. **Update entry points and registry.** One new authority must have one visible
   route and machine-readable governance metadata.
7. **Validate and return the result.** Run governance checks, record evidence,
   and update the originating Document or decision.

## Extend, split, merge or retire

Extend an existing document when the owner, reader and lifecycle are the same.
Split only when at least one of these changes materially. Merge when multiple
files answer the same operator question or repeat the same object rules.

Retire (delete) a document when:

- its canonical scope moved elsewhere;
- it teaches a retired object or workflow as current;
- it is a dated audit or implementation plan no longer needed for normal use;
- its useful facts are now enforced by schema, code, CLI or tests;
- a new Agent cannot tell whether it is current without reading another file.

Historical evidence belongs in an immutable external export when it must travel
with retired runtime data, or in git history for abandoned prose. Active
indexes must not place it in the default reading order. There is no archive
folder; retirement means deletion (git history preserves recovery).

## Retention and redundancy audit

A document remains in the repository only when it satisfies at least one
retention test:

1. it is the current authority for a named scope;
2. it explains implemented code, schema, store, API or an operator procedure;
3. it is an active Expected/Actual design contract used for implementation or
   acceptance;
4. it is required to reproduce a current compliance, migration or acceptance
   claim;
5. it is unresolved research attached to an active decision or TeamWork;
6. a live record or supported compatibility path still references it and cannot
   be migrated safely.

“It may be useful later”, “it took effort to write”, and “it is historical”
are not retention reasons. If none of the tests pass, delete the
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
a bounded cleanup TeamWork, not a permanent record-keeping activity.

## Governance roles

- **Docs Governance Agent** proposes placement, merging, metadata, link repair,
  review dates and retirement actions. It does not change another system's product
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

Docs Governance is an Organization capability, not a background formatter.
Roles below (Docs/System/Lead/Human) are company choices, not a fixed hierarchy. It
maintains four visible Work queues:

| Queue | Trigger | Output |
| --- | --- | --- |
| Intake and placement | a new business activity, policy, module or result has no obvious home | owning system, canonical parent, record type, relations and initial owner |
| Authority conflicts | two active documents claim the same rule or disagree | one retained authority, repaired references, explicit replacement/decision |
| Structural maintenance | a document becomes too large, a module gains new readers, or navigation no longer reveals the business shape | split/merge proposal, updated module/index/views and migration notes |
| Retirement | a schema, product model, design or runbook is superseded | preserved evidence where required, forwarding note where old references exist, retired registry status and removal plan |

Each queue item is a `TeamWork`, with the Docs Governance Agent assigned for
information architecture. The Governance Agent of the affected system remains
accountable for meaning. The Lead resolves cross-system ownership; the Human
Owner approves protected policy or authority changes. Docs Governance may move
and link information but cannot silently redefine legal, financial, permission
or organization truth.

The document UI in the Agent Dashboard provides ordinary execution surfaces
by default. Docs Governance should publish a small health view rather than a
second task system: unowned canonical documents, conflicting scopes, broken
relations, stale reviews, archival candidates, unresolved placement requests
and recent structural decisions. All remediation remains normal TeamWorks.

## Required metadata and review

Canonical and implementation-critical documents must be registered in
`docs/registry.json` with owner, status, lifecycle, authority class,
implementation state, truth references, canonical scope, dependencies, review
date, verification and reorganization trigger. Review is event-driven as well
as date-driven. A document must be reviewed when its schema, store, API, UI,
ADR, owning module or acceptance scenario changes.

These fields answer different questions and must not be inferred from one
another:

| Field | Question | Important boundary |
| --- | --- | --- |
| `status` | Is this document's prose draft, planned or stable? | `stable` means the document is dependable, not that its capability exists. |
| `lifecycle` | How quickly is this document expected to change? | It controls review cadence, not product completion. |
| `authorityClass` | What kind of truth may this document own? | `design_intent` and `research` cannot prove implementation. |
| `implementationState` | How much of the described capability exists? | Use `design_only`, `partial`, `implemented` or `verified`; downgrade when evidence is incomplete. |
| `truthRefs` | Which executable artifacts support that claim? | References are typed as `schema`, `store`, `api`, `ui`, `test`, `decision` or `runtime_evidence`. |

`implemented` requires at least one executable truth reference. `verified`
additionally requires a `test` or `runtime_evidence` reference. A fixture,
mockup, screenshot or polished Expected image does not by itself move a
capability beyond `design_only`; a UI reading fixtures is not Store-live Actual.
The registry gate enforces the structural minimum, while the owning Governance
Agent remains responsible for checking whether each reference actually proves
the claim.

The governance gate must prevent broken links, missing registered authorities,
stale review dates and retired product vocabulary in active authority. Explicit
compatibility, migration and historical contexts may mention retired terms, but
must label them as such.

For the Message fabric, current execution documentation uses identity-first
`Message`, `MessageSubscription` and one `CanonicalMessageDelivery` per
recipient. `TeamMessage`, `TeamMessageProjection`, `team_messages.jsonl`,
`manual_ack` and legacy ACK commands may appear only inside an explicitly
labelled historical/read-export boundary. A current page must never teach them
as a writer, inbox source, provider-dispatch fallback or acceptance signal.
Work/WorkDelivery and RuntimeCommand remain independent planes and must not be
described as Message kinds.

## Definition of healthy documentation

A future Agent can answer, without loading the repository:

- what the product is and which system owns the fact;
- which document is authoritative;
- what is implemented, planned, evidence or history;
- which records and modules are linked;
- what changed, why, who approved it and when it should be reviewed again;
- which older direction must not be reused.
