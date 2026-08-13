---
name: agentfirm-development-loop
description: Run a simple AgentFirm task workflow in which the project brain plans and assigns a Task, Dev completes it, and Reviewer either passes it or returns the same Task for more work. Use when creating, executing, reviewing, retrying, blocking, or completing development Tasks, and when maintaining the matching Notion Task and Review views. There is no Candidate or pre-review readiness gate.
---

# AgentFirm Development Loop

Use the smallest workflow that reliably finishes work:

```text
Brain plans and assigns Task
  -> Dev works
  -> Dev sends READY_FOR_REVIEW with an exact revision
  -> Reviewer returns Pass or Changes Required
  -> Pass: Done
  -> Changes Required: same Task returns to Dev
```

## Choose the role

- Brain / coordinator: read [brain.md](references/brain.md).
- Dev / implementer: read [dev.md](references/dev.md).
- Reviewer / critic: read [reviewer.md](references/reviewer.md).
- For Notion changes or audits, also read [notion.md](references/notion.md).

The Brain includes planning. Do not create a separate Planner role unless the user explicitly needs one.

## Two objects

### Task

Task is the only current execution authority. Keep only:

- Task ID
- Goal / acceptance criteria
- Owner
- Status
- Next Action
- Blocker
- Working Revision
- Review Revision
- Current Reviewer

Statuses are:

```text
Planned -> Doing -> In Review -> Done
                         |
                         -> Changes Required -> Doing

Any active state -> Blocked -> Doing
```

### Review

Create one Review record for each submission:

- Task relation
- Submission Number
- exact Review Revision or document version
- Verdict: `Pass` or `Changes Required`
- Findings
- Reviewer
- Reviewed At

Keep old Review records as history. Do not overwrite a failed Review when Dev submits again.

## Four messages

Use only:

1. `TASK_ASSIGNED`
2. `READY_FOR_REVIEW`
3. `REVIEW_RESULT`
4. `ATTENTION_REQUIRED` for a real blocker

Messages need only the Task ID, sender, relevant revision/version, concise content, and useful evidence links. Do not require event ledgers, fingerprints, Run versions, CAS, or canonical JSON in the default workflow.

## Rules that remain strict

- No Candidate state or readiness gate. Dev decides when to submit.
- Code Review binds to one exact SHA. Document Review binds to one named immutable version or snapshot.
- Reviewer does not modify the work being reviewed.
- `Changes Required` continues the same Task; it does not create a successor Task unless scope genuinely changes.
- The Brain routes work and updates Task state. It does not continuously poll idle sessions.
- For code completion or merge, verify the reviewed SHA is still the revision being completed. A mismatch returns the Task to Doing or In Review.

## Keep advanced controls exceptional

Protocol ledgers, JCS fingerprints, concurrent-writer CAS, Host leases, immutable merge authorizations, and recovery state machines are not part of this default Skill. Introduce them only for a demonstrated need such as concurrent coordinators, regulated audit, destructive automation, or unreliable external side effects.

## Completion report

Report the Task, final status, reviewed revision/version, latest Review verdict, evidence, and next action. If review failed, say explicitly that the same Task returned to Dev.
