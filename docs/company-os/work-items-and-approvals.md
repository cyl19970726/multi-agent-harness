# WorkItems and Approvals

```text
status: canonical Company OS contract
owner_role: product
canonical_for: document-originated work, responsibility, execution references, approvals, and task projections
```

## Purpose

A `WorkItem` is the durable business commitment that connects a source document
or typed record to accountable actors, execution, results, and review. It makes
clear what the company intends to do, who entered it, who asked for it, who owns
the outcome, and where the result returns.

It is not an ordinary message or an inferred agent activity entry. Mission/Wave,
Dynamic Workflow, Agent Team, host execution,
and human work remain ways to perform work; each can be linked as an execution
reference without absorbing the WorkItem's company context or responsibility.

`Work` is the company-wide work ledger. Its only durable grouping above a
WorkItem is `Milestone`; there is no separate Project object.

## Milestone contract

```text
Milestone
- id
- title
- outcome
- status = planned | active | at_risk | achieved | cancelled | archived
- accountable_owner: ActorRef
- source_document_ref?
- business_module_ref?
- target_at?
- acceptance_criteria[]
- work_item_refs[]
- created_at / updated_at / achieved_at?
```

A Milestone is a business checkpoint, not an executor stage. It groups the
WorkItems required to achieve one outcome and exposes remaining, blocked,
waiting-for-approval, and completed work. A WorkItem may initially live in the
Work Inbox without a Milestone and be triaged into one later.

Milestone and Wave are deliberately different: Milestone organizes company
work; Wave orders steps inside one optional Mission. Neither is projected into
the other.

## WorkItem contract

```text
WorkItem
- id
- title
- objective
- status = draft | submitted | triaged | accepted | in_progress |
           waiting_for_approval | blocked | in_review | completed |
           cancelled | archived
- source_document_ref                 # required durable source context
- source_record_refs[]                # trademark application, metric, etc.
- milestone_ref?                      # optional business checkpoint
- work_type = development | design | research | content | legal |
              procurement | finance | operations | governance | human_action |
              general
- result_document_ref?                # destination for the durable outcome
- result_record_refs[]
- submitted_by: ActorRef              # actor that formally entered this record
- requested_by: ActorRef?             # original business requester, if known
- accountable_owner: ActorRef         # exactly one active outcome owner
- assignees: ActorRef[]
- contributors: ActorRef[]
- reviewer: ActorRef?
- approver: ActorRef?
- execution_mode = direct | mission_wave | agent_team | dynamic_workflow |
                   host | external | mixed
- execution_refs[]                    # stable references to actual execution
- approval_refs[]
- evidence_refs[] / artifact_refs[]
- due_at? / priority? / risk_level?
- created_at / updated_at / completed_at?
```

The role fields are intentional and must not be collapsed into one assignee:

| Field | Meaning |
| --- | --- |
| `submitted_by` | The Actor who formally created or submitted the WorkItem. It establishes submission provenance. |
| `requested_by` | The originator of the business need, such as a founder, client, or responsible agent. It can differ from the submitter. |
| `accountable_owner` | The single active Actor accountable for a successful outcome and for escalation. This role is required before acceptance. |
| `assignees` | Actors who are explicitly expected to perform work. |
| `contributors` | Actors who supply a bounded contribution without becoming execution owners. |
| `reviewer` | The Actor who evaluates quality or completeness before closure when review is required. |
| `approver` | The authorized Actor who authorizes a policy-gated decision. Approval is not the same as review. |

All actor-valued fields use `ActorRef`, so a human, Standing Agent, external
participant, or service can be represented while retaining actor-type-specific
authority rules. The system must preserve roles even when one Actor temporarily
occupies more than one; UI should display such overlap rather than hiding it.

## Source, submission, and result provenance

Every WorkItem originates from durable company context. `source_document_ref`
identifies the document page or typed business record where the intent and
constraints can be understood. A document is not modified merely because a
conversation mentioned a request.

Submission records include the actor, time, source context, initial role
assignment, and any automation or delegation path. If a Standing Agent converts
an approved document action into a WorkItem, the agent is `submitted_by`; the
person or record that originated the need remains `requested_by`. If a service
submitted it through an integration, the `service` ActorRef is the submitter
and the owning person or agent must remain visible.

Completion requires a durable outcome summary and a result destination. The
system updates the source document, `result_document_ref`, or both through an
explicit document update or linked typed record; it does not replace the source
content with raw execution logs. Artifact, evidence, metric, decision, and
financial-record links remain referentially stable.

Operational state changes use the governed `work_item.transition` Action rather
than broad record authoring. Its implemented V1 state graph, responsibility
rules, immutable fields, Approval completion gate, and browser evidence are
canonicalized in [WorkItem lifecycle actions](work-item-lifecycle-actions.md).
Reassignment, cancellation, archive, and reopening remain separate future
commands so this transition cannot silently expand its authority.

## Execution references and assignments

`execution_refs` answer how an accepted WorkItem was performed. They are
explicit, typed references such as:

```text
ExecutionRef
- kind = direct_human_work | standing_agent_work | external_engagement |
         mission | wave | agent_team_run | member_run | workflow_run |
         workflow_step | host_execution
- ref
- role_in_execution?
- started_at? / ended_at?
- status?
```

A WorkItem can have multiple execution attempts and mixed modes. A retry adds a
new reference and preserves earlier attempts. Linking a `MemberRun` to a
WorkItem requires an explicit source link; matching a member name, role, model,
or time is never sufficient. A provider session proves observed execution
history, not responsibility or acceptance.

