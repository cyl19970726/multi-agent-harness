# Work Operating System

```text
status: canonical product contract; native implementation incomplete
owner_role: product
canonical_for: company-wide Work information architecture, views, filters, and Milestone boundary
```

## Product responsibility

`Work` is the company-wide operating ledger for commitments. It answers four
questions without making the operator reconstruct them from chats or execution
logs:

1. What work exists across every business line?
2. Who is accountable, who is executing, and who must review or approve?
3. What is complete, in motion, blocked, waiting, overdue, or unassigned?
4. Which document created the work, how is it being executed, and where will
   the durable result return?

Every Work surface is a projection over the same native `WorkItem` records.
Board cards, table rows, workload cells, Milestone summaries, document embeds,
and actor assignments never become duplicate task objects.

## Deliberately small hierarchy

```text
Work
  -> optional Milestone
       -> WorkItem
```

There is no `Project` object and no task graph. A business line is represented
by `BusinessModule` or another explicit business relation; it is a grouping and
filter dimension, not another task container.

`Milestone` is a durable business checkpoint such as “Trademark application
submitted” or “V1 released”. It groups WorkItems around an outcome and target
date. `Mission -> Wave` is an optional execution plan for long-running work and
remains outside this hierarchy. A WorkItem can link to a Mission, Wave,
AgentTeamRun, WorkflowRun, Host execution, Git Issue, or Pull Request through
typed execution and delivery references.

## Primary navigation

The top-level Work page has six stable data views:

| View | Operator question | Default representation |
| --- | --- | --- |
| Overview | What needs attention and where is progress drifting? | operating summary plus attention queues |
| Board | How is work flowing through states? | status Kanban |
| All Work | What is the complete, sortable ledger? | dense table |
| Milestones | Which business outcomes are on track or at risk? | roadmap and grouped work |
| Timeline | What is due when and which dates collide? | chronological schedule |
| Workload | Who owns what and where is capacity or ownership unhealthy? | actor lanes and capacity summary |

Saved views are named query presets, not new pages or stores: `My Work`,
`Agent Work`, `Human Actions`, `Waiting for Approval`, `Blocked`, `Due Soon`,
`Completed`, and `Unassigned`.

## Shared dimensions

All primary and saved views use the same filter vocabulary:

- business line / `BusinessModule`;
- `work_type`;
- status;
- accountable owner;
- assignee actor and actor kind;
- Milestone;
- approval state;
- priority, risk, and due range;
- source document and execution mode.

The implemented V1 store does not yet prove every dimension. In particular,
native Milestone persistence and multi-business-line query support remain
planned until schemas, store, API, and browser evidence exist.

## Board contract

The default status workflow is:

```text
Inbox -> Accepted -> In Progress -> In Review -> Completed
                         |              |
                         +-> Blocked    +-> Waiting for Approval
```

The rendering may collapse low-volume states, but stored state is never
rewritten to make a cleaner board. A card prioritizes operational recognition:

- title, Work type, business line, and optional Milestone;
- accountable owner plus active assignee(s), with actor-kind identity;
- due date, priority, and risk;
- Approval pressure or blocker and next required actor;
- source Document and execution mode;
- explicit completion or review evidence when terminal.

Movement between columns invokes the governed lifecycle Action. Dragging a
card is not permission to bypass responsibility, required Approval, result
provenance, or transition rules.

## All Work contract

The table is the highest-density truth surface. It supports grouping by
business line, Milestone, status, accountable owner, assignee, or Work type and
shows at minimum:

```text
WorkItem | Type | Business line | Milestone | Status | Accountable
Assignees | Approval | Due | Source | Execution | Updated
```

The table must support unassigned and no-Milestone rows honestly. It does not
hide completed items by default; operators can choose an active-only saved
view.

## Milestones and workload

A Milestone view shows outcome, accountable owner, target date, acceptance
criteria, progress by WorkItem state, blockers, approval pressure, and the
remaining critical work. Percent complete is derived and labelled; it cannot
replace the acceptance criteria.

Workload groups explicit assignments by actor. It distinguishes accountable
ownership from execution assignment, human from Standing Agent, and temporary
execution members from the durable organization. Capacity is advisory and may
be unknown. An unassigned lane is always visible when applicable.

## WorkItem focus

The detail page preserves the full accountability chain and durable context:
source and result, submitter, requester, accountable owner, assignees,
contributors, reviewer, approver, Milestone, business line, lifecycle history,
Approval, artifacts, evidence, finance relations, and typed execution/delivery
references. Activity may explain what happened but cannot establish ownership
or acceptance by itself.

## Responsive behavior

Desktop keeps the navigation rail, central work surface, and optional context
rail. Tablet collapses the context rail into a drawer. Mobile uses a compact
view switcher and filter sheet; Board becomes horizontally scrollable status
lanes while All Work becomes a readable item list. Responsibility and pressure
must remain visible before secondary metadata.

## Acceptance scenario

The initial multi-business-line target dataset should include at least:

- Brand & IP: trademark search, filing Approval, and filing evidence;
- Content: publish and measure a video campaign;
- Finance: review the ¥3,000 commitment and record the authorized effect;
- Product & Engineering: implement a governed Company OS capability with Git
  delivery references.

This dataset exists to prove cross-line query and responsibility semantics. It
must be clearly labelled expected/design data until native records exist.

