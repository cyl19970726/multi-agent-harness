---
name: company-work-operator
description: Operate Company OS Work through governed Store/API/Action contracts. Use when a Governance Agent or business Agent needs to inspect, create, route, assign, transition, or close WorkItems and Milestones while preserving Docs, Organization, Finance, and execution truth boundaries.
---

# Company Work Operator

Operate the Company OS Work surface. This skill is a procedural capability, not
product authority. It helps an Agent choose the right governed operation,
prepare safe inputs, and verify native Work records without reintroducing
Project, Task Graph, GoalPhase, or execution-run state as company work.

## Select the Company Store

Before reading or writing Company OS records, identify the Company Store. Prefer
one of:

```bash
harness company current
harness --company <company-id> company work ...
HARNESS_COMPANY=<company-id> harness company work ...
```

If no Company is selected, `harness company ...` falls back to the current
project-derived compatibility Store. Treat that as legacy compatibility, not
the target Agent Company Workspace boundary.

To move legacy Company OS rows into a real Company Store, use
`harness company migrate-from-project --from-project <project-id|path> --id <company-id>`.
It copies only `company_os_*.jsonl`; it does not migrate execution records,
provider sessions, prompts, or runtimes.

## Load the contracts

Before proposing or executing a durable Work change, read:

- `docs/current/company-os/work-items-and-approvals.md`
- `docs/current/company-os/work-operating-system.md`
- `docs/current/company-os/implementation-truth-matrix.md`
- `docs/current/company-os/skill-contracts.md`
- `docs/current/company-os/governance.md`

When the WorkItem starts from or returns to company memory, also read:

- `docs/current/company-os/document-system.md`
- `docs/current/company-os/collaboration-and-agent-work.md`

When software work is sourced from or delivered through GitHub, use
`$connect-github-company-os` to observe and reconcile the external objects.
This Skill still owns the Company WorkItem, acceptance, and result.

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

## Gateway and dogfood intake

External gateways and plugins create Work only when a message or event requires
follow-up. A plugin may expose actions through MCP or a plugin-owned CLI
adapter, and it may sync external state through a connector, but Work remains
the durable commitment layer. A gateway-created WorkItem must still include
source document, business module, WorkType, Milestone when known, requester,
submitter, accountable owner, assignee, priority, and result/evidence return
path. The gateway event itself is evidence/source context, not Work completion.

For Wanchengwanling merchant intake, WeCom messages should route to Merchant
Ops Agent for answerable questions and to Work Governance for follow-up
WorkItems. Finance, Organization, and legal effects remain owned by their
systems.

For social/content plugins, publication, media upload, comment/private-message
reply, profile update, paid promotion, metric sync, and inbox sync are not
separate Work models. They produce or update WorkItems only when there is a
company commitment: prepare a post, review a draft, submit a gated
publication, answer a merchant/customer question, perform a daily retrospective,
or request a paid-promotion approval. Synced account, post, metric, and message
records should be linked as context/evidence rather than copied into the
WorkItem description.

For GitHub delivery, an Issue, PR, commit, check, review, comment, or release is
an external source/delivery fact. Link it through explicit refs and stable
external identity; do not make GitHub labels or PR state the Work lifecycle.
A merged PR and green checks may satisfy delivery criteria, but the accountable
reviewer must still accept the WorkItem and return its result to Docs.

## Continuous intake, triage, and replan

Treat Human requests and Company observations as continuous intake, not as a
batch that waits for a Human to manually operate every step:

```text
Human Principal intent / Docs gap / gateway or GitHub observation
  -> Supervisor preserves requester/source/time identity and routes once
  -> durable source context
  -> Company Lead triage: accept, reject, clarify, deduplicate, or defer
  -> priority and capacity decision
  -> Domain Lead accountability
  -> one bounded Company Assignment and delegated execution
  -> evidence, review, acceptance, and result return
  -> Company Lead replan from changed facts and remaining capacity
```

The Supervisor owns faithful intake delivery and emergency runtime control
mechanics. It does not create, accept, reprioritize, assign, or approve Company
Work. Ordinary intake goes to the Company Lead rather than broadcasting to
every Member or interrupting active execution.

