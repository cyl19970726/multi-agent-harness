# GoalEvaluation

## Outcome

- Status: accepted MVP.
- Evaluator: `critic-evaluator`
- Decision refs:
  - `decision-1779812540876-0`
- Evidence refs:
  - `evidence-goal-learning-gate-goal-design`
  - `evidence-goal-learning-implementation-report`
  - `evidence-goal-learning-waiver-fix`
  - `evidence-goal-learning-checks-passed`
  - `evidence-goal-learning-final-critic-pass`

## What Worked

- The workflow used harness `AgentMember` records as canonical execution.
- The Critic/Evaluator found P1 issues that were fixed before close.
- The final CLI gate can identify missing GoalEvaluation and missing decisions.
- The Dashboard can show goal-learning health instead of hiding it in raw JSON.

## What Failed

- Initial waiver behavior was too permissive.
- External subagent output needed to be explicitly demoted to non-canonical
  input unless recorded through AgentMember reports.
- GoalDesign and GoalEvaluation still rely on evidence conventions.

## Missing Infra

- CLI: first-class goal design/evaluate/case-export commands.
- Skill: needs more cases to tune workflow guidance.
- Adapter: none.
- Dashboard: write actions for decisions, waivers, and follow-ups.
- CI/CD: repository checks pass, but no close-readiness scan of live goals yet.

## Team Design Feedback

- Keep Critic/Evaluator in the team from goal start.
- Assign docs/skill and dashboard as separate roles, not a side effect of CLI
  work.

## Task Graph Feedback

- The root task made staged acceptance easier to inspect.
- Follow-up tasks should be generated automatically from GoalEvaluation once
  commands exist.

## Evidence Feedback

- Evidence refs were enough for MVP.
- Future schema objects should make evaluator fields and waiver references
  machine-checkable without text parsing.

## Event Order Check

- GoalDesign existed before implementation: yes.
- Assignment messages preceded member reports: yes.
- Member reports preceded Leader decision: yes.
- Critic/reviewer output preceded acceptance: yes.
- Lead-local exceptions were recorded: yes.
- Post-hoc evidence risk: reduced, still present until first-class commands
  exist.

## Case Sanitization

- Secrets removed: yes.
- Long logs omitted: yes.
- Provider transcripts summarized: yes.
- Project-specific noise removed: yes.

## Reusable Patterns

- Require GoalDesign before assigning work.
- Require explicit waiver decisions, not bare allow flags.
- Use critic findings as implementation backlog.
- Close goals with GoalEvaluation and a reusable case when the workflow teaches
  a pattern.

## Anti-Patterns

- Lead-local work presented as harness-driven execution.
- Provider chat as the only source of truth.
- Dashboard status that reports ok while close evidence is missing.

## Follow-Up Tasks

- Add `goal design`, `goal evaluate`, and `goal case export`.
- Add Dashboard actions for decision and waiver review.
- Add CI/local gates for staged acceptance before goal close.
