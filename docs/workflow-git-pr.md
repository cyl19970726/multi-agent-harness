# Git, PR, And Review Workflow

This document defines how execution used during a Mission integrates file
changes with Git, worktrees, pull requests, review, and Host plan decisions.
Git owns repository facts; Harness owns execution attribution and Wave
plan/outcome history.

## Native Flow

```text
Mission
  -> ordered Host-plan Wave
  -> Host invokes Agent Team | Dynamic Workflow | direct Host work
  -> executor run or observable Host work
  -> isolated or explicitly direct file changes
  -> diff / commit / PR evidence
  -> execution outcome
  -> explicit Host Wave advance
  -> next Wave or Mission closeout
```

Merging a PR does not advance a Wave, and advancing a Wave does not merge a PR.
The Host records what outcome and useful artifacts/checks justified the plan
change. The Wave does not own the execution run.

## Executor Boundaries

| Executor | Ownership truth | Git integration |
| --- | --- | --- |
| Agent Team | assignment `TeamMessage` plus correlation and MemberRun actions | members use disjoint worktrees/owned paths; Host integrates reviewed results |
| Dynamic Workflow | WorkflowRun/WorkflowStep and WorkflowPatch | isolated writable leaves produce explicit apply/reject patches |
| Host | observable Host outcome and artifacts | normal branch/commit/PR flow; native subagents remain implementation detail |

The Harness must not synthesize a MemberRun or assignment for a provider-native
Host child it does not control.

## Branch And Worktree Policy

- Select the project explicitly before spawning writable work.
- The project root determines the base revision; the centralized store remains
  separate.
- Concurrent file-changing lanes need distinct worktrees and disjoint owned
  paths, or an explicit serialized integration plan.
- Read-only work may share the project root only when the provider actually
  enforces read-only access; otherwise isolate it.
- Do not start direct writable work on a dirty shared project root.
- A worktree diff is evidence, not an automatic merge authorization.

No universal Mission branch or per-Wave branch is required. A repository may
choose one branch per Mission, Wave, or change set, but the policy must be
declared before concurrent edits and reflected in the execution artifacts.

## Dynamic Workflow Patches

For `workflow run-script`, an eligible successful writable leaf records a
pending `WorkflowPatch`; the throwaway worktree is then removed. Apply or reject
the patch explicitly:

```bash
harness workflow patch apply <patch-id>
harness workflow patch reject <patch-id>
```

`persist_changes="discard"` opts out of patch retention. Direct write mode is a
separate explicit choice for small serial work and leaves the diff in the
selected project root for normal Git review.

## Pull Requests

A PR should reference:

- Mission and Wave ids;
- relevant executor run id, when one exists;
- assignment correlation for Agent Team-owned work;
- checks and relevant artifact/diff refs; and
- the outcome the Host may use when updating or advancing its Wave plan.

Review depth is proportional to risk. A dedicated reviewer member or external
code owner may be useful, but Proposal/Review/Decision is not a mandatory
product chain for every Wave.

Protected merge, deployment, remote deletion, payment, or other external
effects require their own authorization. A completed executor attempt alone
never grants it.

## Retry And Failure

- If the Host chooses another execution run, the earlier run remains history.
- A rejected WorkflowPatch remains rejected history; do not mutate it into the
  replacement patch.
- A failed apply must leave the target branch recoverable and report the exact
  conflict or dirty-tree condition.
- Work outside owned paths is a blocker until reviewed or reassigned.
- If a PR merges before Wave advance, the Host still records the explicit
  outcome; if a Wave advances before merge, the PR still follows Git
  protection and review rules.

## Acceptance Checklist

1. Mission, current Host-plan Wave, and relevant execution run are reconstructable.
2. File-changing ownership and isolation are explicit.
3. Diff, commit, patch, or PR evidence resolves to the actual change.
4. Required checks and reviews are recorded.
5. The Wave advance records the Host outcome and useful supporting evidence.
6. The Mission closeout reflects accepted Wave outcomes.
7. No retired coordination object or hidden provider transcript is needed to
   explain the result.