The Company Lead owns Company-wide priority, capacity conflicts, and replan.
The Domain Lead owns delivery decomposition and autonomous delegation inside
its Organization ceiling. Work must preserve the original requester and
submitter, one accountable owner, explicit assignees, acceptance criteria, and
the return location even when several executions or lower Agents contribute.

Human review is exception-driven. Put an item in a Human queue only when its
policy requires a named Human approver, it requests finance/legal/credential/
permission/organization or other protected authority, the source is ambiguous
enough to change the commitment, or no bounded actor can safely proceed.
Do not manufacture Human approvals for ordinary routing or low-risk progress,
and do not let an empty Human queue imply that all Work is complete.

Current implemented truth includes WorkItem requester/submitter/source,
responsibility, priority/risk, `work_item.update`, lifecycle transitions, and
native Assignment delivery. Target support still missing includes a general
Company Lead Inbox / `WorkIntakeEnvelope`, typed intake idempotency and
disposition, requested urgency versus accepted priority, emergency-override
audit, a governed atomic triage Action, and a dedicated capacity/dependency
health model. Use existing fields and Actions honestly; do not claim the target
bridge exists or replace it with a task graph.

An emergency override must be explicit, targeted, policy-checked, and audited
with requester, reason, scope, affected Work/intake, expiry/release condition,
and terminal control acknowledgement. “Urgent” prose is not an override, and
changed delivery timing never bypasses Company Assignment, delegated ceilings,
protected-action Approval, review, or durable triage.

## Current interface state

Current stable dedicated CLI coverage is strongest for Docs:

```bash
harness company docs query --document <document-id>
harness company docs refs --document <document-id>
harness company docs related --record <typed-record-id>
```

Work records and Work projections exist through the Company OS Store/API and
governed Action path. Dedicated `harness company work ...` commands are
implemented for the first native operating slice: list, query, create, update,
assign, transition, close, and baseline Milestone lifecycle.

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
  --description <scope-and-constraints> \
  [--acceptance-criterion <criterion> ...] \
  [--context-ref-json '{"kind":"document","id":"..."}' ...] \
  --submitted-by <actor-id> \
  --accountable-owner <actor-id> \
  [--assignee <actor-id> ...]
harness company work update \
  --definition <custom-page-definition-id> \
  --work-item <work-item-id> \
  --actor <actor-id> \
  [--description <scope-and-constraints>] \
  [--acceptance-criterion <criterion> ...] \
  [--context-ref-json '{"kind":"document","id":"..."}' ...] \
  [--source-document <doc-id>] \
  [--module <business-module-id>] \
  [--work-type <type>] \
  [--accountable-owner <actor-id>] \
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
  [--deliverable-ref-json '{"kind":"evidence","id":"..."}' ...]
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
policy for `work_item.append`, `work_item.update`, `assignment.append`, or
`work_item.transition`.

`work assign` appends a native `Assignment` delivery record. It does not
rewrite `WorkItem.assignees`; it proves routing/delivery. `work update` is the
governed metadata/responsibility correction path for source Document, module,
WorkType, description, acceptance criteria, context refs, accountable owner,
assignees, contributors, reviewer, approver, priority, due date, and risk. It
must not change lifecycle status, result, approval, evidence, artifact, or
execution provenance; use `work transition` / `work close` for lifecycle
movement. Do not use a direct ledger edit or an `Assignment` row to pretend the
WorkItem responsibility chain changed.

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
9. For externally delivered software work, read the remote GitHub object again
   before acceptance and preserve repo, number/SHA, URL, checks/reviews,
   observation time, and connector freshness.
10. Re-read open Work and actor capacity after material delivery, blockage, or
    priority change. Replan through explicit Work updates/assignments; never
    rewrite history or use executor-local plans as the Company queue.

## Validation checklist

- The source Document or TypedRecord exists.
- The WorkItem has a clear title, WorkType, lifecycle status, owner, assignee
  or routing state, source refs, and enough detail for an Agent to execute:
  description when title/objective are insufficient, acceptance criteria for
  review, context refs for navigation, and deliverable refs for returned
  outputs.
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
