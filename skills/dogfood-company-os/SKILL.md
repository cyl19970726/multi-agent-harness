---
name: dogfood-company-os
description: Run Company OS as its own operating system through repeated, evidence-backed Docs, Work, Organization, external-delivery, and execution cycles. Use when a Lead or Supervisor needs the Company to discover its own gaps, create or prioritize WorkItems, route them to Standing Agents or humans, execute through an appropriate runtime, return results to company memory, inspect the UI, and repeat until the chosen acceptance boundary is healthy. Do not use for initial business bootstrap, one isolated Docs/Work/Org mutation, or Mission/Wave execution alone.
---

# Dogfood Company OS

Operate the Company through its own native objects. Dogfood means the system
that discovers, assigns, executes, reviews, and records the improvement is the
same Company OS being improved. A repository report or locally fixed bug is
not enough.

This is a procedural composition Skill, not product authority and not a
universal operator. Delegate each write to the Skill and command that own that
object.

## Keep The Truths Separate

```text
Docs = durable company memory, operating context, and result return
Work = commitments, responsibility, lifecycle, acceptance, and provenance
Organization = durable actors, reporting, permissions, and capability
External connector = observed source/delivery facts and governed effects
Execution = Mission/Wave, Agent Team, Workflow, Host, or human work
Provider session = native transcript, tools, and turn lifecycle
```

The loop links these truths. It does not collapse them.

Use the smallest focused capability:

| Need | Use |
| --- | --- |
| Bootstrap a new commercial operating area | `$company-business-project-bootstrap` |
| Operate Documents, Blocks, records, relations, or views | `$company-docs-operator` |
| Create, assign, transition, or close company commitments | `$company-work-operator` |
| Operate Standing Agents, humans, units, roles, or permissions | `$company-org-operator` |
| Link GitHub source and delivery facts | `$connect-github-company-os` |
| Design a new recurring business domain | `$company-module-designer` |
| Build a core custom page after its contract is approved | `$company-page-builder` |
| Execute a long-running change with persistent members | `$orchestrate-mission-waves` |

Load only the focused Skills needed for the current cycle.

## Start From An Explicit Company

Select Company Store truth independently from execution source code:

```bash
harness company current
harness --company <company-id> company docs query --document <root-document-id>
harness --company <company-id> company work list
harness --company <company-id> company org list
```

A Company Store may bind several repositories. A Git worktree is an execution
resource selected through a Project Binding or explicit MemberRun/TeamRun cwd;
it is neither the Execution Space nor the owner of Company Docs, Work, or
Organization.

Before mutation, inspect:

- the active company root and relevant Document/module;
- open, blocked, in-review, and stale WorkItems;
- Standing Agents, memberships, permissions, and execution bindings;
- external source/delivery freshness;
- active execution and provider/runtime health; and
- the actual Store-live UI for navigation, visibility, error, and empty states.

Do not treat archived Documents, stale source snapshots, old fixture rows, or
an unrelated project-derived compatibility Store as current operating truth.

## Run One Complete Cycle

### 1. Observe

Use native read surfaces. Record a gap only when evidence identifies the
affected company object, source, expected behavior, and current behavior.

Examples:

- a Document tree exposes archived or duplicate context;
- a WorkItem has no result, evidence, or execution relation;
- a Standing Agent cannot see or accept its assigned work;
- an Agent/runtime upgrade left a mailbox or native Session unreconciled;
- GitHub Issue/PR/check state is not linked to the Company commitment; or
- UI cannot reconstruct the relation that CLI can.

### 2. Decide And Commit

Reuse an existing WorkItem when it already owns the gap. Create a new one only
when no current commitment has the same source, objective, and acceptance
boundary.

The WorkItem must preserve:

- source Document/record or external observation;
- objective, description, acceptance criteria, and return location;
- accountable owner, assignee, reviewer, and approver when applicable;
- WorkType/module/Milestone grouping;
- required execution and delivery evidence; and
- protected effects that require Human, Policy, Finance, Legal, or Org review.

A finding is not a commitment until Work owns it.

### 3. Route Through Organization

Assign only to a real Human, Standing Agent, external collaborator, or service.
Confirm the actor exists, is active, has the relevant responsibility and
permission ceiling, and has an execution binding when an AI runtime is needed.

If no actor can own the work, create a capability-gap WorkItem. Do not silently
create a Standing Agent, expand permissions, or infer authority from a
MemberRun, provider session, avatar, Skill, or logged-in external account.

### 4. Execute

Choose the smallest truthful executor. A durable Standing Agent may participate
through its linked Agent Team identity, but Organization identity and execution
membership remain distinct.

When Mission/Wave and Agent Team are used:

- WorkItem remains the company commitment;
- Assignment/TeamMessage remains execution-lane ownership;
- Mission/Wave records Host intent and judgment;
- MemberRun/native Session remains execution continuity; and
- Work receives an `ExecutionRef`, delivery evidence, and final result only
  after the execution actually exists.

Do not make Wave completion equal Work acceptance.

### 5. Accept And Return

Review against the WorkItem acceptance criteria. A provider completion,
Handoff, green CI run, merged PR, Document edit, or pretty UI is evidence, not
by itself Company acceptance.

On acceptance:

1. attach delivery/evidence/execution refs;
2. record the result summary and result Document/record;
3. transition or close the WorkItem through Work;
4. update the originating Docs/module with the durable result;
5. update Organization only if responsibility or capability truly changed; and
6. verify CLI and UI reconstruct the same relations.

### 6. Re-observe

Run another bounded observation pass. Continue while a critical or material
gap inside the chosen scope remains. Stop when:

- acceptance is met and only explicitly deferred work remains;
- a protected decision needs Human input;
- external credentials/service availability block honest progress; or
- a new problem belongs to a different WorkItem and risk boundary.

Dogfood is iterative; one `Docs -> Work -> Docs` example is not proof of the
Company operating model.

## Treat Upgrade And Crash Recovery As Company Work

After a Harness, adapter, protocol, permission, model-control, Plugin, or Skill
change:

1. classify whether only UI/Docs projection changed or a runtime contract
   changed;
2. drain or interrupt active turns before replacing an incompatible runtime;
3. install/sync canonical artifacts before starting the next generation;
4. preserve Standing Agent, WorkItem, MemberRun, Assignment correlation, and
   provider-native Session when the reviewed contract allows resume;
5. create a new native Session when compatibility cannot be proven, retaining
   the old Session as history; and
6. reconcile queued/claimed mail, permissions, model controls, cwd/Skill roots,
   and the single writable-Workspace driver before resuming.

Never let two runtime generations drive the same writable Workspace.

## UI Acceptance

Verify the Store-live product, not a static fixture:

- Organization shows the durable actor and its current work/execution binding;
- Work shows source, owner, assignee, acceptance, delivery, evidence, result,
  and status;
- Docs shows the originating context and returned result without duplicated
  task state;
- Agent detail shows work and external activity without becoming another
  store;
- connector facts show freshness and deep links;
- navigation, scroll, empty/error/loading states, and responsive layout work;
  and
- UI never upgrades a partial/planned object into an implemented claim.

Record UI defects as WorkItems and keep the loop running.

## Handoff

Report:

- Company id and selected operating scope;
- cycles completed and native object ids;
- Docs, Work, Org, external-delivery, and execution relations proven;
- CLI checks and Store-live UI evidence;
- accepted results and explicitly deferred WorkItems;
- runtime/Plugin/Skill generation used; and
- the next highest-value dogfood cycle.
