---
name: company-work-operator
description: Inspect and route authoritative TeamWork through the Company Work aggregate, and manage Company Milestone or Approval references without creating a second task or assignment object.
---

# Company Work Operator

This skill is procedural guidance, not product authority. The native TeamWork contract, Store validation, and Human governance decisions remain authoritative.

Use Company Work as a company-wide read, filter, and routing surface over
authoritative TeamWork. The shorthand `work` is acceptable only when the
execution or Company context is explicit.

The core invariant is:

```text
Company Work row id + revision == authoritative TeamWork id + revision
```

Never copy a TeamWork into a Company-owned task record. Never dual-write
lifecycle, responsibility, delivery, evidence, or status.

## Authority boundary

TeamWork owns:

- identity and revision;
- Team and TeamRun scope;
- parent/prerequisite relationships;
- owner and eligible members;
- phase, condition, and resolution;
- context and completion criteria;
- immutable reports, gate evaluations, operational decisions, events, evidence,
  artifacts, checks, and external links;
- delivery and provider runtime bindings.

Company OS owns only company context that refers to TeamWork:

- `Milestone.work_refs`;
- `Approval.subject_ref(kind=work)`;
- Business Module or Docs relations that point to Work;
- Company-wide filters, summaries, boards, and routing links.

Company OS does not own a second task ledger, assignment ledger, lifecycle, or
execution bridge.

## Lifecycle contract

Treat the three axes independently:

- phase: `open | active | review | closed`;
- condition: `normal | blocked | on_hold`;
- resolution: `accepted | cancelled | failed`, present only when phase is
  `closed`.

Blocking or pausing preserves phase. Delivery state, gate state, and provider
runtime state are independent and must not be inferred from phase.

A Work is accepted only when:

```text
phase = closed AND resolution = accepted
```

The accountable member cannot self-accept. Acceptance must follow the
authoritative chain:

```text
WorkReport -> Evidence -> GateEvaluation / Verification
  -> OperationalDecision -> WorkEvent
```

Every report and decision must bind the exact Work id and revision being
evaluated.

## Select stores explicitly

Company reads need a Company Store; TeamWork mutations need an Execution Space.
Select both explicitly when they differ:

```bash
harness company current
harness project current
harness work list
harness --space <space-id> team-run work list --team-id <team-id>
```

Do not rely on the current directory to imply either authority.

## Read and filter Global Work

The Global Work view (DOC-106) is the one read projection over the canonical
Work/WorkOperation authority. It replaces the retired `company work
list/query` CLI and `/v1/views/company-work` endpoint.

```bash
harness work list
harness work list --team-id <team-id>
harness work list --assignee-membership-id <membership-id>
harness work list --assignee-kind <host|member|unassigned>
harness work list --member-id <agent-member-id>
harness work list --phase <open|active|review|closed>
harness work list --condition <normal|blocked|on_hold>
harness work list --resolution <accepted|cancelled|failed>
harness work list --priority <low|normal|high|urgent>
```

The same projection is served at `GET /v1/views/global-work`. Responsibility
follows the assignee TeamMembership, never a MemberRun or runtime state.

These commands are read-only. They return the original TeamWork records without
fallback rows or copied Company lifecycle fields.

## Route mutations to TeamWork

Company Work mutation commands do not exist. Use the authoritative execution
surface:

```bash
harness team-run work create --team-run-id <run-id> --title <title> \
  --completion-criteria <criteria>
harness team-run work assign --team-run-id <run-id> --work-id <work-id> \
  --member-run-id <member-run-id> --expected-version <version>
harness team-run work start --team-run-id <run-id> --work-id <work-id> \
  --expected-version <version>
harness team-run work block --team-run-id <run-id> --work-id <work-id> \
  --reason <reason> --expected-version <version>
harness team-run work resume --team-run-id <run-id> --work-id <work-id> \
  --expected-version <version>
harness team-run work submit --team-run-id <run-id> --work-id <work-id> \
  --summary <summary> --expected-version <version>
harness team-run work request-changes --team-run-id <run-id> --work-id <work-id> \
  --reason <reason> --expected-version <version>
harness team-run work accept --team-run-id <run-id> --work-id <work-id> \
  --expected-version <version>
harness team-run work cancel --team-run-id <run-id> --work-id <work-id> \
  --reason <reason> --expected-version <version>
```

Always re-read the Work after a conflict. Do not retry a mutation using a stale
version.

## Milestones and approvals

Milestones group authoritative Work ids without replacing their identity:

```bash
harness company work milestone list
harness company work milestone show --milestone <milestone-id>
harness company work milestone create \
  --authority <human-admin-id> \
  --id <milestone-id> \
  --title <title> \
  --outcome <outcome> \
  --accountable-owner <actor-id> \
  --work <work-id>
harness company work milestone update \
  --authority <human-admin-id> \
  --milestone <milestone-id> \
  --work <work-id>
```

An Approval governing Work uses `subject_ref.kind = work` and the exact
authoritative Work id. Approval decisions do not mutate Work directly; the
authorized actor still performs the native Work operation.

## Docs, Finance, gateways, and GitHub

Docs may embed or relate to `EntityRef(kind=work)`, but must not copy Work
phase or completion truth into prose.

Finance owns Commitments and Payments. A Work may be their governed subject or
context through explicit relations, but a Work decision never implies payment.

Gateway and connector observations become context, evidence, or external links.
They do not create a parallel task model. GitHub Issue, PR, commit, check,
review, and release state remain external facts; they may satisfy criteria but
do not close Work without an operational decision.

## Safe workflow

1. Resolve the Company Store and Execution Space explicitly.
2. Read the authoritative Work and its current version.
3. Inspect Team, TeamRun, owner, prerequisites, criteria, gates, and condition.
4. Route any mutation to `team-run work`.
5. Attach immutable reports and evidence to the exact candidate revision.
6. Keep gate, delivery, and runtime state independent.
7. Use Milestone, Approval, Docs, or Finance references only for their owning
   system's context.
8. Re-read the exact Work revision after mutation.
9. Report the original Work id, new version, lifecycle axes, evidence, decision,
   and remaining blockers.

## Validation checklist

- No Company-owned duplicate task or assignment row was created.
- Company projection preserves original Work ids and revisions.
- Non-closed Work has no resolution.
- Closed Work has condition `normal` and exactly one resolution.
- Blocked or on-hold Work preserves its phase.
- Report, evidence, gate evaluation, decision, and event bind the same Work
  revision.
- The accountable member did not self-accept.
- Milestone and Approval reference `work`, not a copied record.
- Mutation used a current expected version and the native TeamWork surface.
- External facts remain linked evidence rather than lifecycle authority.

## Report format

State:

- authoritative Work id and version;
- Team and TeamRun;
- phase, condition, and resolution;
- owner and eligible members;
- report/evidence/gate/decision refs;
- Milestone, Approval, Docs, Finance, and external refs;
- mutation command used, if any;
- remaining blockers or missing cross-space visibility.
