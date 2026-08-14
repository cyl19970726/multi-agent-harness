# Agent Team Work

Status: current
Contract: AFM-2026.08.2

## Purpose

`Work` is the single durable unit of accountable execution inside an Agent
Team. `TeamWork` is the explicit-context name for the same object. Company
Work is a read-only aggregate over these objects and never owns a second task
identity.

## Identity and scope

Every Work has:

- stable `id` and monotonic `version`;
- `team_id` and `team_run_id`;
- optional parent and prerequisite Work ids;
- title, context Markdown, and completion criteria Markdown;
- priority, claim mode, eligible members, owner Member, and active MemberRun;
- lifecycle axes, gates, artifacts, checks, and GitHub links;
- creation actor and timestamps.

The Work id and revision are preserved across Company views, dashboards,
messages, reports, evidence, gates, and decisions.

## Lifecycle

```text
phase:      open -> active -> review -> closed
condition:  normal | blocked | on_hold
resolution: accepted | cancelled | failed   # closed only
```

Condition is orthogonal to phase. Blocking or holding Work does not erase
whether it was open, active, or under review. Closing Work clears transient
conditions and records exactly one resolution.

## Ownership and claim

- `host_assign` Work is assigned by the Host.
- `team_claim` Work may be claimed only by an eligible Member.
- owner identity is an AgentMember id; an active execution attempt is a
  MemberRun id.
- releasing or retargeting Work is a native Work operation with optimistic
  revision checks.
- provider session identity never grants ownership by itself.

## Submission and trust chain

Submitting Work creates an immutable `WorkReport` bound to the exact Work id
and revision. Report claims are supported by evidence, artifact refs, check
refs, and provider observations.

```text
WorkReport
  -> Evidence
  -> WorkGateEvaluation
  -> WorkOperationalDecision
  -> WorkEvent
```

Acceptance requires the latest applicable report and all declared blocking
gates. The accountable Member cannot be the accepting reviewer. A provider
process exiting successfully, a delivery receipt, or a chat report is not
acceptance.

## Message boundary

Work-linked communication uses `Message`:

- assignment and clarification;
- blocker and decision request;
- report and review request;
- request changes and acceptance result;
- handoff with evidence refs.

Interactive provider questions are bridged through correlated Message types;
there is no separate interaction lifecycle object or permission ledger.

## Mutation surface

All executable mutations use:

```bash
firm team-run work list|show|create|assign|claim|start|block|resume|release
firm team-run work submit|review|request-changes|accept|cancel|retarget
firm team-run work reconcile-delivery|poll-github-ci
```

Every transition reads the latest version and supplies the expected revision.
Store operations append the Work revision and any condition record, report,
gate evaluation, decision, and event atomically.

## Company aggregation

`firm company work list/query`:

- reads native Work from selected or known execution spaces;
- filters without copying;
- preserves exact ids and revisions;
- returns mutation routes to the owning execution space;
- reports duplicate-id conflicts;
- never falls back to a former Company task ledger.

Milestone `work_refs` point to native Work ids. Milestones do not rewrite Work
phase, condition, resolution, ownership, report, or gate state.

## Runtime and delivery separation

These states are independent:

| Plane | Examples |
|---|---|
| Work | phase, condition, resolution, owner, report, gate, decision |
| runtime | MemberRun, provider session, process lifecycle |
| delivery | queued, provider received, acknowledged, failed |
| organization | Agent Membership identity and authority |
| company governance | Approval, Finance, Docs, Milestone |

Adapters may correlate these planes by exact ids. They must not infer Work
truth from name similarity, provider completion, or Company display state.

## Removed compatibility design

The Unified Work cutover removed:

- the second Company task and Assignment ledgers;
- source pointers joining Company tasks to TeamWork;
- cutover fences and promotion events for dual identity;
- Company task mutation Actions and CLI commands;
- the browser `workItem` route and task-focus page;
- read fallback to old fixture rows.

Historical Company task rows are disposable and unsupported. The cutover does
not migrate, archive, export, interpret, or dual-read them; operators use a
fresh Execution Space for authoritative Work.

## Acceptance

The contract is accepted when:

- Rust types and JSON Schema agree on lifecycle and evidence invariants;
- Store tests prove atomic operations and self-accept rejection;
- Company API/CLI returns a read-only native Work aggregate;
- dashboard shows exact Work ids and independent lifecycle axes;
- old Company task ledgers, actions, routes, and bridge code are physically
  absent;
- focused Company OS and workspace checks pass on the same revision.
