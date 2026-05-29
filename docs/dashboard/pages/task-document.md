# Task Document Page Spec

```text
status: planned
owner_role: product-design
canonical_for: Task assignment, execution, review, and decision proof
route_or_surface: /tasks/:taskId and Work tab Task surface
```

## Purpose

Primary user question: did this Task follow the harness protocol from
assignment through report, evidence, review, and decision?

Why it exists: Task is an assignable, reviewable unit inside a Goal. It needs
proof order, not just status.

Non-goals:

- do not treat `assignee_agent_id` as proof of assignment;
- do not show report/evidence without task message context;
- do not hide owned paths, branch, PR, or reviewer.

## Objects And Proof

Canonical objects:

- Task;
- `Message(kind=task)`;
- report Message;
- AgentMember;
- ProviderSession;
- Evidence;
- Proposal;
- Review evidence;
- Decision;
- branch/worktree/PR refs;
- warnings.

Workflow proof:

- assignment message before work report;
- delivery state before provider/session claims;
- evidence linked to report/proposal/session;
- review and Decision after proposal/evidence;
- owned paths and PR refs near proposal/review.

Source docs:

- [../../data-model.md](../../data-model.md)
- [../../workflow-git-pr.md](../../workflow-git-pr.md)
- [../read-model.md](../read-model.md)

Read-model inputs:

- `taskDocument(snapshot, taskId)`;
- task-linked messages;
- sessions, evidence, proposals, decisions;
- warnings by object.

## Page-Level Agent Loop

Designer options:

- proof-order document: assignment to decision sequence;
- execution workbench: current state/actions first;
- PR/evidence audit: diff/check/evidence first.

Questioner challenges:

- Does assignment proof come before report?
- Are reviewer and Decision visible?
- Are Git/PR refs near the proposal?

Reviewer decision: use proof-order document. Borrow current-state strip from
execution workbench and owned-path/check block from audit view.

Rejected options:

- execution primary: hides protocol order;
- PR/evidence primary: too narrow and weak on assignment.

Borrowed ideas:

- runtime/current state strip;
- owned-path/check block.

## Information Architecture

Selected IA:

```text
task header
  -> objective and acceptance
  -> assignment and delivery proof
  -> assignee/runtime/report state
  -> evidence and proposal
  -> review and decision
  -> Git refs and warnings
```

Primary actions: assign/send task message, deliver to assignee, request review,
open proposal/evidence, open member, open related docs.

Secondary actions: retry delivery, reconcile session, open PR/check refs.

Empty/loading/error states:

- empty: task exists but lacks assignment message;
- loading: preserve proof section order;
- error: show API/source failure.

Responsive requirements:

- desktop: proof document with assignee inspector;
- tablet: proof document plus drawer for member/evidence;
- mobile: objective, assignment proof, current state, warnings, then evidence.

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | active Goal | selected Task | search | debug |
+-----+----------------------+-----------------------------------+---------------+
| app | proof nav 248        | task proof document 760           | member 400    |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | task identity    | | | title/objective 96           | | | assignee  | |
|     | | status/lane/path | | | acceptance criteria/status    | | | identity  | |
|     | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | proof order nav  | | | assignment proof 144         | | | runtime   | |
|     | | 1 assignment     | | | task message + delivery       | | | health    | |
|     | | 2 report         | | +-------------------------------+ | +-----------+ |
|     | | 3 evidence       | | | report/current state 140     | | | inbox     | |
|     | | 4 proposal       | | | assignee report + session     | | | outbox    | |
|     | | 5 review         | | +-------------------------------+ | +-----------+ |
|     | | 6 decision       | | | proposal/evidence/checks 220 | | | actions   | |
|     | +------------------+ | | changed paths + PR refs       | | | docs      | |
|     | | warnings/docs    | | +-------------------------------+ | +-----------+ |
|     | rail scroll          | | review -> decision below fold  | | member scr |
|     |                      | document scroll                   |             |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- proof nav `240px` to `260px`;
- task document min `720px`;
- member inspector `380px` to `410px`;
- assignment proof `136px` to `160px`;
- report/current state `128px` to `152px`;
- proposal/evidence/checks block `200px` to `240px`.

First viewport content:

- task objective, acceptance criteria, status lane, owned paths, reviewer;
- assignment message and delivery state before any report;
- assignee runtime/current state connected to report;
- evidence, proposal, changed paths, PR refs, and checks;
- review and decision status visible as incomplete/complete pressure.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | active Goal | selected Task | search | debug  |
+-----+-------------------------------+----------------------------+
| app | task proof document 560       | member/evidence 284        |
| 56  | +---------------------------+ | +------------------------+ |
|     | | title/objective/status    | | | assignee/runtime      | |
|     | +---------------------------+ | | inbox/outbox/actions   | |
|     | | sticky proof tabs 48      | | | evidence/checks       | |
|     | +---------------------------+ | +------------------------+ |
|     | | assignment proof          | | inspector scroll         |
|     | | report/current state      | |                          |
|     | | proposal/evidence/checks  | |                          |
|     | | review/decision           | |                          |
+-----+-------------------------------+----------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: Task | live/source | debug   |
+--------------------------------------+
| header 112: objective/status/path    |
| assignee + reviewer + missing proof  |
+--------------------------------------+
| tabs 52: Assign Report Proof Review  |
+--------------------------------------+
| active tab 584                       |
| Assign: task msg + delivery state    |
| Report: runtime + latest report      |
| Proof: evidence/proposal/checks/PR   |
| Review: review -> decision -> warns  |
+--------------------------------------+
```

Scroll ownership:

- desktop: proof nav, task document, and member inspector scroll separately;
- tablet: proof document and inspector scroll separately;
- mobile: only the active tab scrolls.

Screenshot acceptance:

- assignment proof must visually precede report/evidence;
- missing assignment, evidence, review, or decision must look incomplete;
- branch/worktree/PR refs must sit near proposal and checks;
- assignee state must be connected to the task, not a detached member card.

## Failure Modes

- task is only a status card;
- assignee field fakes assignment;
- evidence shown without source message/session;
- review/decision missing;
- mobile hides assignment proof below long evidence lists.

## Screenshot Acceptance Questions

- Can the reviewer trace assignment -> report -> evidence -> review -> decision?
- Is missing assignment proof visually obvious?
- Are branch/worktree/PR refs close to proposal/review?
- Does the page avoid becoming a generic detail card?
