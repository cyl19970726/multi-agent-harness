---
name: agentfirm-development-loop
description: Run a simple AgentFirm development workflow in which the project Brain triages actionable repository issues into Tasks, assigns a Task, Dev completes it, and Reviewer either passes it or returns the same Task for more work. Use when recording a discovered defect, creating or linking a GitHub Issue, creating, executing, reviewing, retrying, blocking, or completing development Tasks, and maintaining the two Notion tables: Development Tasks and Development Documents. There is no Candidate or pre-review readiness gate.
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

This loop operates inside a four-layer system:

```text
Intent -> Task -> Acceptance -> Issue Pool -> Brain triage
```

Notion Specs own accepted intent, one Task owns current execution, exact-version
Review/CI/Spec dogfood own acceptance evidence, and GitHub Issues are cheap
feedback intake. Repository code, schemas, and tests own merged shipped truth;
the Implementation Crosswalk maps intent to that truth without becoming a
second Task ledger.

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
- **Out of scope and actionable:** create or reuse one GitHub Issue. Brain may
  batch it with other Issues into a later Task, defer it, or only record it; an
  Issue does not automatically create a Task.
- **Duplicate, non-actionable, or informational:** preserve the evidence in the
  current Task or Review; do not create another Task.
- **Sensitive:** use the repository's private security-reporting path instead
  of publishing details in a normal GitHub Issue.

A shared Spec may define architecture and acceptance for several related
Issues and Tasks. The Spec does not become their execution-state authority.
Docs-only, research, and internal planning Tasks do not require a GitHub Issue
unless repository tracking is useful.

## Two Notion tables

### Development Tasks

Task is the only current execution authority. Keep execution state to:

- Task ID
- Goal / acceptance criteria
- Owner
- Current Session / executor
- Status
- Next Action
- Blocker
- Working Revision
- Review Revision
- Current Reviewer

A Task may also link its source GitHub Issue as provenance. That link is not a
status, gate, or second writer of Task state. Do not add an `Issue` Task status
or require a Task-kind taxonomy merely to process a defect.

A Session/thread id is an execution binding only. The Session owns its native
transcript and work, but it never owns Task status, submission identity, or a
Review verdict. The Brain writes the current Session binding to the Task and
removes or replaces it when execution moves.

Statuses are:

```text
Planned -> Doing -> In Review -> Done
                         |
                         -> Changes Required -> Doing

Any active state -> Blocked -> Doing
Any active state -> Cancelled
```

`Cancelled` is terminal and means the outcome became obsolete, was explicitly
superseded, or is no longer authorized. It is not a successful Review verdict.

### Development Documents

Use the existing Development Documents table for two ordinary document types:

- **Dev Document / Spec:** a human-readable submission, with a named immutable
  version when it is reviewed;
- **Review Document:** the immutable verdict and findings for one submission.

A Review Document records:

- Task relation
- Submission Number
- exact Git SHA or directly readable immutable Dev Document version
- Verdict: `Pass` or `Changes Required`
- Findings
- Reviewer
- Reviewed At

Keep prior Dev and Review Documents as history. Do not overwrite a reviewed
Dev Document or failed Review when Dev submits again. Do not create a third
table for submissions, snapshots, payloads, protocol events, or Session state.

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

## Reconcile before dispatch

At the start of every routing cycle, the Brain first consumes completed or
attention-required results and compares the Task with its bound Session,
submitted revision/version, and latest Review Document. A completed result that
has not been consumed still occupies a work slot.

If those records disagree or the Brain cannot answer the current owner, exact
submission, latest verdict, and next action, it stops new dispatch. Reconstruct
the facts from the Task, Development Documents, Git, and provider-native
Session state; then update the one Task before assigning more work. A sidebar
badge, chat memory, or continuous polling is not a control plane.

## Rules that remain strict

- No Candidate state or readiness gate. Dev decides when to submit.
- Code Review binds to one exact SHA. Document Review binds to one named,
  immutable, directly readable version.
- Reviewer does not modify the work being reviewed.
- Reviewer blocking findings bind to the submitted revision and the current
  Task acceptance. Out-of-scope findings are non-blocking feedback for the
  Issue Pool.
- `Changes Required` continues the same Task; it does not create a successor Task unless scope genuinely changes.
- The Brain routes work and updates Task state. It does not continuously poll idle sessions.
- For code completion or merge, verify the reviewed SHA is still the revision being completed. A mismatch returns the Task to Doing or In Review.

## Observer and escalation

Observer audits the trajectory, not the artifact. On cadence for long work,
and whenever a repair chain exceeds two links, instructions are repeatedly
restated, or the user expresses dissatisfaction, it asks whether the goal,
method, encountered problems, drift, and remaining distance still make sense.
Its verdict is continue / intervene / escalate / stop. Observer never edits the
work, updates Task state, or replaces Reviewer. Escalate scope trade-offs,
architecture authority, and risk acceptance to the Human with evidence and
options attached.

## Keep review material readable

Reviewers inspect the submitted artifact directly. Do not encode a document as
Base64, split it into carrier pages, or require payload assembly, sorting, or
decoding as the default submission path. Transport integrity is not design or
code review.

Large machine-oriented inventories and structured manifests belong in the
repository. Bind them with an exact Git SHA, path, and file hash when useful;
the Notion Dev Document explains their purpose, schema, decision, and checks
without reproducing the dataset.

## Desired before applied

Keep accepted intent separate from the behavior supported by the checked-out
revision:

1. **Before intent is accepted:** draft target text is `Proposed` or
   `PLANNED TARGET — NON-OPERATIVE`; current executable constraints still win.
2. **Accepted target, implementation not landed:** record the Crosswalk delta
   and one owning Task. Plan toward the accepted target while obeying the
   current checkout and failing closed on unsupported or unsafe effects.
3. **Coordinated cutover:** change code/schema/config, affected repository docs,
   applicable `AGENTS.md`, and this procedural projection in one reviewed
   semantic slice. Mixed or uncertain revisions use the stricter safe
   intersection.
4. **Exact revision applied:** remove target-only labels only in the revision
   that implements them. `Aligned` or `Verified` requires named evidence at the
   exact SHA; merge or release alone is not Review acceptance.
5. **Rollback or discovered drift:** obey the actual checked-out/deployed
   revision, mark the Crosswalk relationship Drift/Unknown and evidence
   Stale/Failed as appropriate, preserve prior proof, and route one Task to
   repair the gap.

Before target-dependent work, identify the actual revision and capability.
Skills choose procedure; they do not decide product truth, widen permissions,
or simulate support that the revision does not have.

## Keep advanced controls exceptional

Protocol ledgers, JCS fingerprints, concurrent-writer CAS, Host leases, immutable merge authorizations, and recovery state machines are not part of this default Skill. Introduce them only for a demonstrated need such as concurrent coordinators, regulated audit, destructive automation, or unreliable external side effects.

## Completion report

Report the Task, final status, reviewed revision/version, latest Review verdict, evidence, and next action. If review failed, say explicitly that the same Task returned to Dev.
