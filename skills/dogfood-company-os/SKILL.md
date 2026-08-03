---
name: dogfood-company-os
description: Run Company OS as its own operating system through repeated, evidence-backed Docs, Work, Organization, external-delivery, and execution cycles. Use when a Company Lead runs a self-hosting cycle or when Human intent has been faithfully routed to the Company Lead to discover gaps, create or prioritize WorkItems, route them to Standing Agents or humans, execute through an appropriate runtime, return results to company memory, inspect the UI, and repeat until the chosen acceptance boundary is healthy. Do not use for initial business bootstrap, one isolated Docs/Work/Org mutation, or Mission/Wave execution alone.
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

GitHub is an external source and software-delivery evidence system. An Issue,
PR, commit, check, review, or release never becomes Company priority,
responsibility, approval, or acceptance merely because the connector observed
it.

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

- the active Company root, document hierarchy, and relevant module;
- the Human Principal / Constitution Owner and exact current constitutional
  subject/version/digest when one has actually been activated;
- open, blocked, in-review, and stale WorkItems;
- Company Lead and Domain Lead ownership, capacity, and escalation policy;
- Standing Agents, memberships, permissions, and execution bindings;
- external source/delivery freshness;
- active execution and provider/runtime health; and
- the actual Store-live UI for navigation, visibility, error, and empty states.

Do not treat archived Documents, stale source snapshots, old fixture rows, or
an unrelated project-derived compatibility Store as current operating truth.

## Continuous Company Operating Loop

Dogfood is continuous intake and replan, not a one-shot audit:

```text
Human Principal intent + Company/connector observations
  -> Supervisor preserves provenance and routes/delivers once
  -> durable source context
  -> Company Lead triage, priority, capacity, and replan
  -> Domain Lead accountable Work
  -> one Company Assignment, attenuated delegation, and truthful execution
  -> evidence, acceptance, and result return
  -> Store projections and UI readback
  -> next observation cycle
```

The Supervisor owns faithful intake/runtime delivery, runtime generation, and
explicit emergency control acknowledgements. It does not create responsibility,
set Company priority, issue grants, or approve Work. The Company Lead
deduplicates or rejects intake, chooses Company priority, balances explicit
capacity across domains, and replans when evidence or constraints change. A
Domain Lead decomposes and delegates operational work autonomously only within
currently implemented policy plus its responsibility, accepted WorkTypes,
tools, data, budget, permissions, and capacity.

Delegation always attenuates authority. The receiving actor gets the minimum
subset needed for the WorkItem and cannot expand its own access, spending,
approval, legal, organization-change, or external-commitment authority.
Provider-native subagents and Agent Team members remain execution details, not
new Organization actors.

Route to a Human queue only for a named Human gate, protected effect, authority
or Organization expansion, unresolved policy/evidence conflict, material
ambiguity that changes the commitment, or lack of safe bounded capacity.
Routine triage, assignment, status updates, and low-risk execution must not
wait for ceremonial Human approval.

Use the decision test: known policy, reversible effect, bounded blast radius,
and no material external commitment may proceed autonomously and remain in the
audit digest. Policy-unknown, materially irreversible/destructive,
root-security/credential, material finance/legal/external commitment,
major-public/production, root/cross-domain expansion, exhausted ceilings, or
unresolved conflict enters the Human Decision Queue with an exact reason and
evidence.

## Current Versus Target Authority Truth

Current implemented truth includes native Docs/Work/Org records and Actions,
Company Assignment delivery, explicit StandingAgent ↔ AgentMember execution
links, and provider/runtime evidence. A reviewed local or unmerged broker/grant
candidate is exact-commit evidence only and must remain labelled `candidate`,
never `implemented`. `Implemented` requires the exact generation to be present
in the selected merged repository and verified through the active installed
runtime, schema, Store/API behavior, projection readback, and proportional
acceptance evidence.

Target-only until accepted end to end:

- one hierarchical `ScopedPermissionGrant` lineage bound to an exact Company
  Constitution generation/digest and approved role-template set;
- strict parent/template attenuation with at least one narrower dimension;
- atomic sibling budget/concurrency reservation and ancestor fencing;
- authenticated Supervisor-bound Member authority with the root token kept
  service-side;
- autonomous child Team Member and approved-template Standing Agent creation;
  and
- truthful configured/effective/fenced authority plus Human Decision Queue UI.

Do not let this Skill promote target contracts into Store truth.

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
permission ceiling, and has capacity plus an execution binding when an AI
runtime is needed.

Route Company-wide priority/capacity conflicts to the Company Lead. Route
domain delivery to the accountable Domain Lead, which may delegate to lower
actors only within the narrower intersection of its own authority, the child's
authority, and the WorkItem's need. Preserve the Company Assignment separately
from the linked Agent Team Work, ordinary TeamMessage conversation, and
provider-native execution history.

If no actor can own the work, create a capability-gap WorkItem. Do not silently
create a Standing Agent, expand permissions, or infer authority from a
MemberRun, provider session, avatar, Skill, or logged-in external account.

### 4. Execute

Choose the smallest truthful executor. A durable Standing Agent may participate
through its linked Agent Team identity, but Organization identity and execution
membership remain distinct.

When Mission/Wave and Agent Team are used:

- WorkItem remains the company commitment;
- Agent Team Work remains execution-lane ownership and WorkDelivery wakes the
  selected MemberRun;
