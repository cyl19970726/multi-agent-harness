---
name: parked-company-finance-operator-20260805
description: Operate Company OS Finance through governed Store/API/Action contracts. Use when a Governance Agent or business Agent needs to inspect, propose, approve-link, transition, or reconcile Commitments, Payments, invoices, refunds, and monetary evidence without confusing financial state with Work or Docs.
---

# Company Finance Operator

Operate the Company OS Finance surface. This skill is a procedural capability,
not product authority. It helps an Agent handle monetary records through
governed contracts and avoid treating approval text, Work notes, or
document tables as money state.

## Select the Company Store

Before reading or writing Company OS records, identify the Company Store. Prefer
one of:

```bash
harness company current
harness --company <company-id> company finance ...
HARNESS_COMPANY=<company-id> harness company finance ...
```

If no Company is selected, `harness company ...` falls back to the current
project-derived compatibility Store. Treat that as legacy compatibility, not
the target Agent Company Workspace boundary.

To move legacy Company OS rows into a real Company Store, use
`harness company migrate-from-project --from-project <project-id|path> --id <company-id>`.
It copies only `company_os_*.jsonl`; it does not migrate execution records,
provider sessions, prompts, or runtimes.

## Load the contracts

Before proposing or executing a durable Finance change, read:

- `docs/current/company-os/financial-relations.md`
- `docs/current/company-os/work-items-and-approvals.md`
- `docs/current/company-os/implementation-truth-matrix.md`
- `docs/current/company-os/skill-contracts.md`
- `docs/current/company-os/governance.md`

When the monetary effect starts from a business document or module, also read:

- `docs/current/company-os/document-system.md`
- `docs/current/company-os/module-design.md`

If repository files, schemas, API code, or acceptance checks conflict with this
skill, the canonical implementation contract wins.

## Operating boundary

Finance owns every monetary state and monetary effect:

- `Commitment`
- `Payment`
- invoice
- refund
- budget or cost center links
- monetary metrics
- reconciliation/evidence refs

Finance does not own:

- Work lifecycle or task completion.
- Docs memory or document structure.
- Organization membership, permissions, or reporting.
- Legal filing outcome.
- Execution runs.

A `Commitment` is not a `Payment`. An approved Commitment is not proof that
money was paid. A Payment without related commitment refs is invalid for the
Company OS contract.

## Docs page integration

Business Docs pages may show budgets, purchase needs, prize costs, merchant
settlements, or payment watchlists. These are Finance panels over Finance
records, not document-owned money state.

When a page contract references Finance, require:

- source Document and, when applicable, source Work ref;
- amount, currency, category, cost center, and business reason;
- Commitment id/status for planned spend;
- Approval id and human decision actor when policy requires approval;
- Payment id/status only after a separate payment record and evidence exist;
- evidence refs for invoices, receipts, transfers, refunds, or reconciliation.

If the page contains a local cost table, treat it as planning context until a
Finance Commitment exists. Do not let Docs text, Work status, or visual cards
imply payment, settlement, reimbursement, or budget approval.

## Current interface state

Finance records exist through the Company OS Store/API and governed Action
path. The first dedicated `harness company finance ...` command family is
implemented for inspection, proposed Commitment creation, Approval routing,
Commitment transitions, and Payment recording/transitions.

Use:

```bash
harness company finance list [--commitment-status <status>] [--payment-status <status>]
harness company finance query --commitment <commitment-id>
harness company finance query --payment <payment-id>
harness company finance propose-commitment \
  --source-document <document-id> \
  --amount <amount> \
  --currency <currency> \
  --submitted-by <actor-id> \
  --accountable-owner <human-id> \
  --authority <human-admin-id>
harness company finance request-approval \
  --definition <custom-page-definition-id> \
  --commitment <commitment-id> \
  --requested-by <actor-id> \
  --approver <human-id> \
  --evidence <ref>
harness company finance decide-approval \
  --definition <custom-page-definition-id> \
  --approval <approval-id> \
  --actor <human-id> \
  --decision approved|rejected
harness company finance transition-commitment \
  --definition <custom-page-definition-id> \
  --commitment <commitment-id> \
  --status proposed|pending_approval|approved|rejected|cancelled \
  --actor <actor-id> \
  [--approval <approval-id>] \
  [--evidence <ref>]
harness company finance record-payment \
  --definition <custom-page-definition-id> \
  --commitment <commitment-id> \
  --actor <actor-id> \
  --approval <payment-approval-id> \
  --evidence <ref>
harness company finance transition-payment \
  --definition <custom-page-definition-id> \
  --payment <payment-id> \
  --status prepared|settled|failed|reversed \
  --actor <actor-id> \
  [--approval <approval-id>] \
  [--evidence <ref>]
```

