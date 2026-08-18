# Agent Team Work

Status: current
Contract: AFM-2026.08.2

## Purpose

`Work` is the single durable unit of accountable execution inside an Agent
Team, and this document is the one canonical Work authority. `TeamWork` is the
explicit-context name for the same object. Global Work (DOC-106) is a read-only
aggregate over these objects and never owns a second task identity. Workspace
placement is not part of Work identity; it belongs to `MemberWorkspaceBinding`
([docs/current/work-workspace.md](../work-workspace.md)).

## Identity and scope

Every Work has:

- stable `id` and monotonic `version`;
- `accountable_team_id` — the durable Team that owns the responsibility;
- `team_run_id` — the run that surfaced the Work, kept for diagnostics and
  history correlation; it never scopes Work identity or responsibility;
- optional parent and prerequisite Work ids;
- title, context Markdown, and completion criteria Markdown;
- priority, claim mode, eligible members, owner AgentMember, assignee
  TeamMembership, and active MemberRun;
- lifecycle axes, gates, artifacts, checks, and GitHub links;
- creation actor and timestamps.

Responsibility hangs off the durable Team, never off a TeamRun: closing,
restarting, or discarding a TeamRun never moves, invalidates, or re-scopes a
Work. `team_id` is a deprecated pre-cutover alias of `accountable_team_id`,
readable through the Rust serde alias and never written by current binaries.

The Work id and revision are preserved across Global Work views, dashboards,
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

Work-linked communication uses the identity-first `Message` fabric:

- assignment and clarification;
- blocker and decision request;
- report and review request;
- request changes and acceptance result;
- handoff with evidence refs.

The source NodeDaemon authenticates and freezes the author identity and source
authority; display names and caller-supplied actor fields never establish
authorship. `MessageSubscription` selects authorized sources, and every
recipient receives its own `CanonicalMessageDelivery` owned by the target
NodeDaemon. Interactive provider questions use the
`provider_interaction_request` Message kind and answers use the correlated
`provider_interaction_response` kind; there is no separate interaction
lifecycle object or permission ledger.

`TeamMessage`, `TeamMessageProjection`, `team_messages.jsonl`, and their
ACK/manual-ACK writers are Legacy read/export only. They are not accepted
current Message or delivery authorities.

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

## Global aggregation

`firm work list` (DOC-106; replaces `firm company work list/query`):

- reads native Work from selected or known execution spaces;
- filters by accountable Team, assignee TeamMembership, phase, condition, resolution, and priority without copying;
- preserves exact ids and revisions;
- routes to durable Team and TeamMembership identifiers;
- fails closed on duplicate Work ids across stores and reports legacy rows pending responsibility migration;
- never falls back to a former Company task ledger.

Canonical assignment binds a TeamMembership with an expected Work version
(`firm team-run work assign --membership-id ...`); responsibility never depends
on an active MemberRun or runtime.

The former Company Milestone `work_refs` join is retired history (DOC-108); no
current object rewrites Work phase, condition, resolution, ownership, report,
or gate state from outside the Work kernel.

## Runtime and delivery separation

These states are independent:

| Plane | Examples |
|---|---|
| Work | phase, condition, resolution, owner, report, gate, decision |
| runtime | MemberRun, provider session, process lifecycle |
| Work delivery | `WorkDelivery`: Work allocation/revision transport only |
| Message delivery | `CanonicalMessageDelivery`: per-recipient queued/routed/claimed/provider-received/acknowledged/failed/expired/invalidated state |
| runtime command | `RuntimeCommand`: fenced provider effects and live controls |
| identity | `AgentMember` identity and its `TeamMembership` participation |

There is no current company-governance plane: Approval, Finance, Docs, and
Milestone were retired by DOC-108 and survive only as export/verify history.
Work review and acceptance on the Work record replace the former generic
Approval object.

Adapters may correlate these planes by exact ids. They must not infer Work
truth from name similarity, Message content/delivery state, provider
completion, a RuntimeCommand result, or any retired company display state. A
Message cannot assign Work or authorize a provider effect; a Work mutation or
CanonicalMessageDelivery transition cannot impersonate authored conversation.

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
- Message tests prove source-authenticated immutable authorship, subscription
  routing, one CanonicalMessageDelivery per recipient, and correlated
  provider-interaction request/response kinds;
- current CLI/API/Dashboard paths read the canonical Message fabric and cannot
  write the Legacy TeamMessage/ACK ledger;
- WorkDelivery, CanonicalMessageDelivery, and RuntimeCommand tests prove that
  none of the three planes can impersonate another;
- the Global Work API/CLI returns a read-only native Work aggregate;
- dashboard shows exact Work ids and independent lifecycle axes;
- the legacy Company task ledgers, actions, routes, and bridge code are
  physically absent;
- focused Work-kernel and workspace checks pass on the same revision.
