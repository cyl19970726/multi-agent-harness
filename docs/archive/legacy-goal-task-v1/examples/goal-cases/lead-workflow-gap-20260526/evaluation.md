# GoalEvaluation

## Outcome

- Status: corrected and kept as a reusable early case.
- Evaluator findings: Critic/Gate subagent.
- Lead synthesis: applied the critic findings to PRD, architecture, skills, and
  examples.
- Decision refs:
  - `decision-1779809552623-0`
- Evidence refs:
  - `evidence-strategy-matrix-agentization-gap`
  - `evidence-lead-workflow-critic-review`

## What Worked

- User feedback exposed the gap between recorded harness objects and actual
  multi-agent operation.
- Critic/Gate review identified the missing Lead workflow as a P0 design gap.
- The fix updated PRD, README, `generic-agent-harness`, and
  `bootstrap-project-workflow` instead of burying the lesson in chat.

## What Failed

- The Lead initially treated the harness as a coordination record, not as the
  operating system for scenario design and task execution.
- Role-specific strategy-matrix agents were created too late.
- Event ordering was not originally part of the acceptance gate.

## Missing Infra

- CLI: no first-class `goal design` or `goal evaluate` command yet.
- Skill: fixed for now, but should be tested in more goal cases.
- Adapter: no issue for this self-hosting case.
- Dashboard: cannot yet show goal design completeness or event-order violations
  clearly enough.
- CI/CD: no gate yet checks that non-trivial goals have an evaluation.

## Team Design Feedback

- Lead should design the team before execution, not after user challenge.
- Critic/Gate should be assigned at goal design time for non-trivial goals.

## Task Graph Feedback

- Future goals need a root goal design task before implementation tasks.
- Follow-up infra tasks should be created when manual work repeats.

## Evidence Feedback

- Evidence was useful but partly post-hoc. Future cases should preserve
  assignment-before-report-before-decision ordering.

## Event Order Check

- GoalDesign existed before implementation: no, this was the gap that produced
  the case.
- Assignment messages preceded member reports: partial; later corrective tasks
  did, earlier strategy-matrix work did not.
- Member reports preceded Leader decision: partial.
- Critic/reviewer output preceded acceptance: yes for the corrected design.
- Lead-local exceptions were recorded: yes, as the usage-gap evidence.
- Post-hoc evidence risk: high in the original workflow, reduced after the
  correction.

## Case Sanitization

- Secrets removed: yes.
- Long logs omitted: yes.
- Provider transcripts summarized: yes.
- Project-specific noise removed: yes.

## Reusable Patterns

- Make product definition changes in PRD first, then update skill and examples.
- Keep raw runtime traces in `.harness`; keep reusable lessons in
  `examples/goal-cases`.
- Use Critic/Gate to reject fake or backfilled harness usage.

## Anti-Patterns

- Lead-local execution presented as multi-agent execution.
- Final chat summaries treated as durable evaluation.
- Dashboard visibility deferred until after the workflow becomes confusing.

## Follow-Up Tasks

- Add dashboard panels for goal design, event ordering, and evaluator verdicts.
- Add CLI support for creating `GoalDesign`, running `GoalEvaluation`, and
  exporting `GoalCase` after fields stabilize.
- Add CI warning for accepted goals without an evaluation artifact once the
  convention is stable.
