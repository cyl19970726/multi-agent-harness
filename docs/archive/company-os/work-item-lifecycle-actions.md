# WorkItem Lifecycle Actions

```text
status: canonical Company OS contract
owner_role: product + architecture
implemented_slice: work_item.transition
```

## Purpose

`WorkItem` is append-only Store truth, but a custom page must not use the broad
`work_item.append` authoring command to impersonate execution. Runtime changes
use the declared `work_item.transition` Action. The server loads the latest
WorkItem, validates the transition, responsibility, immutable business context,
result provenance, Approval gates, policy, scope, idempotency, and audit trail,
then appends the accepted next version.

This action changes a company WorkItem. It does not create or accept a Mission,
Wave, AgentTeamRun, WorkflowRun, provider session, Payment, or legal filing.
Those objects retain their native lifecycles and are linked explicitly.

## V1 transition graph

```text
submitted | triaged | accepted | blocked
                    -> in_progress

in_progress          -> blocked | in_review | waiting_for_approval
blocked              -> in_progress
in_review            -> in_progress | waiting_for_approval | completed
waiting_for_approval -> in_progress | blocked | completed
```

Draft authoring, triage, cancellation, archive, reassignment, and reopening a
completed WorkItem are intentionally outside this action. They need separate
commands and authority rules instead of hidden branches in one transition.

## Responsibility

- entering `in_progress`, `blocked`, `in_review`, or `waiting_for_approval`
  requires the requesting Actor to be the accountable owner or an assignee;
- completing work requires the accountable owner or named reviewer;
- every requester must be active and hold `company.work.execute`;
- a named ActorRef is attribution. The current browser capability is still a
  local operator credential, not final actor-bound authentication.

## Immutable and append-only fields

A transition cannot change title, objective, source provenance, submitter,
requester, accountable owner, assignees, contributors, reviewer, approver,
execution mode, Approval references, creation time, due date, priority, or risk.
Result records, evidence, artifacts, and execution references may only grow.

Entering `in_review` requires a durable outcome summary, a result destination,
and evidence or artifacts. Completion additionally requires `completed_at`. If
the WorkItem names Approval references, every named Approval must exist and be
approved before completion. Approval does not cause completion automatically.

## Command contract

```text
command_name: work_item.transition
subject_ref: { kind: work_item, id: <same WorkItem id> }
required_permission: company.work.execute
risk_tier: r2
requires_human_approval: false
effect: transition_state
payload.definition_id: declaring CustomPageDefinition
payload.record: complete next WorkItem version
```

The browser reuses the existing session-only capability transport. Failed
requests do not append a WorkItem. Retrying the exact same ActionCommand id and
body returns an idempotent replay; reusing the id for another transition is a
conflict.

## Trademark acceptance

The Store-live trademark scenario proves a named Trademark Agent can submit the
prepared result for review, a premature completion is denied while the related
Approval is requested, and the accountable Human can complete it only after the
Approval is approved. The completed WorkItem retains its source document,
result document/record, evidence, responsibility, and audit lineage. No Payment
is created by any WorkItem transition.