- Mission/Wave records Host intent and judgment;
- MemberRun/native Session remains execution continuity; and
- Work receives an `ExecutionRef`, delivery evidence, and final result only
  after the execution actually exists.

Mission/Wave and Agent Team are optional execution capabilities selected for
the WorkItem, not Company planning or Organization primitives. Do not make
Wave completion equal Work acceptance.

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

### 6. Re-observe And Replan

Re-read the active root hierarchy, open Work, actor capacity, exception queue,
connector freshness, and Store-live projections. The Company Lead reprioritizes
or reroutes explicit Work records when actual progress, new Human intent,
blockers, or capacity differ from plan.

Continue while a critical or material gap inside the chosen scope remains.
Stop when:

- acceptance is met and only explicitly deferred work remains;
- a protected decision needs Human input;
- external credentials/service availability block honest progress; or
- a new problem belongs to a different WorkItem and risk boundary.

Dogfood is iterative; one `Docs -> Work -> Docs` example is not proof of the
Company operating model.

## Dogfood Execution Roster And Research Budget

Dogfood TeamRuns use a deliberate provider roster, not whatever provider is
installed:

- Kimi (`kimi_acp`) is the primary execution member. Request the reviewed K3
  model alias with `max` thinking effort, and verify the MemberRun
  requested-vs-effective `provider_controls` receipt before trusting the lane;
  a recorded alias alone is never proof of the model actually used.
- Claude (`claude_agent_sdk`) may join only while its installed SDK version is
  reviewed: `harness member providers --fail-on-review` must be green. An
  unreviewed SDK is `review_required` and stays out of dogfood lanes.
- Codex providers are not dogfood execution members. Historical Codex runs are
  read-only evidence, and bounded `codex_exec` paths stay inside Dynamic
  Workflow; do not create new Codex Team lanes or use them as fallbacks.

Every member runs under a strict research budget. One evidence pass — the
Assignment, owned paths, and directly linked records — is enough to start.
After that pass the member must either produce its deliverables or report a
blocked verdict with the exact missing fact and a recommendation. The Host
steers or interrupts a member that keeps exploring past the checkpoint;
unbounded repository archaeology is a lane defect, not diligence.

## Rolling Reconciliation After Master Merges

A necessary master merge — a Harness, adapter, protocol, permission,
model-control, Plugin, or Skill change that dogfood must run on — triggers
rolling Supervisor reconciliation of every live dogfood runtime:

1. classify whether only UI/Docs projection changed or a runtime contract
   changed; projection-only merges need no restart;
2. drain or interrupt active turns before replacing an incompatible runtime;
3. install/sync canonical artifacts from the new master before starting the
   next generation;
4. rebase each member worktree onto the new master, or recreate a clean
   same-repository worktree when rebase is unsafe; never let two runtime
   generations write the same Workspace;
5. resume the same MemberRun and provider-native Session under a higher
   Supervisor generation when the reviewed contract allows it; when
   compatibility cannot be proven, record the reason and start a new native
   Session, retaining the old Session as history;
6. reconcile queued/claimed mail, permissions, model controls, cwd/Skill
   roots, and the single writable-Workspace driver before resuming; and
7. prove the new generation with an acceptance probe: a fresh queued
   WorkDelivery reaches the existing MemberRun, the same native Session
   continues, ordinary Work-linked conversation still flows, and the Member
   can submit the Work for explicit Host review.

Rolling means lane by lane: reconcile one member, probe it, then move to the
next. The reconciliation itself is Company Work — link the merge commit, the
Supervisor generations, and each resume-or-new-session decision to the
governing WorkItem.

## Truthful Store Projections And UI Acceptance

Treat append-only Store rows and their latest projections as Company truth.
Search indexes, SQL/read models, connector caches, dashboard cards, and custom
pages are derived views. They must remain rebuildable, expose freshness and
partial/planned status, and never manufacture an actor, assignment, approval,
capacity value, delivery, or accepted result from display state.

Verify the Store-live product, not a static fixture:

- Organization shows the durable actor, hierarchy, declared capacity, and
  current work/execution binding without inferring runtime health as authority;
- Work shows source, owner, assignee, acceptance, delivery, evidence, result,
  status, and Human-exception reason when applicable;
- Docs shows the active root hierarchy and returned result without duplicated
  task state or archived content presented as current;
- Agent detail shows work and external activity without becoming another
  store;
- connector facts show observation time, freshness, and deep links;
- navigation, scroll, empty/error/loading states, and responsive layout work;
  and
- UI never upgrades a partial/planned object into an implemented claim.

Record UI defects as WorkItems and keep the loop running.

## Handoff

Report:

- Company id, root Document, and selected operating scope;
- cycles completed and native object ids;
- Human intake, Company Lead triage/replan, Domain Lead delegation, and
  Human-exception decisions proven;
- Docs, Work, Org, external-delivery, and execution relations proven;
- CLI checks and Store-live UI evidence;
- accepted results and explicitly deferred WorkItems;
- projection freshness and any canonical-vs-derived mismatch;
- the execution roster used (provider, mode, model/effort receipt) and any
  research-budget breaches the Host had to steer;
- runtime/Plugin/Skill generation used, plus each rolling-reconciliation
  decision after a master merge (merge commit, Supervisor generation,
  worktree action, resume or new native Session); and
- the next highest-value dogfood cycle.
