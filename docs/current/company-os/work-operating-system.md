# Company Work Operating System

Status: current
Contract: AFM-2026.08.2

## Product boundary

Company Work is the company-wide operating view of native TeamWork. It lets an
operator search, filter, group, inspect, and route Work across execution
spaces. It never creates another executable object.

The authority split is:

| Concern | Authority |
|---|---|
| Work identity and revision | native TeamWork store |
| lifecycle transition | `team-run work` |
| immutable submission | `WorkReport` |
| verification | `WorkGateEvaluation` and evidence |
| accept/reject decision | `WorkOperationalDecision` |
| company grouping | read-only Company Work projection |
| business outcome grouping | Milestone referencing Work ids |
| human policy decision | Approval |

## Read projection

The projection is computed from execution-space stores at read time:

```text
ExecutionSpace A ─┐
ExecutionSpace B ─┼─> Company Work aggregate ─> filter / board / route
ExecutionSpace C ─┘
```

It must:

- preserve exact Work ids and revisions;
- expose the source execution space and mutation route;
- filter by team, team run, phase, condition, resolution, and owner;
- report duplicate-id conflicts rather than silently selecting one source;
- keep an explicitly empty aggregate empty;
- avoid fixture or retired-ledger fallback.

## Operator flow

1. Read Company Work to find the relevant native Work.
2. Resolve its execution-space route.
3. Inspect the latest native Work revision.
4. Mutate only through `team-run work` in that execution space.
5. Submit an immutable report with evidence and checks.
6. Run declared gates.
7. Record an operational decision.
8. Observe the changed Work through the Company aggregate.

## Dashboard contract

The default Company Work page is read-only and shows:

- exact Work id;
- team and TeamRun;
- owner;
- `phase`, `condition`, and `resolution` independently;
- blocker reason where present;
- summary counts derived from the same `works` collection.

The page exposes deterministic provenance markers:

- `data-company-work-authority="team-work"`;
- `data-company-work-read-only="true"`;
- `data-company-work-projection="company_work_aggregate"`.

There is no Company task detail route and no `workItem` URL parameter. A native
Work focus uses `teamWork=<id>` and routes to the Team Work surface.

## Milestones

Milestone is a Company grouping object, not a work executor. Its `work_refs`:

- store native Work ids unchanged;
- may span teams or execution spaces;
- do not copy Work title, owner, lifecycle, evidence, or revision;
- never cause a Work transition when the Milestone changes.

Milestone closure is a governed business statement. Native Work acceptance
remains separately evidenced.

## Failure handling

- Duplicate Work id across spaces: show a conflict and withhold a mutation
  route until scope is explicit.
- Missing execution space: keep the Work visible with an unavailable route.
- Stale revision: reject mutation and require a fresh read.
- Blocked/on-hold: preserve phase and record condition history.
- Provider or delivery failure: record runtime truth without silently closing
  Work.
- Gate failure: retain report and evidence, reject acceptance, and create a new
  Work revision only through a declared corrective operation.

## Cutover invariant

The live system contains no second Company task or assignment authority. Old
fixture data, screenshots, and historical decision records may describe the
former design, but active APIs, schemas, CLI commands, dashboard routes,
operator skills, and acceptance checks must use Unified Work.
