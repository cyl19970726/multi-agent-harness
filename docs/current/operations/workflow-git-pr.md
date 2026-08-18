# Repository Development: Spec, Git, PR, And Review

This document defines the repository delivery contract. Notion owns product
intent and the canonical implementation Spec; GitHub owns executable issue and
delivery facts; Git owns source history. Product TeamWork acceptance remains a
separate runtime contract and is not replaced by developer self-review.

## Canonical Flow

```text
Notion discussion
  -> frozen implementation Spec
  -> one umbrella GitHub Issue
  -> one Primary Codex Session claims the Development Record
  -> clean isolated worktree and one integration branch
  -> implementation and focused checks
  -> one PR linked to Spec and Issue
  -> Completion Report on the frozen Candidate SHA
  -> final-SHA self-review and required CI
  -> narrow Host Gate when required
  -> merge
  -> Notion closeout and Issue closure
```

There is one accountable owner for a development Wave. Internal checkpoints
and temporary Sub-Agents are implementation details, not separately claimable
repository tasks. Notion cannot launch a local Codex Session; the Session is
started externally and records its own active Session id during claim.

Here **development Wave** is only the established name for one repository
delivery batch. It is not the retired runtime `Wave` structure and creates no
lifecycle, advance action, or gate on any current object.

## Truth Boundaries

| Surface | Owns | Does not own |
| --- | --- | --- |
| Notion Spec | intent, included scope, non-goals, acceptance, amendments | runtime status or review verdict |
| Development Record | active Session, branch, base/candidate/merge SHA, links, CI and Host Gate status | a second Work lifecycle |
| Execution Report | high-signal findings, difficulties, decisions, verification, completion claim | Spec changes or reviewer verdict |
| Review Report | exact-SHA review revisions, findings, acceptance matrix, Host decision | implementation transcript |
| GitHub Issue | executable repository context and closure target | long-term product mental model |
| Pull Request | final diff, checks, technical discussion, merge fact | replacing the canonical Spec |

## Claim And Isolation

Before editing:

1. Read the Development Playbook, Development Record, Spec, and Issue.
2. Confirm the record is Ready and has no Active Session.
3. Fetch the latest remote base and create a clean isolated worktree.
4. Record the Primary Session id, branch, exact Base SHA, Execution Report,
   Review Report, and `In Progress` stage.
5. Re-read the record and stop if another Session owns it.

Never start writable work on a dirty shared project root. Concurrent development Waves use
separate worktrees and an explicit merge order. Shared hot files require a
declared integration fence; later work must absorb, not overwrite, the earlier
merged invariants.

## Execution And Reporting

The Primary Session owns implementation, tests, documentation, PR maintenance,
CI repair, and closeout. It records only high-signal findings: verified root
cause, important failed approaches, scope-affecting decisions, blockers,
handoffs, and evidence. Raw command streams, private checklists, provider
transcripts, and Sub-Agent internals do not belong in Notion.

Harness Members are not admitted for repository repair until the explicit
dogfood admission standard passes. A bootstrap or repair development Wave may use the
Primary Codex Session and bounded temporary Sub-Agents, while honestly stating
that this is not Harness Member execution.

## Pull Request And Candidate SHA

One development Wave produces one integration PR. The PR links the Spec and umbrella Issue
and states:

- what changed and what deliberately did not change;
- data, migration, compatibility-read, and dual-write policy;
- validation performed on the Candidate SHA;
- known risks and follow-ups.

Any code change creates a new Candidate SHA and invalidates prior affected CI,
Completion, and Review claims. A green earlier revision cannot authorize a
later revision.

## Review And Host Gate

Ordinary repository work uses final-diff self-review by the Primary Session;
there is no mandatory second reviewer queue. An independent read-only reviewer
is used when the Spec requires it or the change affects core persistence,
cross-module public contracts, security/permissions, or irreversible data.

Review is bound to the exact Candidate SHA and records each acceptance item as
`PASS`, `FAIL`, or `NOT PROVEN`. P0/P1 findings block merge. Later `CHANGES
REQUIRED` supersedes an earlier PASS until a new SHA is reviewed. Host Gate is
narrow: it authorizes merge only when the Development Record requires it; it
does not replace product Work Gate, Acceptor, Evidence, Finding, Failure, or
Decision semantics.

## Merge And Closeout

Merge only when the final SHA has required CI green, self-review passed, no
open P0/P1, and Host approval when required. After GitHub records the merge:

1. record the merge SHA and closeout evidence in Notion;
2. clear the Active Session id but retain Session lineage in the Execution
   Report;
3. close only Issues whose acceptance is fully satisfied;
4. create explicit follow-up work for remaining non-blocking risks.

PR merge is a repository fact. It does not by itself accept a product Work or
prove live Agent Team execution.

## Failure Semantics

- Do not make a flaky gate green by rerun, timeout inflation, or deleting the
  assertion.
- Do not hide partial append-only success; report the successful prefix and
  exact failure.
- Do not convert retryable Store contention into a permanent client error.
- Do not claim live provider or Harness Member behavior from deterministic
  shims.
- When blocked, preserve the exact branch/SHA, clean-or-dirty state, completed
  work, reproducible evidence, primary cause, remaining risk, and next action.

## Required Local Gates

Use focused checks during implementation. Before delivery, run the checks in
[operations.md](operations.md) on one frozen SHA. The repository-owned clean
archive gate is:

```bash
pnpm gate:clean-archive
```

It requires a clean committed source tree and pnpm 9.15.4, extracts the exact
SHA, performs the frozen install before runtime tests, then runs Rust,
governance, and JavaScript gates from the archive.
