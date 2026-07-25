# Company OS V1 implementation acceptance

Status: **legacy baseline evidence; active design intent moved to Company OS v2/v3**

Fixture: `company-os-trademark-v1`

Evidence clock: `2026-07-20T09:30:00+08:00`

This document records the retired Wave 7 acceptance boundary. Generated v1
expected images and the v1 visual contract have been removed from the active
repository because they are no longer product direction. Only the retained v1
actual screenshots and deterministic fixture remain as legacy comparison
inputs.

## One evidence chain

Each visual case has three deliberately separate paths:

| Evidence | Location | Meaning |
| --- | --- | --- |
| Current baseline | `.visual-evidence/company-os-v1/<run-id>/baseline/` | Current real route before implementation. Missing routes are recorded as gaps, not screenshots. |
| Expected | retired | Removed from the active repository; use Company OS v2/v3 visual contracts for current design intent. |
| Implemented | `.visual-evidence/company-os-v1/<run-id>/implemented/` | Real implementation captured with the declared data source, route, viewport, locale, and readiness contract. |

The capture manifest at `<run-id>/capture-run.json` records all three paths for
every page and viewport, even when a route is blocked. Transient browser
captures stay ignored; selected comparisons can later be promoted under the
design directory.

## Prototype and live evidence layers

The two data modes answer different questions and must never be merged:

| Data mode | Proves | Does not prove |
| --- | --- | --- |
| `fixture` | deterministic rendering, route identity, responsive behavior, and the canonical trademark fact chain | persistence, store reads, live API projection, or production readiness |
| `live` | the same pages and facts are rendered from a named real Harness project/store | user visual approval or production deployment |

Every `capture-run.json` records `data_mode`, an `assertion_contract`, and a
separate `data_source`. A fixture run identifies its isolated fixture API. A
live run records the Harness API base, projects endpoint, snapshot endpoint,
selected project descriptor, snapshot generation time, and Company OS
projection identity.

### Fixture contract

The authority remains
`docs/design/company-os-v1/fixtures/company-os-trademark-v1.json`. The browser
fixture does not fork those facts. Its manifest pins the authority by SHA-256,
and its loader fails when the authority changes without review.

During capture the fixture is available in both forms:

- `window.__COMPANY_OS_FIXTURE__` for deterministic component fixtures;
- read-only `GET /v1/company-os/fixture`, `GET /v1/company-os/bootstrap`, and
  `GET /v1/snapshot` API projections.

The API supplies a stable project, clock, workflows response, and SSE snapshot
frame so unrelated connection failures do not masquerade as page defects.

Fixture evidence is explicitly a prototype. It requires
`data-company-os-prototype="true"` and must never be presented as live store
evidence.

### Live-store contract

`--data-mode live` requires `--api-base-url` pointing to a real running Harness
server. It never starts the fixture API, never defines
`window.__COMPANY_OS_FIXTURE__`, and does not freeze the browser clock. Before
opening the dashboard it reads `GET /v1/projects` and the selected project's
`GET /v1/snapshot`; absence of a real `company_os` projection blocks capture.

The visible page root must declare both:

```html
data-company-os-data-mode="store-live"
data-company-os-prototype="false"
```

The runner additionally inspects the browser global and fails if a live page
exposes `window.__COMPANY_OS_FIXTURE__`. A page that falls back to the bundled
prototype cannot become live evidence merely because its URL points at an API.

## Scenario acceptance

The deterministic acceptance test proves one linked pre-settlement truth:

```text
Trademark application CN-2026-018 (Document)
  -> Trademark filing for Brand A (WorkItem)
  -> Trademark Agent (accepted Assignment)
  -> agent submission + filing package + legal review (Evidence)
  -> Brand Owner · Human (required Approval)
  -> Trademark filing fee · Commitment · ¥3,000 · pending approval
  -> result_document_ref and updated typed record return to the source truth
```

It additionally proves:

- Brand Owner is requester, accountable owner, and required human approver;
- Trademark Agent is assignee and submitter;
- Finance Agent is finance reviewer;
- External Lawyer is contributor and legal reviewer;
- all timestamps are in July 2026;
- the financial effect is a pending Commitment, not a Payment;
- no Payment record or settlement evidence exists.

Run:

```bash
node apps/agent-dashboard/tests/company-os-trademark-acceptance.mjs
```

Last fully synchronized result: **118 passed, 0 failed**. Expected-image hash
changes remain owned by the visual lane and must be synchronized before this
combined test is rerun.

## Browser route contract

A route is accepted only when its visible root declares the common attributes:

```html
<main
  data-company-os-page="work-item-focus"
  data-company-os-ready="true"
>
```

Fixture runs additionally require
`data-company-os-fixture="company-os-trademark-v1"` and
`data-company-os-prototype="true"`. Live runs require
`data-company-os-data-mode="store-live"` and
`data-company-os-prototype="false"` on that same root; they deliberately do
not require or claim a fixture identity. Their source is pinned by the Harness
API/store metadata in `capture-run.json`.

