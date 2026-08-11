# Development system pattern

## Separate the three lifecycles

Use three authorities when a development system must represent durable work, repeated execution, and governed documents:

| Authority | Owns | Must not own |
|---|---|---|
| Development Work | objective, scope, overall state, priority, owner, next action | each execution attempt or full document bodies |
| Delivery Runs | one attempt/candidate, assignee or provider, run state, timestamps, result | the durable work state or canonical specification |
| Development Documents | typed authored artifacts and their document status | live execution state duplicated from a run |

This separation supports one work item with many candidates, retries, or reviews without overwriting history.

## Keep the document set intentional

Default to three document types unless a real governance need proves another type:

- **Specification:** desired behavior, scope, constraints, acceptance criteria, and open decisions.
- **Execution Report:** approach, chronological journal when useful, changed artifacts, checks, deviations, and completion summary.
- **Review Report:** reviewer findings, evidence assessment, gate outcome, and required follow-up.

Put journal and completion inside the Execution Report instead of creating separate generic document types. A document type should exist because it has distinct authorship, audience, lifecycle, or gate semantics—not because it is another phase label.

## Model relations semantically

Recommended minimum connections:

- A Run belongs to exactly one Work item.
- A Document belongs to one Work item.
- An Execution Report normally identifies the Run it reports.
- A Review Report identifies the Run or proposal it evaluates when that distinction matters.
- A Specification may govern multiple Runs under the same Work item.

Use clear property names such as `Work`, `Run`, `Specification`, and `Review Report`. Avoid a single polymorphic `Related pages` relation.

## Assign state to the correct owner

- Work owns overall lifecycle and next action.
- Run owns attempt state and execution outcome.
- Review Report owns the reviewer recommendation or gate finding.
- The accepted Decision, if modeled separately, owns acceptance.

Do not infer that Work is complete merely because a Run finished. Do not make the Review Report silently overwrite Run history. Rollups may summarize authoritative state, but should not create a second editable state field.

## Build the Work page as a cockpit

The default Work page should answer:

1. What outcome is being pursued?
2. What is the current decision/state?
3. What happens next, and who owns it?
4. Which specification governs the work?
5. Which runs occurred or are active?
6. What execution and review evidence exists?

Use filtered linked views of Delivery Runs and Development Documents rather than copied tables. Keep the narrative concise and place exceptional decisions in context.

## Support candidate and retry history

Represent parallel candidates, provider sessions, retries, or remediation attempts as separate Runs. Preserve abandoned or rejected runs with explicit status. Never reuse one run record so aggressively that evidence from a prior attempt disappears.

## Avoid common collapses

Reject designs that:

- place Work, Run, and Document states in one row;
- make a document URL field stand in for a relation;
- create one mega-page for spec, live journal, completion, and review;
- copy run status into several documents;
- use generic page relations to recreate an implicit graph;
- put every implementation log in the canonical knowledge wiki.

Development evidence may link to canonical architecture docs, but execution artifacts and durable knowledge have different lifecycles and should remain distinguishable.
