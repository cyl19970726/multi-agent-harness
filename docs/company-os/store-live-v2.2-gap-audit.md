# Company OS Store-live V2.2 gap audit

```text
status: canonical implementation audit
owner_role: product-architecture
audited_at: 2026-07-20
scope: V2.2 UI against native Company OS schemas, stores, APIs, actions, and live evidence
```

## Executive finding

Company OS is already Store-backed. The next delivery step is **not** to build a
second Store-live substrate. The Rust product types, append-only ledgers,
read/write HTTP API, authority-labelled Dashboard projection, governed action
engine, and the trademark Store-live seed already exist and have V1 acceptance
evidence.

The remaining work is narrower and more concrete. The first proof item below
was completed on 2026-07-20; the other items remain:

1. **completed:** prove the merged V2.2 visual implementation against a real Store projection;
2. connect the visible Company OS controls to the governed Action transport;
3. add the product contracts that are still represented only in documentation
   or as generic `TypedRecord`s; and
4. remove stale comments and completion claims that blur those boundaries.

## Status vocabulary

| Status | Meaning |
| --- | --- |
| `implemented` | Native contract, persistence or runtime path, and focused acceptance exist for the claimed layer. |
| `partial` | A real implementation exists, but an important product path or native contract is absent. |
| `design-only` | Canonical documentation defines the behavior, but no native type/store/API path implements it. |
| `missing` | A required connection is absent and the current UI explicitly falls back or disables the operation. |

## Evidence matrix

