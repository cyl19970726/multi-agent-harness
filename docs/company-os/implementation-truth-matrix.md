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
| Docs | `document-system.md`; Document, Block, TypedRecord, Relation, View, BusinessModule | `schemas/company-os/knowledge.schema.json` | append-only ledgers and latest projections in `harness-store/src/company_os.rs` | read/direct administrative append plus declared document/block/typed-record/relation/view Actions | Docs Workspace, document page and module view consume the labelled Company OS projection | core/store/API tests and dashboard Docs checks | **partial** — native substrate exists, but root-document result writeback was not reachable through the governed module Action scope at audit time |
| Organization | `organization-and-actors.md`; Human, Standing Agent, External, Service, OrgUnit and Membership | `schemas/company-os/actors.schema.json` | typed actor and organization ledgers with reference validation | resource reads and administrative authoring; no governed Org/HR lifecycle proposal/approval Action family yet | current Store-live organization and actor projections exist; governance-led hierarchy remains Expected only | core/store/API reference tests and navigation checks | **partial** — identity and membership truth exist; organization evolution and approved governance target do not |
| Work | `work-items-and-approvals.md`, `work-operating-system.md`; WorkItem, Milestone, Assignment, Approval | `schemas/company-os/work.schema.json` | append-only ledgers, WorkQuery and projections | governed WorkItem transitions and Approval decisions; declared create commands exist but new WorkItem creation still relies on administrative seed because Action subjects must pre-exist | six responsive Work views and WorkItem/Approval action surfaces consume Store-live projection | core/store/API tests, Work checks and browser action scripts | **partial** — lifecycle is strong, but source Document → governed WorkItem/Assignment creation is not yet a clean native Action chain |
| Finance | `financial-relations.md`; Commitment and Payment stay separate | `schemas/company-os/finance.schema.json` | separate Commitment and Payment ledgers with monotonic validation | governed commitment/payment transitions and Human approval; initial Commitment proposal still relies on administrative seed | Finance and Approval views show Store-live monetary state and explicitly distinguish commitment from payment | core/store/API financial boundary tests and browser approval checks | **partial** — approval and settlement controls exist, but native WorkItem → proposed Commitment creation is incomplete |

## Cross-system trademark truth

The current API acceptance already proves real Store records, latest-row-wins
projection, assignment ownership, WorkItem lifecycle, a ¥3,000 Commitment,
Human Approval, audit events, idempotency and the no-Payment-before-settlement
boundary. It does **not** yet prove the complete product sentence because setup
uses administrative imports and Work completion only points at an existing
TypedRecord; it does not update the source Document and application record via
governed result-return Actions.

The smallest closure slice is therefore:

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

The scenario must assert that no fixture contributes business records and that
no Payment is inferred from the approved Commitment. Administrative bootstrap
may create the Human root, BusinessModule, page declaration and initial source
Document; it may not create the scenario's WorkItem, Assignment, Commitment,
Approval or returned result.

## Product gates

- `product_truth`: every displayed relationship resolves to native Store rows;
  the complete scenario is reproducible through governed Actions and tests.
- `visual_fidelity`: separately compares approved Expected and Store-live
  Actual at identical viewports. Product truth cannot waive visual defects and
  visual similarity cannot waive missing records.
- Organization lifecycle and rich governance-agent workspaces remain planned
  after this trademark slice; the UI must label them as Expected rather than
  Actual until their own schema, Action and acceptance chains exist.

