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

Links to hard layout specs: pending.

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