Current v1 boundary:

- `propose-commitment` creates an initial `proposed` Commitment through the
  existing Human administrative import path.
- `request-approval`, `decide-approval`, `transition-commitment`,
  `record-payment`, and `transition-payment` use the governed Action
  dispatcher and require `HARNESS_COMPANY_OS_TOKEN`.
- Payment approval is separate from Commitment approval. Do not reuse an
  approval for `commitment.append` as proof for `payment.append`.
- `record-payment` creates a Payment record and does not imply settlement.
  Settlement requires an explicit Payment transition and evidence.
- The nested operator surface is also available when a page definition declares
  the necessary finance and approval Actions:

```bash
harness company finance commitment list
harness company finance commitment show --commitment <commitment-id>
harness company finance commitment propose --definition <page-definition-id> --work <work-id> --source-document <document-id> --submitted-by <actor-id> --accountable-owner <actor-id> --amount <amount> --currency <CURRENCY> --relation <relation-id>
harness company finance commitment transition --definition <page-definition-id> --commitment <commitment-id> --status pending_approval|approved|cancelled|fulfilled --actor <actor-id> --approval <approval-id> --evidence <ref>
harness company finance payment list
harness company finance payment show --payment <payment-id>
harness company finance payment record --definition <page-definition-id> --commitment <commitment-id> --source-document <document-id> --submitted-by <actor-id> --accountable-owner <actor-id> --amount <amount> --currency <CURRENCY> --approval <approval-id> --evidence <ref>
```

Use `harness company approval request|decide` for Human approval records linked
to finance policies such as `<definition>:commitment.append` and
`<definition>:payment.append`.

Report budget, invoice, refund, reconciliation, and richer finance reporting as
planned until their CLI and acceptance checks exist.

## Safe workflow

1. Inspect the source Work and Docs context before changing Finance.
   If the request comes from a business page, inspect the page contract so the
   Finance record links back to the correct Document, Work, and right-rail
   finance panel.
2. Determine whether the request is a proposed future spend, an approved
   commitment, an actual payment, a refund, or a metric observation.
3. Create or update Finance records only through the Finance CLI/API. Do not
   edit document tables or JSONL ledgers directly.
4. Apply approval policy. If the amount, actor, category, or policy requires
   Human approval, request Approval before transition or payment.
5. Link Finance records back to Work and Docs. The finance record is the
   money truth; Docs renders it and Work references it.
6. Record evidence for actual effects: invoice, receipt, transfer record,
   payment processor id, refund id, or reconciliation note.
7. Report state precisely: proposed, pending approval, approved, committed,
   paid, rejected, cancelled, refunded, or reconciled.

## Validation checklist

- Amount, currency, category, cost center, and related Work/Docs refs are
  explicit.
- Actor has appropriate Organization authority or a Human approval exists.
- Approval is a durable Approval record, not a comment.
- Commitment and Payment ids are distinct when both exist.
- Payment links to related commitment refs.
- Evidence refs are durable and inspectable.
- Docs and Work show Finance links without duplicating money truth.
- Any Docs page that shows money state does so through Finance refs/View data,
  not copied prose or a custom page-local total.

## Report format

When handing off, state:

- finance capability status: `implemented`, `partial`, `planned`, or
  `design-only`;
- Commitment ids and statuses;
- Payment/refund/invoice ids, if any;
- source Work and Docs refs;
- approval refs and decision actor;
- evidence refs;
- remaining system gaps.
