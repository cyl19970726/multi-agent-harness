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
