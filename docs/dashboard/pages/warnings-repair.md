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
- Gap ledger (first-class `Gap` objects; Bug = `Gap(category=bug)`);
- Goal;
- Task;
- AgentMember;
- Message;
- Evidence;
- Proposal;
- Decision;
- ProviderSession.

Implemented: the Warnings surface is the home of the `Gap` ledger. It renders
`Gap` rows sortable by `severity` (`p0`/`p1`/`p2`) and `status`
(`open`/`in_progress`/`fixed`/`blocked`/`deferred`/`wontfix`), alongside the
derived `WorkflowWarning` queue. The implemented warning kinds are
`review_needs_decision`, `gap_unresolved`, `failed_provider_session`,
`goal_learning_gap`, `goal_close_without_evaluation`, and
`waiver_without_follow_up`.

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

## Layout Contract

Desktop target: `1440x1000`.

```text
+--------------------------------------------------------------------------------+
| top 56: Workbench | live/source | warning scope | active object | search | dbg |
+-----+----------------------+-----------------------------------+---------------+
| app | severity rail 248    | warning workspace 760             | repair 400    |
| 64  | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | warning summary  | | | global queue header 72       | | | selected  | |
|     | | P0/P1/P2 counts  | | | affected scope + filters      | | | warning   | |
|     | +------------------+ | +-------------------------------+ | +-----------+ |
|     | | severity groups  | | | urgent warnings 280          | | | cause     | |
|     | | missing proof    | | | object, cause, consequence    | | | consequence|
|     | | delivery/session | | +-------------------------------+ | +-----------+ |
|     | | stale workflow   | | | object-local callouts 240    | | | safe      | |
|     | +------------------+ | | goal/task/member proof gaps   | | | repair    | |
|     | | filters          | | +-------------------------------+ | +-----------+ |
|     | | object/status    | | | disabled repairs/follow-ups   | | | docs/logs |
|     | rail scroll          | workspace scroll                   | repair scr   |
+-----+----------------------+-----------------------------------+---------------+
```

Region dimensions:

- app rail `64px`;
- severity rail `240px` to `260px`;
- warning workspace min `720px`;
- repair inspector `380px` to `410px`;
- queue header `64px` to `80px`;
- urgent warning block target `260px` to `320px`;
- object-local callout block target `220px` to `270px`.

First viewport content:

- severity counts and affected-object filters;
- urgent warnings with object ref, cause, consequence, and next safe route;
- object-local callouts near broken proof sections;
- selected repair panel with enabled safe action or disabled reason;
- source docs/logs/evidence links for the warning.

Tablet target: `900x1180`.

```text
+------------------------------------------------------------------+
| top 56: Workbench | warnings | active object | search | debug     |
+-----+---------------------------------------+--------------------+
| app | warning workspace 548                | repair 288         |
| 56  | +-----------------------------------+| +----------------+ |
|     | | summary + severity filters        || | selected warn  | |
|     | +-----------------------------------+| | cause/effect   | |
|     | | urgent queue                      || | repair/disabled| |
|     | | object-local callouts             || +----------------+ |
|     | | follow-up task suggestions        | repair scroll      |
+-----+---------------------------------------+--------------------+
| severity rail collapses to filters row/drawer                              |
+------------------------------------------------------------------+
```

Mobile target: `390x844`.

```text
+--------------------------------------+
| top 48: Warnings | source | debug    |
+--------------------------------------+
| summary 88: P0/P1/P2 + affected obj  |
+--------------------------------------+
| tabs 52: Urgent Local Repair History |
+--------------------------------------+
| active tab 604                       |
| Urgent: warning rows + affected link |
| Local: callouts by Goal/Task/Member  |
| Repair: cause/consequence/next step  |
| History: resolved/killed warnings    |
+--------------------------------------+
```

Scroll ownership:

- desktop: severity rail, warning workspace, and repair inspector scroll
  separately;
- tablet: workspace and repair inspector scroll separately;
- mobile: only the active tab scrolls.

Screenshot acceptance:

- a warning must always name what is wrong, where, why it matters, and what
  path is safe;
- disabled repair actions must look disabled and explain missing backend/API;
- warning severity must not rely on color alone;
- local callouts must appear near affected proof sections, not only globally.

## Failure Modes

- warning is detached from object;
- warning cannot be acted on or explained;
- disabled repair appears enabled;
- warnings become decorative badges.

## Screenshot Acceptance Questions

- Can the reviewer see what is wrong and where?
- Is there a safe next action or disabled reason?
- Are warning severity and affected object clear without color alone?
