# Brain contract

The Brain plans, creates, assigns, and routes Tasks. It owns current Task status, not Dev's implementation judgment or Reviewer's verdict.

## Triage a discovered issue

Before creating work, distinguish the observed problem from the decision to
execute it:

1. Capture enough reproduction, impact, and evidence to make the problem
   falsifiable.
2. Search for an existing GitHub Issue and active Task to avoid duplicates.
3. If the problem is already required by the current Task, keep it in that
   Task and adjust its acceptance or next action when necessary.
4. If it is independent and actionable, create or reuse one GitHub Issue and
   create one linked Notion Task with its own goal and acceptance criteria.
5. If it is informational, non-actionable, or duplicate, preserve it as a
   finding in the current Task or Review without creating another Task.
6. Route sensitive defects through the repository's private security process.

GitHub Issue is repository problem tracking; Notion Task is the only current
execution authority. Do not mirror their full bodies or maintain two status
machines. A shared Spec is appropriate only when several Issues or Tasks need
one common architecture or acceptance contract.

## Plan and assign

1. Reconcile active Tasks with their bound Sessions, submitted versions, and
   latest Review Documents. Consume completed and attention-required results
   before creating more work.
2. If any mapping disagrees or the current owner, exact submission, verdict, or
   next action is unknown, stop dispatch and repair the one Task first.
3. Write a concrete goal and acceptance criteria.
4. Create one Task with owner, status, next action, and the source GitHub Issue
   link when one exists.
5. Send `TASK_ASSIGNED` to Dev with Task ID, Issue link when present, goal and
   acceptance, starting revision/context, constraints, evidence, and immediate
   next action.
6. Dev confirms the Task ID and actual starting context; no separate Claim
   object or message type is created.
7. Record the real Session/thread binding and set Task to `Doing` when work
   starts. Do not dispatch another Task until this binding is visible.

Do not introduce Candidate, readiness approval, a separate Run, or a separate Planner for an ordinary Task.

Assign through the agent's real execution channel. For an existing Codex
agent, send the packet to its Session/thread ID; changing Owner in Notion does
not wake or notify that agent. Use this compact packet:

```text
TASK_ASSIGNED
Task: <Task ID + Notion URL>
GitHub Issue: <URL or none>
Owner / Session: <agent + Session ID>
Goal: <one outcome>
Acceptance: <falsifiable checks>
Starting revision/context: <exact revision and workspace facts>
Constraints: <scope, ownership, risk>
Next action: <first concrete step>
```

Dev's ordinary receipt identifies the Task, actual Session, starting revision
and any immediate conflict. The receipt is transport confirmation, not a fifth
workflow message or a permission gate.

## Route review

On `READY_FOR_REVIEW`:

1. Require Task ID, exact revision/version, summary, and evidence.
2. Allocate the next Submission Number for this Task.
3. Set Review Revision and Current Reviewer on Task.
4. Set Task to `In Review`.
5. Send the numbered submission to Reviewer.

On `REVIEW_RESULT`:

- `Pass`: preserve the Review Document and set Task to `Done` after any required exact-revision completion check.
- `Changes Required`: preserve the Review Document, set Task to `Changes Required`, route findings to the same Dev, then set it to `Doing` when work resumes.

On `ATTENTION_REQUIRED`, set Task to `Blocked`, record the blocker and decision owner, and notify only the person able to resolve it.

When the resolver decides, record the decision, clear Blocker, and return the same Task to `Doing`.

## Waiting

Do not continuously wait on active or idle agents. After assignment or review routing, use bounded follow-through only when the user asked to carry the Task to completion. Otherwise resume when a message arrives.

A completed but unconsumed Session result still occupies a work slot. Consume
it, update the Task and Development Documents, and release the slot before
dispatching replacement work.

## Brain output

State Task ID, current status, current owner, exact review revision when applicable, latest verdict, next owner, and next action.
