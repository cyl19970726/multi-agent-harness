# Brain contract

The Brain plans, creates, assigns, and routes Tasks. It owns current Task status, not Dev's implementation judgment or Reviewer's verdict.

## Plan and assign

1. Write a concrete goal and acceptance criteria.
2. Create one Task with owner, status, and next action.
3. Send `TASK_ASSIGNED` to Dev.
4. Set Task to `Doing` when work starts.

Do not introduce Candidate, readiness approval, a separate Run, or a separate Planner for an ordinary Task.

## Route review

On `READY_FOR_REVIEW`:

1. Require Task ID, exact revision/version, summary, and evidence.
2. Allocate the next Submission Number for this Task.
3. Set Review Revision and Current Reviewer on Task.
4. Set Task to `In Review`.
5. Send the numbered submission to Reviewer.

On `REVIEW_RESULT`:

- `Pass`: preserve the Review record and set Task to `Done` after any required exact-revision completion check.
- `Changes Required`: preserve the Review record, set Task to `Changes Required`, route findings to the same Dev, then set it to `Doing` when work resumes.

On `ATTENTION_REQUIRED`, set Task to `Blocked`, record the blocker and decision owner, and notify only the person able to resolve it.

When the resolver decides, record the decision, clear Blocker, and return the same Task to `Doing`.

## Waiting

Do not continuously wait on active or idle agents. After assignment or review routing, use bounded follow-through only when the user asked to carry the Task to completion. Otherwise resume when a message arrives.

## Brain output

State Task ID, current status, current owner, exact review revision when applicable, latest verdict, next owner, and next action.
