---
name: company-finance-operator
description: Operate Company OS Finance through governed Store/API/Action contracts. Use when a Governance Agent or business Agent needs to inspect, propose, approve-link, transition, or reconcile Commitments, Payments, invoices, refunds, and monetary evidence without confusing financial state with Work or Docs.
---

# Company Finance Operator

Operate the Company OS Finance surface. This skill is a procedural capability,
not product authority. It helps an Agent handle monetary records through
governed contracts and avoid treating approval text, WorkItem notes, or
document tables as money state.

## Load the contracts

Before proposing or executing a durable Finance change, read:

- `docs/company-os/finance.md`
- `docs/company-os/work-items-and-approvals.md`
- `docs/company-os/implementation-truth-matrix.md`
- `docs/company-os/skill-contracts.md`
- `docs/company-os/governance.md`

When the monetary effect starts from a business document or module, also read:

- `docs/company-os/document-system.md`
- `docs/company-os/module-design.md`

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

## Current interface state

Finance records exist through the Company OS Store/API and governed Action
path. Until dedicated `harness company finance ...` commands are implemented,
use the current API/action contract and report CLI coverage honestly as
`partial`.

The intended command family is:

```bash
harness company finance query --commitment <commitment-id>
harness company finance propose-commitment --work-item <work-item-id> --amount <amount> --currency <currency>
harness company finance request-approval --commitment <commitment-id> --approver <human-id>
harness company finance transition-commitment --commitment <commitment-id> --status <status>
harness company finance record-payment --commitment <commitment-id> --amount <amount> --evidence <ref>
harness company finance reconcile --payment <payment-id>
```

Do not present those commands as implemented until the CLI and acceptance tests
exist.

## Safe workflow

1. Inspect the source WorkItem and Docs context before changing Finance.
2. Determine whether the request is a proposed future spend, an approved
   commitment, an actual payment, a refund, or a metric observation.
3. Create or update Finance records only through the governed Company OS Action
   path. Do not edit document tables or JSONL ledgers directly.
4. Apply approval policy. If the amount, actor, category, or policy requires
   Human approval, request Approval before transition or payment.
5. Link Finance records back to WorkItem and Docs. The finance record is the
   money truth; Docs renders it and Work references it.
6. Record evidence for actual effects: invoice, receipt, transfer record,
   payment processor id, refund id, or reconciliation note.
7. Report state precisely: proposed, pending approval, approved, committed,
   paid, rejected, cancelled, refunded, or reconciled.

## Validation checklist

- Amount, currency, category, cost center, and related WorkItem/Docs refs are
  explicit.
- Actor has appropriate Organization authority or a Human approval exists.
- Approval is a durable Approval record, not a comment.
- Commitment and Payment ids are distinct when both exist.
- Payment links to related commitment refs.
- Evidence refs are durable and inspectable.
- Docs and Work show Finance links without duplicating money truth.

## Report format

When handing off, state:

- finance capability status: `implemented`, `partial`, `planned`, or
  `design-only`;
- Commitment ids and statuses;
- Payment/refund/invoice ids, if any;
- source WorkItem and Docs refs;
- approval refs and decision actor;
- evidence refs;
- remaining system gaps.