Required business objects must be rendered from the declared data source and
identified by `data-company-os-ref="<canonical-id>"`. Actor nodes also carry
`data-actor-type`; financial nodes carry `data-financial-record-type`.
These are semantic capture anchors, not user-visible debug labels.

The runner rejects a route when any of the following is true:

- page, required data-mode identity, or readiness identity is absent;
- a fixture page slice lacks one of its required record references;
- an actor type is implicit;
- the ¥3,000 record is not explicitly a Commitment;
- a Payment or settlement-evidence node appears;
- durable provider-thinking state appears;
- the page logs a console error;
- the document or body has horizontal overflow.

## Coverage

All twelve pages require desktop evidence at `1440x1000`:

1. Home
2. Docs workspace
3. Document focus
4. Workboard
5. Work item focus
6. Finance
7. Organization
8. Standing Agent focus
9. Governance proposal
10. Approval focus
11. Business module focus
12. Human member focus

The seven focus/decision pages additionally require `900x1180` tablet and
`390x844` mobile evidence: Document, WorkItem, Standing Agent, Governance
Proposal, Approval, Business Module, and Human Member.

## Legacy status

The v1 capture script and v1 design-intent package have been retired. This file
is retained only to explain the legacy actual screenshots and deterministic
trademark fixture still used as comparison baselines. New capture, comparison,
or acceptance work must use the active Company OS v2/v3 visual contracts and
Store-live checks.

For the canonical acceptance scenario, use the deterministic public-API seed
orchestrator. It creates a temporary project and Store, starts the real Harness
server with `HARNESS_COMPANY_OS_TOKEN`, writes through the bootstrap and
administrative envelopes, and stops at the Human gate:

```bash
node scripts/seed-company-os-trademark-v1.mjs \
  --capture \
  --run-id company-os-v1-live-acceptance \
  --output .visual-evidence/company-os-v1/company-os-v1-live-acceptance
```

The seed fails if the Approval is not `requested`, the Commitment is not
`pending_approval`, its amount is not exactly CNY 3,000, any Approval is
`approved`, or any Payment exists. The live browser uses a capture-only
same-origin proxy for `/v1` because the production API intentionally does not
expose wildcard CORS. API preflight and evidence metadata continue to address
the real Harness server directly; no fixture is injected into the browser.

## Browser results

The pre-integration route audit is stored at:

`.visual-evidence/company-os-v1/wave7-route-audit/capture-run.json`

It reports **failed, 26 gaps**: all twelve desktop routes lack the Company OS
semantic root, and the fourteen dependent tablet/mobile captures were blocked
after their desktop routes failed. The current selection parser drops the new
`home`, `work`, `finance`, `organization`, and `approvals` surface identities,
so capturing the legacy Workbench would be false evidence. No baseline image
was promoted.

The final deterministic prototype capture is stored at:

`.visual-evidence/company-os-v1/company-os-v1-fixture-regression/capture-run.json`

It reports **passed, 26 captures, 0 gaps**:

- twelve desktop routes captured at `1440x1000`;
- seven focus/decision routes also captured at tablet and mobile, adding
  fourteen responsive captures;
- every page rendered all required canonical fixture references;
- actor and financial types were explicit;
- the ¥3,000 record remained a pending Commitment;
- no Payment, settlement evidence, or durable provider thinking appeared;
- console errors: 0;
- horizontal-overflow failures: 0.

This run has `data_mode=fixture`; it is implementation evidence for the page
system, not evidence of Company OS persistence.

The final real Store-backed capture is stored at:

`.visual-evidence/company-os-v1/company-os-v1-live-acceptance/capture-run.json`

It reports **passed, 26 captures, 0 gaps, 0 retries** with
`data_mode=live`. Its source is an authoritative `company-os-v1` Harness Store
projection. The sibling `seed-manifest.json` and
`live-company-os-snapshot.json` prove the pre-settlement gate, while
`archived-harness-home/` preserves the native append-only ledgers used by the
capture. The browser did not define `window.__COMPANY_OS_FIXTURE__`.

The selected durable visual evidence is under
`docs/design/company-os-v1/actual/` (26 PNG files). The old v1 three-way
comparison report and manifest have been removed; current comparison reports
live under Company OS v2/v3. The original columns deliberately meant different
things:

1. Current before: audited missing route, with no substitute screenshot;
2. Expected: generated design reference, still pending Human visual approval;
3. Actual: real browser render from the authoritative Store projection.

The runner retries one failed first render once in a fresh page load. This
separates an in-flight project/SSE navigation or Chromium
`ERR_NETWORK_CHANGED` event from a durable defect. It accepts the result only
when the complete second pass is clean; the first error remains in run
provenance, and product console errors or missing facts are never filtered.

Automated Wave 7 fixture and live-store acceptance are complete. Human review
of the implemented screenshots against the expected designs is still pending.
The browser checks prove route, data-source, fact, console, and overflow
contracts; they do not declare the generated expected designs Human-approved.
