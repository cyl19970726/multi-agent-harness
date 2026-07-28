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

## Docs page integration

Business Docs pages may show Work panels, milestone boards, assignment status,
or next actions, but Work remains the source of truth for commitments. When a
Docs page contract references Work, require:

- source `Document` or `TypedRecord` ref;
- `WorkItem` id, WorkType, status, priority, Milestone, and business line;
- requester, submitter, accountable owner, assignee, reviewer, and approver
  refs as distinct fields when applicable;
- result document/record refs and evidence refs for completion;
- no copied task status inside prose when a Work record exists.

If a page needs a Work board or task list, prefer a saved View or explicit
related-record panel. Do not let Docs text or a custom page mark work done
without `work_item.transition` / `close` and result provenance.

## Current interface state

Current stable dedicated CLI coverage is strongest for Docs:

```bash
harness company docs query --document <document-id>
harness company docs refs --document <document-id>
harness company docs related --record <typed-record-id>
```

Work records and Work projections exist through the Company OS Store/API and
governed Action path. Dedicated `harness company work ...` commands are
implemented for the first native operating slice: list, query, create, assign,
transition, close, and baseline Milestone lifecycle.

Use:

```bash
harness company work query --work-item <work-item-id>
harness company work list --module <business-module-id> --milestone <milestone-id>
harness company work create \
  --definition <custom-page-definition-id> \
  --source-document <doc-id> \
  --module <business-module-id> \
  --title <title> \
  --objective <objective> \
  --submitted-by <actor-id> \
  --accountable-owner <actor-id> \
  [--assignee <actor-id> ...]
harness company work assign \
  --definition <custom-page-definition-id> \
  --work-item <work-item-id> \
  --assignee <actor-id> \
  --assigned-by <actor-id>
harness company work transition \
  --definition <custom-page-definition-id> \
  --work-item <work-item-id> \
  --status <in_progress|blocked|waiting_for_approval|in_review|completed> \
  --actor <actor-id>
harness company work close \
  --definition <custom-page-definition-id> \
  --work-item <work-item-id> \
  --actor <actor-id>
harness company work milestone list
harness company work milestone show --milestone <milestone-id>
harness company work milestone create \
  --authority <human-admin-id> \
  --id <milestone-id> \
  --title <title> \
  --outcome <outcome> \
  --accountable-owner <actor-id>
harness company work milestone update --authority <human-admin-id> --milestone <milestone-id> --status <planned|active|at_risk|achieved|cancelled|archived>
harness company work milestone close --authority <human-admin-id> --milestone <milestone-id>
```

`list` and `query` are read-only. Writes require
`HARNESS_COMPANY_OS_TOKEN`, a matching `CustomPageDefinition`, and an Action
policy for `work_item.append`, `assignment.append`, or
`work_item.transition`.

`work assign` appends a native `Assignment` delivery record. It does not
rewrite `WorkItem.assignees` in v1 because current `work_item.transition`
correctly forbids changing responsibility fields. Set initial assignees during
`work create`; add a later explicit assignment-update Action if the product
needs reassignment to affect the Work projection.

## Safe workflow

1. Inspect source truth first. Use Docs query/refs when the work starts from a
   Document or TypedRecord. Prefer native Store/API projection reads over UI
   screenshots or fixtures.
   If the request comes from a business page, inspect the page contract to know
   where the result must return and which right-rail Work panel should show it.
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
- Any Docs page that shows the WorkItem does so through Work refs/View data, not
  copied prose.

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
