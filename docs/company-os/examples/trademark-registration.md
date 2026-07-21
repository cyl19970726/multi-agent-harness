# Trademark Registration: End-to-End Company OS Example

```text
status: canonical illustrative example
example_id: trademark-registration-cn-2026-018
```

This example shows how a new business domain becomes a linked, governed module
rather than an isolated document or payment.

## Scenario

The company needs a China trademark registration for a new brand. No approved
trademark module exists. The application is `CN-2026-018`; its official
registration fee is **¥3,000**.

| Participant | Actor type | Role |
| --- | --- | --- |
| Brand Owner Human | human | Accountable business owner and human approver. |
| Trademark Agent | agent | Prepares application, manages WorkItems, updates linked records. |
| External Lawyer | external | Provides constrained legal review and filing support. |
| Docs Governance Agent | agent | Proposes document and record architecture. |
| Work Governance Agent | agent | Creates and routes the durable filing WorkItem. |
| Finance Governance Agent | agent | Validates the commitment, controls, and evidence. |
| Org / HR Governance Agent | agent | Checks roles, capacity, permissions, and durable capability. |
| Finance reviewer | human or authorized agent | Verifies financial controls and evidence. |
| Legal reviewer | human or authorized legal role | Verifies filing and jurisdictional requirements. |

## 1. Grow the module deliberately

The Brand Owner Human creates a source document explaining brand, territory,
classes, intended filing date, and reason. That document creates a WorkItem
submitted by the Brand Owner Human and assigned to the Trademark Agent.

Because this is a new domain, the Docs Governance Agent proposes an
R2/R3 module design:

```text
Legal & IP
  -> Trademark Management
      -> Trademark overview
      -> Applications database
      -> Materials and evidence
      -> Classes and jurisdictions knowledge
      -> Timeline and deadlines
      -> Fees and finance view
      -> Risks, objections, and renewal calendar
```

It defines `TrademarkApplication` as a typed record related to the brand,
source document, Milestone, WorkItems, legal evidence, approvals, and
FinancialRecords. The Org/HR Governance Agent confirms the Trademark
Agent can own operational WorkItems, the External Lawyer is limited to this
matter, and the Brand Owner Human remains accountable.

The Lead sponsors the proposal. Legal and Finance review it because it creates
an external legal process and a financial payment. The Brand Owner Human (or a
named human with policy authority) approves the R3 filing/payment boundary.
The proposal, reviews, approval, module objects, and access grants are audited.

## 2. Create the application and work

```text
TrademarkApplication
- id: CN-2026-018
- jurisdiction: CN
- accountable_owner: Brand Owner Human
- operational_owner: Trademark Agent
- external_legal_support: External Lawyer
- source_document: Brand / Trademark registration request
- linked_milestone: Brand launch readiness (when applicable)
- status: preparation

WorkItem: Prepare and file CN-2026-018
- requested_by: Brand Owner Human
- submitted_by: Trademark Agent
- accountable_owner: Brand Owner Human
- assignees: Trademark Agent
- contributor: External Lawyer
- reviewer: Legal reviewer
- approver: Brand Owner Human
- source_document: Brand / Trademark registration request
- result_document: Legal & IP / Trademark Management / CN-2026-018
```

The Trademark Agent collects materials, drafts class selection, requests legal
review, and records evidence. It cannot self-approve the filing or spend. The
External Lawyer sees only matter-specific material and WorkItems.

## 3. Link the ¥3,000 fee to finance

The application does not store a copied number as financial truth. It relates to
this FinancialRecord chain:

```text
FR-BUDGET-CN-2026-018
- type: budget
- amount: ¥3,000
- source_document: CN-2026-018 application page
- business_record: TrademarkApplication CN-2026-018
- milestone: Brand launch readiness
- work_item: Prepare and file CN-2026-018

FR-COMMITMENT-CN-2026-018
- type: commitment
- amount: ¥3,000
- linked_to: FR-BUDGET-CN-2026-018
- business_record: TrademarkApplication CN-2026-018
- status: pending_approval
- evidence: official filing-fee schedule and budget review
- submitted_by: Trademark Agent
- accountable_owner: Brand Owner Human
```

Human approval changes the commitment to `approved`; it does not claim that
money moved. Only settlement with an official receipt creates
`FR-PAYMENT-CN-2026-018(type=payment)` linked to the commitment. If an agency
invoice exists, an `invoice` record links between commitment and payment. A
returned amount is a separate `refund` linked to the payment. No state is
inferred from chat, and fees are never changed in place to hide a correction.

Before payment, the Finance reviewer verifies amount, currency, payee/official
authority, policy, evidence, and approval scope. The human gate records a
decision for ¥3,000 and names the authorized payer. Settlement evidence updates
the payment; trademark, milestone-cost, and finance-report views update from the
same relations.

## 4. Close the loop

After filing, the Trademark Agent updates `CN-2026-018` with evidence, status,
deadlines, and result artifacts. The Legal reviewer reviews the WorkItem; the
Brand Owner Human accepts it. Later opposition, renewal, extra classes, or a
refund create linked WorkItems and records in the same module. The Document
Architecture Agent reviews actual use and proposes controlled amendments only
when evidence shows the template, relations, or organization need to change.

See [Financial Relations](../financial-relations.md) and
[Company OS Governance](../governance.md).
