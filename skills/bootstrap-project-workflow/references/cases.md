# Case Reference

Use this reference to calibrate the skill on concrete examples. Cases are not
generic rules. Extract the principle first, then adapt it to the target
project's real objects, tools, and acceptance path.

## Extraction Method

```text
case detail
  -> observed failure
  -> generic risk
  -> reusable principle
  -> target-project adaptation
  -> validation gate
```

Avoid copying case object names into a different project. A trading project, a
compiler, a SaaS dashboard, and an agent runtime may all need auditable work
assignment, but they should not necessarily share the same object names.

## Case: Fake Assignment Risk

Case detail:

```text
A project allowed a task's assignee field to be updated without proving the
work was actually delivered to the executor.
```

Extracted principle:

```text
Acceptance-critical workflows need auditable events, not only latest-state
fields.
```

Generic adaptation:

- identify the state field that can fake progress;
- identify the event or artifact that proves the workflow step happened;
- make the UI show both latest state and proof event;
- add a review or CI gate when the proof is stable.

Possible gates:

- accepted work item has assignment/delivery proof;
- review cannot accept claims without evidence refs;
- dashboard warns when latest state lacks proof events.

## Case: Historical Notes Polluting Current Spec

Case detail:

```text
An MVP document accumulated run logs, failed attempts, and repair history until
it no longer clearly described current acceptance.
```

Extracted principle:

```text
Current specs describe the target state and acceptance gates. Historical
execution lessons belong in cases, postmortems, or examples.
```

Generic adaptation:

- move incidents, run logs, and old experiments out of normative docs;
- keep the smallest lesson that changes current design;
- link to the case only when future agents need context;
- delete obsolete history when it no longer affects decisions.

Possible gates:

- spec documents contain current requirements and acceptance, not chronology;
- case documents include date, context, lesson, and reusable follow-up;
- docs index distinguishes current spec from history.

## Case: First Integration Pollutes Generic Architecture

Case detail:

```text
The first provider integration had enough detail that it started defining the
generic runtime model by accident.
```

Extracted principle:

```text
The first implementation should prove the generic contract, not become the
generic contract.
```

Generic adaptation:

- write provider-neutral or platform-neutral contracts separately;
- put concrete provider behavior in integration docs;
- route source audits into reference notes;
- promote hard-to-reverse boundary choices into ADRs.

Possible gates:

- generic docs avoid provider-only method names except as examples;
- provider docs cannot redefine core object semantics;
- source audit findings either remain reference notes or update ADR/integration
  docs explicitly.

## Case: UI Backward Design

Case detail:

```text
A dashboard looked useful but did not show whether the workflow actually
happened, where evidence came from, or why a decision was accepted.
```

Extracted principle:

```text
Operational UI should prove acceptance-critical workflow links, not only render
attractive summaries.
```

Generic adaptation:

- start from what the user must judge;
- list which missing states would make the judgment impossible;
- make warnings first-class product features;
- let UI needs expose missing schema, event, or evidence fields.

Possible gates:

- UI fixture shows work, owner, status, evidence, review, and decision;
- warnings appear for missing evidence, stale runtime, failed delivery, or
  source-of-truth gaps where relevant;
- UI actions write through canonical API/CLI/store rather than local display
  state only.

## Case: External Domain Adapter

Case detail:

```text
A generic coordination product needed to operate a domain project without
importing that project's business logic into the generic core.
```

Extracted principle:

```text
Generic systems should expose domain capability through adapters, skills,
tools, artifacts, permissions, and evidence policy.
```

Generic adaptation:

- define what the generic core owns and refuses;
- expose domain commands through descriptors or stable APIs;
- keep domain dashboards and artifacts as evidence links;
- encode risky domain actions with explicit permissions and review gates.

Possible gates:

- adapter descriptor validates;
- representative domain command produces structured evidence;
- generic docs do not embed domain implementation logic;
- permissions and destructive operations have explicit approval paths.
