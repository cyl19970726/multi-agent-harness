# Warnings And Repair Page Spec

```text
status: planned
owner_role: product-design
canonical_for: workflow risk queue and repair navigation
route_or_surface: /warnings, Team workspace queue, object-local callouts
```

## Purpose

Primary user question: what threatens acceptance, where is it, why does it
matter, and what safe repair path exists?

Why it exists: Workflow gaps should be visible near affected objects and in a
global queue. Warnings are not decorative alerts; they guide review and repair.

Non-goals:

- do not use toast-only warnings;
- do not show warnings detached from object context;
- do not simulate repair actions without canonical API/CLI support.

## Objects And Proof

Canonical objects:

- WorkflowWarning read model;
- Goal;
- Task;
- AgentMember;
- Message;
- Evidence;
- Proposal;
- Decision;
- ProviderSession.

Workflow proof:

- each warning names affected object, severity, cause, consequence, and repair
  navigation;
- local callout appears near the broken proof section;
- global queue groups urgent issues;
- disabled repair actions explain missing backend support.

Source docs:

- [../read-model.md](../read-model.md)
- [../acceptance.md](../acceptance.md)
- [../../operations.md](../../operations.md)

Read-model inputs:

- `warningsByObject(snapshot)`;
- stale/failed messages and sessions;
- missing evidence/review/decision/evaluation signals;
- role gaps and old-code contamination warnings.

## Page-Level Agent Loop

Designer options:

- global queue plus local callouts;
- inspector-only warnings;
- workflow health checklist.

Questioner challenges:

- Does warning context explain why it matters?
- Can the operator navigate to the affected proof section?
- Are unavailable repairs honestly disabled?

Reviewer decision: use global queue plus local callouts. Borrow checklist
summary in Goal/Task headers.

Rejected options:

- inspector-only: too easy to miss;
- checklist-only: too passive.

Borrowed ideas:

- compact health checklist for object headers.

## Information Architecture

Selected IA:

```text
global warning queue
  -> severity groups
  -> affected object navigation
object-local callout
  -> cause
  -> consequence
  -> safe repair or disabled reason
```

Primary actions: open affected object, filter severity, inspect cause, trigger
safe repair when API exists.

Secondary actions: create follow-up task when repair is not supported.

Empty/loading/error states:

- empty: no current warnings;
- loading: preserve queue/callout geometry;
- error: show warning derivation/source failure.

Responsive requirements:

- desktop: global queue plus object-local callouts;
- tablet: Warnings drawer/tab;
- mobile: Warnings tab with affected object jump links.

Links to hard layout specs: pending.

## Failure Modes

- warning is detached from object;
- warning cannot be acted on or explained;
- disabled repair appears enabled;
- warnings become decorative badges.

## Screenshot Acceptance Questions

- Can the reviewer see what is wrong and where?
- Is there a safe next action or disabled reason?
- Are warning severity and affected object clear without color alone?
