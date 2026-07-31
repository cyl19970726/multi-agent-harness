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
- description?                         # human/agent readable scope and constraints
- acceptance_criteria[]                # checklist for closure/review
- context_refs[]                       # typed links needed to understand the work
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
- deliverable_refs[]                  # typed links to returned documents, evidence, records, PRs, etc.
- due_at? / priority? / risk_level?
- created_at / updated_at / completed_at?
```

`objective` is the compact intended outcome. `description` is the richer task
brief that explains scope, constraints, and non-goals. It must still be concise
and operational; long rationale belongs in the source Document. A WorkItem
without `description` remains valid for historical rows, but new operational
Work should provide one when the title/objective alone cannot guide an Agent.

`acceptance_criteria` are the review checklist for the WorkItem. They do not
replace Milestone acceptance criteria and they do not authorize sensitive
actions. They answer what must be true before review or closure can be
considered.

`context_refs` are typed links to Documents, TypedRecords, actors, modules,
Milestones, WorkItems, Approvals, Finance records, evidence, or execution
records needed to understand the assignment. They prevent the WorkItem from
becoming a copied mini-document while still giving an Agent enough navigation
context.

`deliverable_refs` are typed links to returned durable outputs. Transitions may
append deliverables; they must not remove prior deliverables. Result
Document/record refs, evidence refs, artifact refs, and deliverable refs can
overlap, but the UI should label their role clearly rather than hiding the
source/result/evidence distinction.

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
Reassignment, owner/reviewer/approver correction, cancellation, archive, and
reopening remain separate future commands so this transition cannot silently
expand its authority. An `Assignment` delivery record may prove that someone
was asked to act, but it does not rewrite `WorkItem.assignees` or repair a
misrouted WorkItem. A dedicated reassignment Action must preserve source,
objective, detail, existing result/evidence provenance, and audit the previous
and next role refs.

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

## Agent-owned WorkItem routing

The normal autonomous-company path is that Agents turn observed gaps into
WorkItems instead of leaving them as private reasoning, chat notes, or custom
page text.

```text
Document / TypedRecord / Gateway event / Git provider observation
  -> Agent detects a gap or next action
  -> WorkItem with source context, owner, assignees, and acceptance criteria
  -> Organization validates that assigned Actors exist and may act
  -> execution happens directly or through Mission/Wave, Agent Team, Workflow,
     host work, external work, or human work
  -> result, evidence, and durable summaries return to Docs and Work
