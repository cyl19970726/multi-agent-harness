# Goal Learning Gate MVP, 2026-05-26

## Summary

- Source goal: `goal-learning-gate-mvp`
- Scenario type: self-hosting workflow gate
- Project adapter: none
- Outcome: accepted MVP, final critic PASS
- Case maturity: reusable MVP case

## Why This Case Matters

This case captures the corrected self-hosting workflow for Multi-Agent
Harness: the Lead must design the goal, assign work through harness
`AgentMember` messages, collect reports and evidence, run critic review, record
a Leader decision, and only then produce GoalEvaluation.

The main lesson is that multi-agent operation is not proven by chat context or
external subagent output. It is proven by ordered harness records.

## Artifacts

- Goal design: [goal-design.md](goal-design.md)
- Evaluation: [evaluation.md](evaluation.md)
- Source evidence:
  - `.harness/evidence/phase3g/delegated-lead-goal-design.md`
  - `.harness/evidence/phase3g/goal-learning-implementation-report.md`
  - `.harness/evidence/phase3g/goal-learning-waiver-fix.md`
  - `.harness/provider-sessions/session-1779812417133-0/last-message.md`
  - `.harness/evidence/phase3g/goal-learning-gate-evaluation.md`

## Reusable Patterns

- Start with GoalDesign before implementation.
- Make stage acceptance explicit in `AGENTS.md` and skill guidance.
- Use `AgentMember` reports and evidence refs as canonical execution.
- Let critic findings drive code gates and tests.
- Distill the finished goal into a reusable case.

## Anti-Patterns

- Backfilling messages after local Lead work.
- Treating external subagents as canonical harness members.
- Using bare `--allow-*` flags to skip lifecycle stages.
- Closing goals without GoalEvaluation.

## Follow-Up Tasks

- Add first-class GoalDesign and GoalEvaluation commands.
- Add dashboard write actions for decisions and follow-up tasks.
- Promote stable goal-learning fields into schemas.
