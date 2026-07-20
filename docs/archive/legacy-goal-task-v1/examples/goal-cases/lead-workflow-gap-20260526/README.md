# Lead Workflow Gap, 2026-05-26

## Summary

- Source goal: `self-host-mvp`
- Scenario type: self-hosting product design correction
- Project adapter: none
- Outcome: product definition and harness skill corrected
- Case maturity: early example

## Why This Case Matters

This case captures a core failure mode: the Lead performed work locally and then
recorded harness objects afterward. The corrected pattern is that the Lead must
first design the scenario workflow, infra gaps, agent team, task graph, and
acceptance gates, then use messages, reports, evidence, critic review, and
decisions.

## Artifacts

- Goal design: [goal-design.md](goal-design.md)
- Evaluation: [evaluation.md](evaluation.md)
- Source evidence:
  - `.harness/evidence/phase3f/multi-agent-usage-gap.md`
  - `.harness/evidence/phase3f/critic-lead-workflow-review.md`

## Reusable Patterns

- Put the product thesis in the PRD before adding implementation detail.
- Treat Lead-local work as an exception that needs evidence and a follow-up
  infra task.
- Require task assignment messages before member reports and decisions.
- Use a Critic/Gate agent to evaluate whether harness usage was real.

## Anti-Patterns

- Treating task/message/evidence records as proof by themselves.
- Backfilling agent messages after the Lead has already reached the conclusion.
- Letting provider chat or final summaries replace durable reports.

## Follow-Up Tasks

- Make Agent Dashboard show goal design completeness and event ordering.
- Add CLI/API support for goal evaluation and case generation after the fields
  stabilize.