```

Common routing examples:

| Discovery | WorkItem owner/assignee pattern | Boundary |
| --- | --- | --- |
| Docs Governance Agent finds outdated or missing company memory | accountable owner can be Docs Governance; assignee can be Docs Governance, a lower Docs Agent, or a one-time executor | The source Document remains the context; the WorkItem is the commitment. |
| Development Agent finds a codebase improvement | accountable owner can be Development Agent; delivery refs may include Git Issue, branch, PR, checks, and preview | Git does not replace the WorkItem and PR merge does not prove business acceptance. |
| Work Governance Agent sees untriaged work | Work Governance may route or create WorkItems, but assignment must target existing Organization Actors | Reassignment requires an explicit Work action, not a chat mention. |
| Lower Standing Agent needs more capacity | current WorkItem can use temporary execution; recurring gaps become an Org/HR capability proposal | Provider subagents and MemberRuns are not durable Organization actors. |
| Gateway message requires follow-up | gateway service may submit a WorkItem with evidence refs; accountable owner must be a Human, Standing Agent, external participant, or service allowed by policy | Incoming messages are intake evidence, not completed work. |

An Agent may complete a WorkItem itself when the role, permissions, and
acceptance criteria fit. It may also route to an existing lower Standing Agent
or use a temporary subagent inside its execution. Durable delegation is always
expressed by WorkItem roles and Organization records; executor-internal
planning remains separate.

Finance is not part of this core path unless the WorkItem declares a monetary
effect. If the work involves purchase, payout, refund, budget, commitment, or
invoice state, Work records the request and Finance owns the monetary record.

In AgentOS self-hosting, Work is not merely the middle step of a fixed
Docs-to-Work pipeline. A Work blocker may create an Organization capability
proposal; an Org change may create documentation and migration WorkItems; a
Docs audit may create corrective Work. Every WorkItem still requires explicit
source context, accountability, lifecycle, review, and result promotion. See
[AgentOS self-hosting dogfood loop](agentos-self-hosting-loop.md).

## Managed intake, capacity, and parallel execution

New requirements enter one managed company queue. Intake preserves the
requester, source record or message, submission actor, observation time, and
the route by which the requirement reached Company Work. A Supervising
Operator preserves that intake provenance and routes the request once to the
Company Lead; it does not triage, prioritize, assign, or broadcast the request
to every Agent. An explicit emergency runtime control may interrupt active
execution, but it does not reprioritize Company Work. The Company Lead orders
the queue against existing commitments, dependencies, risk, and available
capacity.

That ordering is an operating decision over WorkItems, not a second work
ledger. WorkItems remain the durable intent, acceptance, constraints, risk,
source, and result provenance. The Company Lead owns company-wide triage,
priority, capacity arbitration, replan, and cross-WorkItem resource conflicts;
an accountable Domain Lead owns those decisions inside its delegated domain.
A Runtime Supervisor owns delivery and control for its bound MemberRun, native
session, and writable workspace only. It does not allocate Company capacity or
resolve cross-WorkItem priority. The core does not add a Task Graph, Project
object, or universal resource scheduler for this purpose.

Before routing or starting a lane, the responsible Lead must read back:

- the accountable owner, assignees, reviewer, work type, priority, source,
  context, result destination, and lifecycle state of the existing WorkItem;
- active Assignments and execution references for the same WorkItem and Actor,
  including delivery and acknowledgement truth where supported;
- the Actor's active status, availability, required permission, accepted work
  types, declared assignment capacity, and any exclusive assignment;
- active lanes that own the same repository paths, writable workspace, external
  effect, integration boundary, or other declared shared hotspot; and
- the transition policy, required review or Approval, and the evidence still
  missing for the intended next state.

Missing `assignment_capacity` or accepted-work-type declarations are explicit
unknowns, not proof of unlimited capacity or routing compatibility. A WorkItem
role proves accountability, an Assignment proves durable routing, and a native
execution record proves observed execution; none substitutes for the others.
Runtime availability alone grants neither Company authority nor capacity.

Current implemented Work truth includes durable WorkItem and Assignment
records, explicit Actor and source/result references, public Work Actions, and
the Action policy and permission checks applied by those supported commands.
Native execution and delivery references may be linked after they exist.
Trusted project execution permission is an executor capability ceiling, not
Company authority, delegation, approval, or proof of accepted Work.

Hierarchical `ScopedPermissionGrant` attenuation, atomic budget and concurrency
reservations, a consolidated audit/digest view, a unified Human Decision Queue,
and template-governed child Agent or Team creation remain target-only until
their schemas, policies, service behavior, and acceptance evidence exist. This
contract does not claim that target runtime enforcement is implemented.

The normal autonomy boundary routes Work to the accountable domain Lead, such
as a CTO or Docs Lead. With the current interface, that Lead may route one
durable Assignment and use an available Agent Team within configured Company
and execution boundaries; this does not imply a scoped grant or autonomous
template-governed child creation. Developer, Reviewer, QA, or other execution
Members need not report each child step through every Organization layer.
Child outcomes, evidence, blockers, and delivery references update the
originating WorkItem directly through governed actions. Raw transcripts and
private reasoning remain provider-native.

Supported mutations produce their implemented Action and audit records; a
consolidated digest is still target-only. Routine in-scope execution does not
wait for synchronous Human notification or approval. Material finance, legal,
root-security, destructive, major-public, permission-changing, policy-unknown,
or scope-expanding effects use the current governed Approval or escalation
path. A unified Human Decision Queue remains target-only.

### Minimum parallel Assignment brief

An Assignment carries only the execution brief for its lane. With the current
V1 record, these details live in its stable identity, WorkItem reference,
sender, recipient, role, correlation, and `scope`; implementations may later
add structured fields without changing their meaning:

- the bounded objective and non-goals, plus the originating WorkItem and exact
  correlation used for delivery and handoff;
- owned paths or resources, named shared hotspots, and the rule for surfacing a
  newly discovered conflict;
- the expected deliverable, required checks, evidence form, and the actor who
  integrates or orders overlapping lanes;
- the effective permission, budget, risk, and concurrency ceilings, including
  whether child Agent creation is allowed; and
- any Lead-pinned base SHA, worktree, or branch constraint needed for safety.

A Lead may pin a base, worktree, or branch when a known conflict requires that
safety constraint; these are not mandatory user-authored WorkItem fields.
Otherwise a permitted executor may choose or create an appropriate clean
worktree and branch, then reports the actual base, worktree, branch, commit,
checks, and conflicts in its handoff. The Runtime Supervisor does not choose
Git resources or allocate work across WorkItems. It enforces only the real
single-driver and runtime binding for its MemberRun, native session, and
writable workspace.

### Queue and lifecycle readiness

Queue order never changes lifecycle truth by itself. The operator applies the
smallest public governed action and leaves the current state unchanged when a
required action, authority, or immutable link is unavailable:

| Intended step | Minimum read-back before the governed action |
| --- | --- |
| route or deliver | Existing non-duplicate WorkItem; live compatible Actor; explicit Assignment identity, sender, recipient, scope, and correlation; declared execution and conflict ceilings. |
| `submitted` to `in_progress` | Accountable owner and assignee are valid; the executable scope has been durably routed; source/context are resolvable; required authority and protected-action policy are satisfied. |
| `in_progress` to `in_review` | The bounded execution is terminal; outcome, execution, evidence, artifact, and deliverable references required by acceptance are appended through supported governed paths; result provenance is named without claiming acceptance. |
| `in_review` to `completed` | Reviewer decision, all acceptance criteria, required Approvals, durable outcome summary, and result Document or record return are present and read back. |

Evidence arriving late is appended through its governed evidence/result path;
the WorkItem is never regressed to an earlier status merely to attach it.
Provider completion, a handoff, a successful check, a merged pull request, or
an acknowledged Assignment does not automatically complete Company Work.

If public policy cannot express the next truthful link, the operator records a
blocker and preserves the existing record instead of editing ledgers directly,
inventing an Assignment, or creating a duplicate WorkItem. Related WorkItems
may remain distinct only when their objectives and acceptance boundaries are
distinct and their typed context links make that ownership reconstructable.
Accepted results return to the canonical source/result Documents through an
explicit governed Docs action; context Documents never silently replace the
source.

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
