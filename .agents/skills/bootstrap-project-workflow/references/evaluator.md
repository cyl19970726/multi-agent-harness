# Evaluator Reference

Use an independent evaluator when the quality of the docs, architecture, CI/CD,
or skill itself matters. The evaluator tests whether the skill can drive
another agent toward the desired outcome, not whether the original author can
explain the answer.

## Setup

```text
baseline worktree:
  create or select a clean worktree that represents the pre-improvement state
  record base_commit and branch
  do not leak the intended final structure or exact fix

evaluator agent:
  receives the skill, raw repo state, user goal, and final acceptance standard
  performs or designs the docs/architecture reorg in that worktree
  reports changed files, decisions, diagrams, checks, and remaining gaps

lead review:
  compares evaluator output against final acceptance
  records what the skill made easy, what it missed, and which prompts/rules need improvement
```

Record metadata:

```text
SkillEvaluation
  base_commit:
  evaluator_worktree:
  evaluator_agent:
  user_goal:
  final_acceptance:
  expected_outcome_class:
  changed_paths:
  checks:
  reviewer_findings:
  skill_followups:
```

`expected_outcome_class` describes the kind of result expected, not the exact
answer. Use it to avoid leaking the target solution while still making the
evaluation judgeable.

## Prompt Shape

```text
Use the bootstrap-project-workflow skill on this repository state.
Goal: <user goal>
Final acceptance: <observable success criteria>
Do not optimize for producing many docs. Identify the critical mechanisms,
key modules, object relationships, ADRs, diagrams, CI gates, and reorg needed.
Edit only the evaluator worktree or return a plan if editing is not allowed.
Report evidence, changed paths, and gaps that still threaten final acceptance.
```

## Passing Criteria

- evaluator starts from vision and final acceptance, not file templates;
- key mechanisms and failure modes are identified before docs are proposed;
- module relationships naturally produce architecture/data-flow/workflow or
  lifecycle diagrams where useful;
- provider-specific and generic contracts are separated when relevant;
- ADRs are proposed for hard-to-reverse decisions;
- stable commitments are routed to schema, CLI/API, Dashboard, CI/CD, or skill;
- stale or misplaced docs are split, merged, archived, or deleted;
- result is reviewable from files, diffs, checks, and notes.

## Reference Case: Multi-Agent Harness

This is a concrete evaluator case for this repository. Use it to calibrate
quality, not as a generic object model for other projects.

```text
base_commit: 53c6ae2 Rework docs around critical mechanisms
final_acceptance: this project can use persistent AgentMembers, task graph,
message delivery, evidence, review, decision, and Dashboard visibility to
develop itself.
expected_outcome_class: similar quality to a commit that reorganizes docs
around critical mechanisms, adds data-model / agent-runtime / dashboard /
Git-PR workflow / integration guidance, records ADRs, updates skills and
registry, and passes repository checks.
expected_discovery: docs must cover data model, agent runtime, dashboard,
Git/PR workflow, provider integration, and ADRs; task assignment must be
message-delivered; provider transcript is evidence, not source of truth.
```