| Capability | Status | Implemented evidence | Remaining gap |
| --- | --- | --- | --- |
| Document / Block / TypedRecord / Relation / View / BusinessModule | `implemented` | [`crates/harness-core/src/company_os.rs`](../../crates/harness-core/src/company_os.rs), [`crates/harness-store/src/company_os.rs`](../../crates/harness-store/src/company_os.rs), [`schemas/company-os/knowledge.schema.json`](../../schemas/company-os/knowledge.schema.json) | No second document substrate is needed. |
| Human / Standing Agent / External / Service and OrgUnit membership | `implemented` | Separate native actor records and ledgers in Core/Store; actor and organization schemas; Store transition tests | Standing Agent collaboration and organization mutation are not yet connected from the product UI. |
| WorkItem and Assignment | `partial` | Native `WorkItem`, `Assignment`, HTTP resources, plus governed `work_item.transition` lifecycle, responsibility, provenance, Approval-gate, replay, audit, and Store-live browser acceptance | Native `Milestone`, `WorkItem.milestone_ref`, and `WorkItem.work_type` are absent even though the canonical product docs require them. Assignment acknowledgement/reassignment still needs its own governed action. |
| Approval / Commitment / Payment governance | `implemented` | Native records, monotonic transition checks, Human authority enforcement, idempotent `ActionCommand`, and atomic audit writes in Store/API tests | Product controls do not dispatch these commands yet. Existing V2.2 approval buttons are deliberately disabled. |
| Company OS HTTP reads and writes | `implemented` | [`crates/harness-cli/src/company_os_api.rs`](../../crates/harness-cli/src/company_os_api.rs) exposes Store snapshot, typed resources, administrative import, and declared actions protected by `HARNESS_COMPANY_OS_TOKEN` | Approval Focus and WorkItem Focus use the transport. Its session capability is a local operator boundary, not final Human authentication. |
| Authoritative Dashboard projection | `implemented` | Snapshot uses `snapshot_contract=company-os-v1`, `projection_kind=live_company_os`, Store source metadata, and a revision hash; [`sourceTruth.ts`](../../apps/agent-dashboard/src/company-os/sourceTruth.ts) recognizes it fail-closed | None for read authority. Page-level completeness still depends on the adapter and supplied records. |
| V2.2 six-page Store-live read rendering | `implemented` | [`expected-vs-store-live-v2.2.html`](../design/company-os-v2/expected-vs-store-live-v2.2.html) and its manifest prove six routed browser renders from an authority-labelled isolated Harness Store, alongside Expected and fixture Actual | Read proof is complete. Interactive action coverage is tracked per native command. |
| V1 Store-live product evidence | `implemented` | [`implementation-acceptance.md`](../design/company-os-v1/implementation-acceptance.md), `scripts/seed-company-os-trademark-v1.mjs`, and 26 Store-backed captures | This proves the V1 implementation and backend chain, not the merged V2.2 visual revision. |
| Governed programmable-page backend | `implemented` | Server-owned action policy shapes, declaration scope checks, Human gates, idempotency, effect validation, and audit reservations | The frontend demonstration runtime is not the browser-to-server transport used by Company OS pages. |
| Frontend programmable-page action contract | `partial` | [`apps/agent-dashboard/src/company-os/runtime/`](../../apps/agent-dashboard/src/company-os/runtime/) denies undeclared actions, enforces policy and Human proof, and rejects undeclared effects in focused tests | Its example commands (`finance.commitment.request`, `finance.commitment.authorize`, and others) do not match the backend command vocabulary (`commitment.append`, `approval.decide`, and others). The runtime is not mounted into `CompanyOsRouter`. |
| WorkItem, Approval, Governance and Agent interaction controls | `partial` | WorkItem Focus dispatches `work_item.transition` from explicit Agent preparation through accountable Human completion; Approval Focus dispatches `approval.decide`. Both refresh Store truth and preserve replay/audit boundaries. [`work-item-lifecycle-actions.md`](work-item-lifecycle-actions.md), [`browser-action-transport.md`](browser-action-transport.md), and their Store-live galleries are the evidence. | Replace the local operator capability with actor-bound sessions; add native Request changes/follow-up Work and Assignment actions. Governance and Standing Agent collaboration remain missing. |
| Milestone | `design-only` | [`work-items-and-approvals.md`](work-items-and-approvals.md) defines Milestone as the only Work grouping above WorkItem; V2.2 Work honestly shows `Unassigned milestone` | Add native type, schema, ledger, API projection, relations, Work grouping, and acceptance. Do not add `Project`. |
| Work type | `design-only` | Canonical Work contract defines development, legal, procurement, finance, governance, and other types | Add the field and migration/default policy to the native WorkItem contract and UI filters. |
| MetricDefinition / MetricObservation | `partial` | BusinessModule can reference metric-definition IDs; Blocks and views can render metric-shaped content; the trademark seed stores a metric as a `TypedRecord` | Native MetricDefinition/MetricObservation types, ledgers, API resources, and authority rules are absent. |
| Governance Proposal | `partial` | The trademark seed stores the proposal as a `TypedRecord`; adapters can read typed governance proposal records | There is no native GovernanceProposal type or ledger and the top-level live snapshot currently emits `governance_proposals: []`. Decide whether it remains a typed business record or becomes a native governed object, then make docs and UI consistent. |
| Standing Agent subject-linked collaboration | `missing` | Standing Agent focus separates organization identity from execution identity and never persists thinking | Composer is disabled; no durable subject-linked conversation/action API is connected. Direct-report activity and delegation still need a product contract. |
| Lead direct reports | `implemented` | Store seed writes `agent_lead_actor_ref` plus Agent Lead membership; the projection adapter retains those facts; Lead focus derives Standing Agent reports only from the governed unit membership | Cross-unit actors remain a capability roster, explicitly not a reporting relation. Subject-linked delegation activity is still missing. |
| Git Issue / PR linkage | `design-only` | Product docs describe development WorkItem integration as an adapter concern | No native adapter currently proves WorkItem start, PR review, merge evidence, and acceptance linkage. Keep it outside the generic core until the Work contract is stable. |
| Host Wave acceptance | `missing` | Wave gate correctly refuses to invent an accepted run | Host executor has no eligible attempt creation path, so completed Host Waves cannot name an accepted attempt. This is an execution-foundation defect, separate from Company OS. |

