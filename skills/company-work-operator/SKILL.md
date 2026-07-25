---
name: company-work-operator
description: Operate Company OS Work through governed Store/API/Action contracts. Use when a Governance Agent or business Agent needs to inspect, create, route, assign, transition, or close WorkItems and Milestones while preserving Docs, Organization, Finance, and execution truth boundaries.
---

# Company Work Operator

Operate the Company OS Work surface. This skill is a procedural capability, not
product authority. It helps an Agent choose the right governed operation,
prepare safe inputs, and verify native Work records without reintroducing
Project, Task Graph, GoalPhase, or execution-run state as company work.

## Load the contracts

Before proposing or executing a durable Work change, read:

- `docs/company-os/work-items-and-approvals.md`
- `docs/company-os/work-operating-system.md`
- `docs/company-os/implementation-truth-matrix.md`
- `docs/company-os/skill-contracts.md`
- `docs/company-os/governance.md`

When the WorkItem starts from or returns to company memory, also read:

- `docs/company-os/document-system.md`
- `docs/company-os/collaboration-and-agent-work.md`

If repository files, schemas, API code, or acceptance checks conflict with this
skill, the canonical implementation contract wins.

## Operating boundary

Work owns the company's commitments to do something:

- `WorkItem`
- `Milestone`
- `Assignment`
- Work lifecycle/status
- Work-owned Approval links
- source/result provenance for work
- execution references that explain how work ran

Work does not own:

- Docs structure, blocks, typed records, relations, views, or module definitions.
- Organization membership, roles, permissions, or Standing Agent lifecycle.
- Finance commitments, payments, refunds, invoices, or monetary metrics.
- Mission/Wave, Agent Team, Dynamic Workflow, provider-native sessions, or raw
  execution transcripts.

Do not create a `Project` object to group work. In the current Company OS
language, WorkItems may be grouped by Milestone, WorkType, business line,
module, owner, priority, due date, and source document/record.

## Current interface state

Current stable dedicated CLI coverage is strongest for Docs:

```bash
harness company docs query --document <document-id>
harness company docs refs --document <document-id>
harness company docs related --record <typed-record-id>
```

Work records and Work projections exist through the Company OS Store/API and
governed Action path. Until dedicated `harness company work ...` commands are
implemented, use the repository's current API/action contract and record the
gap honestly as `partial` when reporting capability status.

The intended command family is:

```bash
harness company work query --work-item <work-item-id>
harness company work list --business-line <line> --milestone <milestone-id>
harness company work create --source-document <doc-id> --title <title> --owner <actor-id>
harness company work assign --work-item <work-item-id> --assignee <actor-id>
harness company work transition --work-item <work-item-id> --status <status>
harness company work close --work-item <work-item-id> --result-document <doc-id> --evidence <ref>
```

Do not present those commands as implemented until the CLI and acceptance tests
exist.

## Safe workflow

1. Inspect source truth first. Use Docs query/refs when the work starts from a
   Document or TypedRecord. Prefer native Store/API projection reads over UI
   screenshots or fixtures.
2. Decide whether the work is operational, financial, organizational, legal, or
   execution-only. Route cross-system effects to the owning system.
3. Create or update the WorkItem through the governed Company OS Action path.
   The record must preserve source Document/TypedRecord refs and the accountable
   actor.
4. Assign responsibility to a Human, Standing Agent, external collaborator, or
   service that exists in Organization. Do not invent a member from a chat name.
5. Link execution only as an `ExecutionRef` when work actually runs through
   Mission/Wave, Agent Team, Dynamic Workflow, Host execution, Git, or an
   external system. Execution does not replace Work ownership.
6. If money is requested, stop and route to Finance for a Commitment. A
   WorkItem can request a monetary effect, but Finance owns the monetary state.
7. If approval is required, create/request the Approval through the governed
   Work/Approval path. A comment or model answer is not an Approval.
8. On completion, return durable result and evidence to the originating Docs
   record/module. Closing a WorkItem without result provenance is incomplete.

## Validation checklist

- The source Document or TypedRecord exists.
- The WorkItem has a clear title, WorkType, lifecycle status, owner, assignee
  or routing state, and source refs.
- Milestone is used only as a work grouping/lifecycle planning object.
- Any assigned actor exists in Organization and has a compatible role.
- Any financial effect has a linked Finance Commitment, not just text in the
  WorkItem.
- Any required Approval exists and has a real decision actor.
- Execution evidence resolves to the native executor record when execution ran.
- Result/evidence returned to Docs and did not become a duplicate truth.

## Report format

When handing off, state:

- work capability status: `implemented`, `partial`, `planned`, or `design-only`;
- created/updated WorkItem ids;
- source and result refs;
- assigned actor refs;
- approval refs, if any;
- finance refs, if any;
- execution refs, if any;
- remaining system gaps.
