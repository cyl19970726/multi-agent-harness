# Work visual contract V1

## Invariants across all screens

- One shared shell and one shared Work navigation vocabulary.
- `Overview`, `Board`, `All Work`, `Milestones`, `Timeline`, and `Workload` are
  primary views; saved views are visually secondary.
- Business line, Work type, status, accountable owner, assignee, and Milestone
  remain recognizable in every dense view.
- Accountable owner is never visually collapsed into assignee.
- Approval pressure names the required human or authority.
- Mission/Wave appears only as a typed execution reference.
- No Project object, GoalPhase, task graph, or invented universal Agent object.

## Screen-specific proof

| Screen | Must prove |
| --- | --- |
| Overview | company-wide counts, business-line health, due/blocked/approval queues, active Milestones |
| Board | state flow, mixed actor assignments, blockers, due pressure, honest low-volume columns |
| All Work | full sortable ledger and cross-line grouping without duplicate cards |
| Milestones | outcome checkpoints, target dates, acceptance criteria, remaining work and risk |
| Workload | accountability versus execution assignment, human/Agent distinction, unassigned work |
| WorkItem Focus | complete responsibility chain, source/result, approval, evidence, finance and execution links |

## Truth labels

- `Current Store-live`: browser evidence from implemented Store projections.
- `Expected`: generated target composition using illustrative design data.
- `Actual`: future deterministic or Store-live browser capture from the same
  approved revision and viewport.

Human review is required before these expected images become implementation
acceptance baselines.