## Contract discrepancies that must not be hidden

### The completion claim is broader than the interactive product

V1 completion evidence correctly proves the Store chain and browser read model.
It does not prove that a user can approve, request changes, reject, create an
organization actor, or message a Standing Agent from the current V2.2 pages.
Those controls remain disabled. Future completion language must distinguish
**backend governed action acceptance** from **interactive product action
acceptance**.

### Metric and governance records use a generic compatibility representation

The trademark seed stores `Metric_Observation` and `Governance_Proposal` as
`TypedRecord.record_type` values. This is honest only if the product decision is
that these are module-defined typed records. It must not simultaneously claim a
native `MetricObservation` or native Governance Proposal lifecycle exists.

### The Work contract and implementation have diverged

Canonical docs state that `Milestone` is the only durable grouping above a
WorkItem and that WorkItem carries `milestone_ref` and `work_type`. The native
Rust `WorkItem` has neither field. The V2.2 Work page correctly exposes this
gap instead of fabricating data; the model now needs to catch up with the
approved product decision.

## Ordered implementation after this audit

### Next Wave A — V2.2 Store-live proof

**Completed 2026-07-20.** The strict dual-mode capture, isolated Store seed,
six Store-live images, archived evidence, and four-way comparison are now
reproducible through `pnpm visual:capture:company-os-v2:live` and
`pnpm visual:compare:company-os-v2:live`.

- extend the V2 capture runner with a strict `live` mode;
- reuse the real isolated trademark Store seed and Harness server;
- require `data-company-os-data-mode=store-live` and `prototype=false`;
- capture the six V2.2 mother pages with source revision and Store archive;
- compare V2.2 Expected, fixture Actual, and Store-live Actual without replacing
  one truth class with another.

This is the smallest next step because it tests the already-merged UI against
the already-implemented backend before adding new mutations.

### Next Wave B — first interactive governed action

**First slice completed 2026-07-21.** Approval Focus now proves invalid-token
denial, Human approve/reject attribution, Store refresh, durable audits,
idempotent replay, an unchanged pending Commitment, and zero Payments. The
remaining B work is an actor-bound authenticated Human session rather than a
global local capability.

**Second slice completed 2026-07-21.** WorkItem Focus now proves an explicit
Standing Agent assignee can start and submit durable results, premature
completion is denied until the linked Approval is approved, the accountable
Human completes the WorkItem, exact replay is idempotent, and no Payment is
created. See `docs/design/company-os-v2/workitem-action-v1/review.html`.

- define one browser Action transport and credential boundary;
- align frontend declarations with server-owned command names;
- connect `approval.decide` on Approval Focus for a named Human actor;
- render policy denial, expired/missing approval, conflict, accepted effect, and
  durable audit references;
- refresh the Store-backed snapshot after an accepted command;
- keep Commitment and Payment as separate later transitions.

### Next Wave C — Work model completion

- add `Milestone` and `WorkType` to Core, schema, Store, API, projections, and UI;
- migrate existing WorkItems to an explicit unassigned state without inventing
  a Milestone;
- add Work overview, Milestones, All WorkItems, and typed filters;
- add Git Issue/PR only as an adapter after native acceptance is stable.

### Next Wave D — metrics, governance, and collaboration

- decide and implement the native-vs-TypedRecord boundary for metrics and
  governance proposals;
- derive Lead reports from organization membership rather than actor-list
  proximity;
- add subject-linked Standing Agent collaboration and governed organization
  proposals without mixing in Agent Team MemberRun lifecycle.

## Wave 1 exit decision

The Store-live substrate, trademark backend loop, and V2.2 Store-live read proof
are implemented. The next executable slice is therefore **one real Human
approval browser action**. Rebuilding Documents, actors, WorkItems, Approvals,
or finance ledgers would duplicate working product infrastructure and is
explicitly out of scope.
