# Agent Dashboard Read Model

The Agent Dashboard read model turns canonical harness objects into an
operator-facing control plane. It is a projection, not the source of truth.

## Source Boundary

```text
Harness store / CLI / API
  -> DashboardSnapshot
  -> read model selectors
  -> advisory warnings
  -> React panels
```

The read model may filter, group, count, and link objects. It must not create
canonical task assignment, delivery, evidence, proposal, review, or decision
state.

Append-only objects that represent mutable state, such as `Message` delivery
updates, must be projected as the latest row per object id before Dashboard
warnings are computed. Otherwise stale rows such as an old `queued` assignment
can create false warnings after a later `acknowledged` row exists.

## Goal Scope

The default product unit is the selected goal:

```text
selected goal
  -> tasks where task.goal_id matches
  -> members referenced by owner / assignee / reviewer / messages / sessions
  -> teams containing those members
  -> warnings linked to the goal, task, member, proposal, decision, or session
```

This keeps the Dashboard focused on one operational workflow instead of a raw
repository-wide object dump.

## Detail Panels

Task detail must show the evidence chain that proves work happened:

```text
task
  -> assignment proof: Message(kind=task)
  -> reports: Message(kind=report)
  -> provider sessions and child threads
  -> evidence refs
  -> proposal
  -> review / critic evidence
  -> decision
```

Observer proposal detail must show the autonomous loop that creates future
work:

```text
Evidence(source_type=goal_proposal|graph_change_proposal|blocker|follow_up)
  -> proposal Message from Observer/member to Lead
  -> linked evidence such as next_round_plan
  -> Lead Decision disposition
  -> follow-up task ids and goal ids
```

Member detail must show whether an `AgentMember` is a durable harness actor:

```text
member
  -> inbox / outbox
  -> runtime health
  -> current task / proposal
  -> provider sessions
  -> child threads
```

## Warning Promotion

Warnings in `apps/agent-dashboard/src/warnings.ts` are advisory until they are
promoted.

Promotion rule:

```text
read-model warning
  -> stable failure mode
  -> Rust validation / CLI gate / review gate / CI check
  -> Dashboard displays canonical result
```

Do not let a UI-only warning become an invisible acceptance rule. If a warning
blocks a goal, move the rule to a canonical surface first.

## Current Advisory Warnings

| Warning | Meaning |
| --- | --- |
| `fake_assignment_risk` | Task has an assignee but no task assignment message. |
| `assignment_not_delivered` | Task assignment exists but has not reached delivered or acknowledged. |
| `missing_report` | Task is in review or done without a report message. |
| `failed_delivery` | Message delivery failed. |
| `queued_task_message` | Task assignment is still queued. |
| `closed_member_pending_delivery` | Message is queued or claimed for a member that cannot receive normal delivery. |
| `claimed_delivery_pending` | Message has a provider-session claim and waits for terminal reconciliation. |
| `provider_session_blocks_delivery` | Running or queued provider session should block later normal delivery. |
| `failed_provider_session` | Provider session failed or was canceled. |
| `unresolved_provider_session` | Provider turn was accepted but still lacks terminal reconciliation. |
| `provider_only_claim` | Provider session exists without a harness report message. |
| `proposal_missing_evidence` | Submitted or accepted proposal has no evidence refs. |
| `proposal_bad_evidence_ref` | Proposal references evidence missing from the snapshot. |
| `owned_path_violation` | Proposal changed paths outside task ownership. |
| `decision_missing_evidence` | Decision has no evidence refs. |
| `goal_learning_gap` | Goal learning status reports a missing workflow link. |

## Implementation Files

| File | Role |
| --- | --- |
| `apps/agent-dashboard/src/types.ts` | Snapshot and UI object types. |
| `apps/agent-dashboard/src/readModel.ts` | Selectors, goal scope, member/team/task grouping. |
| `apps/agent-dashboard/src/warnings.ts` | Advisory warning derivation. |
| `apps/agent-dashboard/src/components/*` | Display and navigation only. |

## Current Autonomous Projection

The Rust snapshot includes `autonomous_proposals`, a Dashboard-only projection
derived from canonical `Evidence`, `Message`, `Decision`, and `Task` records.
It is intentionally not a new stable schema yet.

Current proposal source types:

```text
goal_proposal
graph_change_proposal
blocker
follow_up
```

`next_round_plan` is supporting evidence linked through the proposal message.
The projection adds convenience fields such as source member, target Lead,
disposition, linked evidence ids, and follow-up task/goal ids. If these fields
become required by gates outside the Dashboard, they should graduate into a
versioned schema or explicit CLI/API contract.
