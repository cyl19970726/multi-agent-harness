---
name: agentfirm-development-loop
description: Run a simple AgentFirm development workflow in which the project Brain triages actionable repository issues into Tasks, assigns a Task, Dev completes it, and Reviewer either passes it or returns the same Task for more work. Use when recording a discovered defect, creating or linking a GitHub Issue, creating, executing, reviewing, retrying, blocking, or completing development Tasks, and maintaining the matching Notion Task and Review views. There is no Candidate or pre-review readiness gate.
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

## Issue intake is routing, not a third workflow

An Issue describes an observed repository problem. A Task is the Brain's
decision to assign and finish work. Do not create a second Issue lifecycle in
Notion or treat a GitHub Issue as the current execution authority.

When a defect is discovered, the Brain first decides:

- **Already required by the current Task:** record the finding and resolve it
  in that Task.
- **Independent and actionable:** create or reuse one GitHub Issue, then create
  one Notion Task that links it.
- **Duplicate, non-actionable, or informational:** preserve the evidence in the
  current Task or Review; do not create another Task.
- **Sensitive:** use the repository's private security-reporting path instead
  of publishing details in a normal GitHub Issue.

A shared Spec may define architecture and acceptance for several related
Issues and Tasks. The Spec does not become their execution-state authority.
Docs-only, research, and internal planning Tasks do not require a GitHub Issue
unless repository tracking is useful.

## Two objects

### Task

Task is the only current execution authority. Keep execution state to:

- Task ID
- Goal / acceptance criteria
- Owner
- Status
- Next Action
- Blocker
- Working Revision
- Review Revision
- Current Reviewer

A Task may also link its source GitHub Issue as provenance. That link is not a
status, gate, or second writer of Task state. Do not add an `Issue` Task status
or require a Task-kind taxonomy merely to process a defect.

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

`TASK_ASSIGNED` is the complete handoff to an agent. It contains the Task ID,
linked GitHub Issue when present, goal and acceptance criteria, owner, relevant
starting revision/context, constraints, and immediate next action. The agent
confirms receipt and begins the same Task; do not add a separate Claim object,
Run, acceptance ceremony, or message type.

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
