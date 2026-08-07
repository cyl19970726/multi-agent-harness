# Company OS V1 shared fixture contract

`fixtures/company-os-trademark-v1.json` is the single source fixture for the
twelve Company OS V1 expected pages. It turns the visual series into one
inspectable pre-settlement business scenario, rather than twelve pages that
repeat similar-looking but unrelated facts.

## Fixed scenario

The fixture's only trademark application is `CN-2026-018` for **Brand A**. Its
source document title is exactly **Trademark application CN-2026-018**. The
linked WorkItem title is exactly **Trademark filing for Brand A**.

The responsibility chain is fixed:

| Work role | Actor |
| --- | --- |
| Requested by | Brand Owner · Human |
| Submitted by | Trademark Agent · Standing Agent |
| Assigned to | Trademark Agent · Standing Agent |
| Accountable owner | Brand Owner · Human |
| Contributor and legal reviewer | External Lawyer · External |
| Finance reviewer | Finance Agent · Standing Agent |
| Required approver | Brand Owner · Human |

The financial fact is likewise fixed: **Trademark filing fee · Commitment ·
¥3,000 · Pending approval**. It is a `commitment`, never payment evidence.
There is no `payment` record and no settlement evidence in this fixture.
Nothing may show a payment, paid amount, receipt, or completed settlement
before a later fixture explicitly supplies the requisite human approval and
settlement evidence.

All timestamps in the JSON are in July 2026 (`Asia/Shanghai`). IDs are stable
keys, not presentation strings; implementations resolve the records by ID and
must not manufacture page-local substitutes.

## Organization truth rule

An organization screen may only display a status that appears in
`organization.explicitly_reported_statuses`.

- `Trademark Agent` has the explicitly reported role state `proposed`.
- `Document Architecture Agent` has the explicitly reported availability
  `available`.
- Every other actor has no availability, capacity, online, idle, or health
  status in this fixture. Omission must remain omission, not an inferred state.

Runtime health, current session activity, chat recency, and an actor's familiar
name are never evidence of availability, assignment, capacity, or ownership.

## Required page slices

Every page has a `page_slices.<page>` entry. `required_refs` is the minimum
canonical data each implementation must query; `required_facts` is the visible
truth the page must preserve. A page may render additional supporting records
only if they are also present in this fixture and do not contradict the slice.

| Expected page | Slice key | Minimum proof |
| --- | --- | --- |
| Company Home | `home` | Human decision needed for CN-2026-018 |
| Docs workspace | `docs-workspace` | Proposed trademark structure and finance relation |
| Document Focus | `document-focus` | Brand A plan links to the trademark work |
| Workboard | `workboard` | Explicit ownership and pending approval state |
| Work Item Focus | `work-item-focus` | Full responsibility, evidence, approval, and finance chain |
| Finance | `finance` | Same ¥3,000 pending commitment, never a payment |
| Organization | `agents-organization` | Mixed actors and explicit-only reported statuses |
| Standing Agent Focus | `standing-agent-focus` | Explicitly reported availability and architecture proposal |
| Governance Proposal | `governance-proposal` | Pending module decision and its effects |
| Approval Focus | `approval-focus` | Named human authorization, evidence, and consequences |
| Business Module Focus | `business-module-focus` | Module joins application, work, finance, people, and knowledge |
| Human Member Focus | `human-member-focus` | Human ownership and approval without agent telemetry |

The JSON is deliberately more exact than the image prompts. If a prompt asks
for copy that conflicts with the fixture, the fixture wins. In particular, the
Business Module Focus prompt's historical phrase “Created from approved Module
Design” must not be presented as a completed fact while this fixture says the
module proposal is pending final approval.

## Validation rules

Before an expected image, browser fixture, or implementation capture is added:

1. Parse `fixtures/company-os-trademark-v1.json` as JSON.
2. Confirm that every `page_slices` key matches the twelve page keys in
   `visual-contract.json`.
3. Confirm every reference in `required_refs` resolves to one fixture record.
4. Confirm every timestamp is in July 2026.
5. Confirm no `financial_records` entry has `type: payment`, and the only
   trademark fee record is the pending ¥3,000 commitment.
6. Confirm the WorkItem's requester, submitter, assignee, owner, reviewer,
   legal reviewer, contributor, and approver resolve to the exact actors above.
7. Confirm status-bearing organization copy is backed by an explicitly
   reported status entry.

These checks protect visual acceptance from accidental data duplication,
silent payment bypasses, conflated responsibility, and invented Agent presence.