An Assignment is an explicit routing/acceptance record between a WorkItem and
an Actor. It can be projected from a WorkItem for an Agent profile/configuration view,
but neither assignment nor WorkItem is inferred from ordinary chat. If work
must be split, create related WorkItems or an executor-native plan with explicit
links while keeping executor-internal planning outside the Company OS record.

## Approval contract

An `Approval` is an auditable authorization request associated with a WorkItem
or typed record. It names the proposed action, authority policy, evidence,
approver(s), decision, and expiry. A comment saying looks good is not an
approval unless it is formally recorded as one.

```text
Approval
- id
- subject_ref                         # WorkItem or typed business record
- action_summary
- requested_by: ActorRef
- required_approver_refs[]
- policy_ref
- status = requested | approved | rejected | expired | cancelled
- decision_note?
- evidence_refs[]
- requested_at / decided_at? / expires_at?
```

Approval rules are driven by organization and module policy. At minimum, the
following must be gateable as a human-only or named-authority decision:

- committing or paying money, changing budget, or accepting an invoice;
- legal filings, contracts, representations, and regulated submissions;
- changes to organization authority, permissions, or external access; and
- any module policy declared high-risk.

An Agent may prepare a request, validate completeness, or recommend an action;
it cannot impersonate a human approver. A `service` cannot approve by virtue of
automation. An external participant can approve only where a policy explicitly
recognizes their contractual authority and still records the required internal
approval path.

While an approval is pending, the WorkItem should be `waiting_for_approval` or
continue only through policy-approved preparation work. Rejection, expiry, or
materially changed scope must be visible to the accountable owner and source
document; it cannot be hidden by an execution retry.

## Example: trademark filing

```text
WorkItem: File Brand A trademark in China
Source document: Brand A / IP / Trademark application CN-001
Submitted by: Trademark Agent
Requested by: Founder
Accountable owner: Brand Owner (human)
Assignees: Trademark Agent, External Lawyer
Reviewer: IP Lead Agent
Approver: Founder
Execution refs: legal-search Wave, external-lawyer engagement
Result document: CN-001 application record
Related records: budget, invoice, payment, filing evidence
```

The official filing fee is a linked financial record, not a manually copied
number in the trademark page. Finance views and the trademark document render
the same budget, commitment, invoice, payment, or refund records. The payment
approval records who requested it, the authorized human decision, the amount,
currency, evidence, and the relation to the application. Completing the filing
updates the WorkItem, application record, financial state, source document, and
evidence links together.

## UI and projection requirements

Docs are the principal entry and return surface for WorkItems:

- a document can display embedded Action/WorkItem blocks, tables, boards,
  timelines, metrics, approvals, and related financial records as live views of
  shared typed data;
- a WorkItem detail must show source, submitter, requester, accountable owner,
  assignees, contributors, reviewer, approver, execution references, approvals,
  result destination, artifacts, evidence, and state history;
- a task/distribution view groups by status, accountable owner, assignee,
  document, Milestone, module, work type, and approval state without making
  duplicate data;
- Work provides Overview, Milestones, All WorkItems, My Work, Agent Work,
  Human Actions, Waiting for Approval, Blocked, and Workload projections over
  the same records;
- agent profiles/configuration and human detail pages show only explicitly linked WorkItems and their
  documented role; the UI must never infer ownership from chat, sessions, or a
  familiar name;
- Needs You highlights the precise required actor, authority, subject,
  financial/legal consequence, due date, and linked evidence;
- users can navigate from a WorkItem to its source and result documents, any
  execution substrate, approval, actor, and related records.

Execution consoles may link back to a WorkItem but must retain their own native
lifecycles: a Wave gate is not an approval, a WorkflowStep is not a company
task, and a TeamMessage assignment is not automatically a business WorkItem.

## Development WorkItems and Git delivery

A development WorkItem remains the company work record. Git Issue, branch,
worktree, commit, pull request, checks, preview, deployment, and release are
typed delivery references and evidence; they do not replace the WorkItem.

```text
Development WorkItem
  -> start: create or link Issue + branch/worktree
  -> execute: direct Agent or optional Mission/Team/Workflow
  -> submit: commits + Pull Request + checks + evidence
  -> deliver: merge
  -> accept: acceptance criteria + product/deployment verification
  -> completed WorkItem
```

Issue closure is not WorkItem completion. Pull Request merge proves delivery to
the target branch, not product acceptance. The WorkItem reaches `completed`
only after declared acceptance criteria and required review, visual, deployment,
or Human gates pass.

The default relationship is one primary Issue per development WorkItem with
zero or more Pull Requests. Existing provider Issues may be imported, but each
integration declares whether Company OS or the provider is authoritative for
each synchronized field and uses stable external references to prevent event
loops. Repository Milestones are delivery-provider groupings and do not become
Company OS Milestones automatically.

Agents may create Issues, branches, commits, and Pull Requests within policy.
They cannot bypass protected branches, required checks, independent review,
Human-required merge gates, or acceptance. The Development WorkItem page
composes objective, acceptance, execution, Git delivery, review, checks,
Expected/Actual visual evidence, preview, deployment, and Activity in one
traceable surface.

## Non-goals and truth boundary

- No requirement that a WorkItem select an executor at intake.
- No automatic conversion of a message, provider transcript, or activity event
  into a submitted request, assignment, review, or approval.
- No ownership inferred from an execution run, provider session, Agent Team
  role name, or document mention.
- No raw provider thinking stored as work evidence, result, review, or
  approval rationale. Thinking remains sanitized, transient live state only.
- No financial, legal, or authority-changing action treated as approved because
  an Agent completed its execution step.
