# Development system pattern

## Default to one Task authority

The default development system is deliberately small:

```text
Brain assigns Task
  -> Dev works freely
  -> READY_FOR_REVIEW at one exact revision
  -> Reviewer writes one immutable Review
  -> Pass: Task Done
  -> Changes Required: same Task returns to Doing
```

Use one Task database as the current authority. Do not require a second Run,
Candidate, readiness gate, event ledger, or merge-authorization state machine.
Add an advanced execution-attempt object only when a demonstrated operational
need cannot be represented by Task plus immutable Review history.

## Task and Review responsibilities

| Authority | Owns | Must not own |
|---|---|---|
| Development Task | goal/acceptance, owner, status, next action, blocker, working revision, review revision, current reviewer | repeated editable copies of review history |
| Development Review | Task relation, submission number, exact reviewed revision, verdict, findings, reviewer, reviewed time | current Task routing or mutable execution state |
| Development Document | typed Specification or durable supporting material | duplicate Task status or a hidden execution lifecycle |

Task statuses are: `Planned`, `Doing`, `In Review`, `Changes Required`,
`Blocked`, `Done`, and `Cancelled`. `Cancelled` is the non-success terminal
state for an obsolete, explicitly superseded, or no-longer-authorized outcome;
ordinary current views exclude both terminal states.

## Review history

Brain allocates the next submission number when routing `READY_FOR_REVIEW`.
Every submission receives one Review record. Code review binds to the exact
review revision. `Changes Required` preserves the Review, clears the active
review routing, and returns the same Task to `Doing`; it does not create a new
Task or mandatory successor Run.

Review findings may link to GitHub, checks, evidence, and a Specification. Do
not split a normal submission into Snapshot and Result databases.

## Build the Task page as a cockpit

The default Task page should answer:

1. What outcome and acceptance criteria govern this Task?
2. Who owns it and what is its current status?
3. What happens next, or what is blocking it?
4. Which Specification and GitHub Issue/PR are related?
5. What immutable Review history exists?

Use a filtered linked Review view rather than copied review tables or generic
`Related pages` sections. Relations should be named by meaning: `Task`,
`Specification`, `Reviews`, and `GitHub Issue`.

## Keep execution activity at its source

The Codex/provider Session is source truth for activity; GitHub is source truth
for commits, CI, PR, and merge. Notion records semantic checkpoints, not every
commit or provider event. Working revision is the last reported exact revision,
not a readiness permission.

## Avoid common collapses

Reject designs that:

- make Task and Run mandatory editable authorities for the same work;
- introduce Candidate or pre-work readiness as a permission gate;
- put Specification, execution journal, and Review into one mega-page;
- duplicate current status, blocker, revision, CI, or next action;
- use generic page relations or URL fields instead of semantic relations;
- copy provider/session activity into Notion as a scheduler ledger.
