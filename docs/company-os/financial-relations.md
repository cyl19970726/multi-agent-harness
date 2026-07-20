# Financial Relations

```text
status: canonical Company OS contract
owner_role: finance
canonical_for: financial business records, their relations, and financial controls
```

## Purpose

Finance is a strongly typed, auditable business system. A document may display
financial data, but it must never become a second ledger by copying amounts into
free text. A financial view in Docs reads the same `FinancialRecord` that the
finance module, Milestone view, and related business module read.

## FinancialRecord

`FinancialRecord` is the durable record for a planned, committed, billed, paid,
or returned amount.

```text
FinancialRecord
- id
- type: budget | commitment | invoice | payment | refund
- amount
- currency
- status
- occurred_at / due_at?
- submitted_by: ActorRef
- accountable_owner: ActorRef
- source_document_id
- relation_ids
- evidence_refs
- approval_refs
- audit_event_ids
```

| Type | Meaning | Typical source |
| --- | --- | --- |
| `budget` | Approved or proposed spending limit; not a liability. | Milestone, module, period plan |
| `commitment` | Expected obligation or reserved spend; not payment proof. | Signed engagement, approved application |
| `invoice` | Supplier or authority charge requiring settlement. | Invoice, official fee notice |
| `payment` | Money actually paid or settled. | Payment confirmation, bank evidence |
| `refund` | Returned money linked to an earlier payment or invoice. | Refund notice, bank evidence |

Amounts retain their original currency. Conversion, if needed, is a separately
dated view with a stated exchange-rate source; it never overwrites the original
amount. Corrections are append-only records or explicit adjustments, never a
silent mutation of historical payment evidence.

## Required relations

Every record links to a source document and accountable owner. It may also link
to the following targets:

| Relation | Why it exists |
| --- | --- |
| `business_record` / `module` | Identifies the domain event, such as a trademark application. |
| `milestone` | Supports milestone budget, cost, and outcome views. |
| `work_item` | Connects spend to the action that caused or manages it. |
| `source_document` | Preserves the decision context and originating request. |
| `actor` | Records submitter, owner, reviewer, approver, payee, or vendor contact. |
| `financial_record` | Connects budget → commitment → invoice → payment → refund without guessing. |
| `evidence` | Connects contracts, invoices, receipts, payment proof, and notices. |
| `approval` | Proves required authority was granted before a sensitive state change. |

Embedded document views do not duplicate relations. A trademark page and a
monthly finance report present the same payment record.

## Controls, permissions, and audit

Financial actions use explicit permissions and append-only audit events. An
Agent may prepare, categorize, request approval, or reconcile supported
evidence only within granted policy. It cannot obtain payment authority simply
by creating a document or WorkItem.

- Each record carries a sensitivity classification and policy reference.
- Only authorized Actors can create, edit, approve, export, or settle records.
- Separation of duties applies where policy requires it: submitter, reviewer,
  approver, and payment executor may be different Actors.
- State changes record actor, time, previous and new state, reason, evidence,
  and authorization basis.
- Confirmed invoices and payment evidence are immutable except through an
  auditable correction, void, or reversal flow.
- Bank details, tax data, supplier contracts, and legal invoices are
  least-privilege and may be limited to named human and finance roles.

## Human approval boundary

Agents can recommend and prepare; they do not independently authorize a
financial commitment or payment unless policy explicitly grants it. The default
rule requires a named human approver for a new commitment or payment, changes
to approved amount/payee/account/jurisdiction, refunds/write-offs/reversals,
and any action at or above the financial, legal, or fraud-risk threshold.

An Approval records amount, currency, payee or authority, linked record(s),
decision, approver, policy, and expiry. A budget approval does not automatically
approve a later invoice or payment unless its scope says so.

## Lifecycle

```text
Source document / WorkItem
  -> budget (planned limit)
  -> commitment (approved obligation, when applicable)
  -> invoice (charge received)
  -> payment (settled amount)
  -> refund (if money returns)
```

This is a relation graph, not a mandatory linear sequence: an official fee may
have no invoice, or a payment may later be reversed and refunded. Exceptions
retain evidence and links so totals remain explainable.

See [the trademark registration example](examples/trademark-registration.md).
