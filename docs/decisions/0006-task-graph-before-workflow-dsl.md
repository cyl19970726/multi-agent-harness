# 0006: Task Graph Before Workflow DSL

## Decision

Use a simple task DAG before introducing a larger workflow DSL.

## Rationale

The MVP needs dependencies, parent tasks, workspace refs, branch refs, PR refs,
owned paths, reviewers, messages, evidence, and decisions. A workflow DSL would
prematurely hide the object semantics we still need to validate.

## Consequences

Parallel development is expressed as separate tasks with separate worktrees and
branches, then integrated through PRs or equivalent review artifacts.
