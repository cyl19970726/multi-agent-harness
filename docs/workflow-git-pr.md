# Git, PR, And Review Workflow

This document defines how Mission/Wave executors integrate file changes with
Git, worktrees, pull requests, review, and acceptance. Git owns repository
facts; Harness owns execution attribution and Wave acceptance.

## Native Flow

```text
Mission
  -> Wave(executor)
  -> executor attempt
  -> isolated or explicitly direct file changes
  -> diff / commit / PR evidence
  -> executor outcome
  -> Wave gate
  -> next Wave or Mission closeout
```

Merging a PR does not accept a Wave, and accepting a Wave does not merge a PR.
The Wave gate names the accepted completed attempt and records the outcome and
useful artifacts/checks.

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
declared before concurrent edits and reflected in the attempt's artifacts.

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
- executor attempt id;
- assignment correlation for Agent Team-owned work;
- checks and relevant artifact/diff refs; and
- the outcome being proposed for the Wave gate.

Review depth is proportional to risk. A dedicated reviewer member or external
code owner may be useful, but Proposal/Review/Decision is not a mandatory
product chain for every Wave.

Protected merge, deployment, remote deletion, payment, or other external
effects require their own authorization. A completed executor attempt alone
never grants it.

## Retry And Failure

- A revise gate creates a new executor attempt and preserves the earlier one.
- A rejected WorkflowPatch remains rejected history; do not mutate it into the
  replacement patch.
- A failed apply must leave the target branch recoverable and report the exact
  conflict or dirty-tree condition.
- Work outside owned paths is a blocker until reviewed or reassigned.
- If a PR merges before a gate, the gate still needs an explicit outcome and
  accepted attempt; if a gate accepts before merge, the PR still follows Git
  protection and review rules.

## Acceptance Checklist

1. Mission, Wave, executor kind, and attempt are reconstructable.
2. File-changing ownership and isolation are explicit.
3. Diff, commit, patch, or PR evidence resolves to the actual change.
4. Required checks and reviews are recorded.
5. The Wave gate names one completed accepted attempt.
6. The Mission closeout reflects accepted Wave outcomes.
7. No retired coordination object or hidden provider transcript is needed to
   explain the result.
