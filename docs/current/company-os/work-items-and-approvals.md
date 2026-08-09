# Unified Work and Approvals

Status: current
Contract: AFM-2026.08.2
Supersedes: the former Company-owned task object and its execution bridge

## Decision

There is one executable work object: native Agent Team `Work`, presented as
`TeamWork` when the team context must be explicit.

Company Work is not another ledger. It is a read-only aggregate over TeamWorks
from one or more execution spaces. It preserves each Work id, revision, team,
team run, owner, lifecycle, evidence, and gate result exactly as stored by the
native Work authority.

```text
Company Work
  = filter + aggregate + route(TeamWork...)
  ≠ duplicate task object
  ≠ dual-write projection
  ≠ lifecycle adapter
```

Milestones and business records may reference Work ids. They do not own or
rewrite Work lifecycle.

## Lifecycle

Work has three independent axes:

| Axis | Values | Meaning |
|---|---|---|
| `phase` | `open`, `active`, `review`, `closed` | Progress through execution |
| `condition` | `normal`, `blocked`, `on_hold` | Current operating condition |
| `resolution` | `accepted`, `cancelled`, `failed` | Why a closed Work ended |

Invariants:

- non-closed Work has no resolution;
- closed Work has exactly one resolution and a normal condition;
- blocking or holding Work does not rewrite its phase;
- resuming Work restores `condition=normal` and preserves phase;
- only the native Work store may perform lifecycle transitions.

## Evidence and acceptance chain

Submission and acceptance are separate:

```text
Work(revision N)
  -> immutable WorkReport(exact Work id + revision)
  -> Evidence
  -> WorkGateEvaluation / Verification
  -> WorkOperationalDecision
  -> WorkEvent
```

Rules:

- a report is immutable and binds the exact submitted revision;
- acceptance evaluates the latest valid report;
- the accountable member cannot self-accept;
- provider completion is runtime truth, not Work acceptance;
- delivery success is transport truth, not Work acceptance;
- a changed Work revision invalidates evidence that was bound to an earlier
  revision unless a new evaluation explicitly adopts it.

## Approval boundary

Approval remains a separate governed Company object for human or policy
decisions. Approval never acts as a second Work lifecycle:

- Approval can authorize finance, legal, organization, or document actions;
- a Work may wait for an Approval without changing object identity;
- an approved action may produce evidence used by a Work gate;
- Approval does not silently accept, close, or reassign Work.

## Company read model

The Company Work projection returns:

- `authority: "team_work"`;
- `read_only: true`;
- raw `works` with unchanged ids and revisions;
- aggregate summary and board dimensions;
- a route for each unambiguous Work back to its execution space;
- an explicit conflict when the same Work id appears in multiple spaces.

An empty aggregate stays empty. It must not fall back to fixture rows or a
retired Company task ledger.

## Mutation routes

All Work mutations use the native command surface:

```bash
harness team-run work create --team-run-id <id> --title <text> --completion-criteria <text>
harness team-run work assign --team-run-id <id> --work-id <id> ...
harness team-run work start --team-run-id <id> --work-id <id> ...
harness team-run work block --team-run-id <id> --work-id <id> ...
harness team-run work resume --team-run-id <id> --work-id <id> ...
harness team-run work submit --team-run-id <id> --work-id <id> ...
harness team-run work review --team-run-id <id> --work-id <id> ...
harness team-run work accept --team-run-id <id> --work-id <id> ...
harness team-run work cancel --team-run-id <id> --work-id <id> ...
```

Company commands are read, filter, aggregate, and Milestone operations only.
Former Company task creation, update, assignment, transition, and close
commands are intentionally rejected with a route to `team-run work`.

## No compatibility bridge

The cutover has no dual-write or read fallback:

- no Company task ledger;
- no Company Assignment ledger;
- no source pointer joining two task identities;
- no execution-chain bridge that pretends Company acceptance controls native
  Work;
- no browser action that creates a corrective Company task.

Legacy Company task data is disposable and unsupported. There is no migration,
archival workflow, compatibility reader, or fallback; use a fresh Execution
Space for authoritative Work.
