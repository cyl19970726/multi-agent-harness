# Repository Development: Task, Git, PR, And Review

This document projects the Development Playbook onto repository delivery.
Accepted Notion Docs own product intent, one Notion Development Task owns the
current state of a change, Development Documents preserve readable submissions
and immutable Review verdicts, Git owns source history, and GitHub owns Issue,
PR, CI, and merge facts. None is a universal authority.

## Ordinary flow

```text
Brain assigns one Development Task
  -> Dev works in one explicit workspace
  -> Dev submits an exact Git SHA or readable immutable document version
  -> independent Reviewer returns Pass or Changes Required
  -> Pass permits completion of that exact submission
  -> Changes Required returns the same Task to Dev
```

There is no Candidate, Delivery Run, Claim, separate Planner, readiness gate,
Host Gate, or second task ledger in the repository development lifecycle. A
product Agent Team Work record is a separate runtime contract and does not
replace the Development Task or Review.

A GitHub Issue records a problem; it is not a mandatory stage before every
Task. Brain may keep an in-scope finding in the current Task, batch one or more
out-of-scope Issues into a later Task, defer them, or only record them. An Issue
never creates a Task automatically.

## Truth boundaries

| Surface | Owns | Does not own |
| --- | --- | --- |
| Accepted Notion Doc/Spec | current product intent and shared decisions | repository implementation or Task status |
| Development Task | current goal, owner, status, executor binding, submitted version, next action, blocker | transcript or Review verdict |
| Dev Document | directly readable design/submission at a named immutable version | current Task state |
| Review Document | verdict and findings for one exact SHA or document version | implementation or Task-state mutation |
| Git | source history and exact revision identity | product intent or Review acceptance |
| GitHub Issue / PR / CI | problem provenance, diff, checks, and merge facts | Notion Task status or product acceptance |
| Provider Session | native transcript, tools, commands, and turn lifecycle | Task ledger or Review authority |

## Start and isolation

Before editing:

1. Read the Task, accepted Spec/ADR, applicable `AGENTS.md`, and this checkout's
   relevant implementation contracts.
2. Fetch the named remote base when the Task requires it and record the exact
   starting SHA.
3. Inspect the actual worktree and affected paths. Preserve other sessions' and
   users' changes; use an isolated worktree when writable work would collide.
4. Record the real owner, Session/thread binding, workspace, branch, starting
   revision, and first action on the one Development Task.
5. Re-read the Task. If Task and Session/workspace state disagree, stop and
   reconcile them before dispatching or editing.

A Session is an executor, not a claim object. Changing a Notion owner does not
wake a Session; assignment travels through its real execution channel.

## Execution and submission

Dev owns implementation, focused checks, documentation changes, and PR upkeep
inside the Task boundary. Raw command streams, provider transcripts, private
checklists, and subagent internals stay in provider-native records, not Notion.

Submit:

- Task ID and acceptance criteria;
- exact Git SHA, or one named immutable and directly readable Dev Document
  version;
- concise change summary, checks, evidence, risks, and limitations;
- PR/Issue links when relevant.

Large machine-readable inventories and structured manifests belong in the
repository. Review binds them by exact Git SHA, path, and file hash where
useful. Notion explains the decision, schema, and checks; it is not a Base64
transport, object store, or carrier-page assembly protocol.

Any change after submission creates another submission of the same Task and
requires a new Review Document. Previous Review Documents remain immutable.

## Review and completion

Reviewer independently inspects the exact submitted artifact and does not edit
it, merge it, or own Task state. Code Review binds one exact SHA. Document
Review binds one directly readable named immutable version. The Review Document
records `Pass` or `Changes Required`, findings, evidence, reviewer, submission
number, and review time.

On `Changes Required`, Brain preserves the Review and returns the same Task to
Dev. On `Pass`, Brain verifies the completed or merged revision still matches
the reviewed SHA/version, records the repository/merge facts, and marks the
Task Done. CI green or PR merge alone is not Review acceptance.

Reviewer blocking findings must bind to the current Task acceptance and exact
submitted revision. Out-of-scope findings are non-blocking Issue Pool input.
Brain may set a Task to `Cancelled` when its outcome is obsolete, explicitly
superseded, or no longer authorized; cancellation is terminal but is not Pass.

## Coordinator state discipline

Before new dispatch, consume completed and attention-required Session results,
update their Tasks and Development Documents, and release their work slots. A
completed but unconsumed result remains active coordination debt.

If the coordinator cannot state the current owner, bound Session, exact
submission, latest Review verdict, and next action for every active Task, it
stops dispatch and reconstructs those facts from the two Notion tables, Git,
GitHub, and provider-native Session state. Sidebar badges and chat memory are
notifications, not authority.

## Failure semantics

- Do not make a flaky gate green by rerun, timeout inflation, or deleting the
  assertion.
- Do not hide partial append-only success; report the successful prefix and
  exact failure.
- Do not claim live provider or Harness Member behavior from deterministic
  shims.
- When blocked, preserve the exact branch/SHA, clean-or-dirty state, completed
  work, reproducible evidence, primary cause, remaining risk, and next action.

## Required local gates

Use focused checks during implementation. Before delivery, run the checks in
[operations.md](operations.md) on the exact submitted revision. The
repository-owned clean archive gate is:

```bash
pnpm gate:clean-archive
```

It requires a clean committed source tree and pnpm 9.15.4, extracts the exact
SHA, performs the frozen install before runtime tests, then runs Rust,
governance, and JavaScript gates from the archive.
