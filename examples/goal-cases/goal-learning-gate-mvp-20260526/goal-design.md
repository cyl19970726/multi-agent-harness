# GoalDesign

## Goal

- Goal id: `goal-learning-gate-mvp`
- Objective: make GoalDesign and GoalEvaluation CLI-checkable,
  Dashboard-visible, and Review Gate-verifiable.
- Owner: `delegated-lead-kepler`
- Success criteria: staged acceptance is enforced and visible through harness
  objects.

## Scenario

- Scenario summary: repair the gap where the Lead could use external helpers
  or local work and later backfill harness records.
- Non-goals: do not build a full workflow DSL; do not make domain-specific
  strategy logic part of generic harness core.
- Risk and permission boundaries: repo-local docs, CLI, Dashboard, tests, and
  harness evidence only.

## Required Infra

- CLI: goal learning status, strict gates, review-gate integration, waiver
  validation.
- Skill: generic harness skill must require staged acceptance and canonical
  AgentMember records.
- Adapter: none for this self-hosting case.
- Dashboard: expose goal learning status and event-order warnings.
- CI/CD: Rust tests, JS syntax, schema/descriptors/docs governance checks.

## Agent Team

| Member | Role | Owns | Evidence |
| --- | --- | --- | --- |
| `delegated-lead-kepler` | Lead | task graph and decisions | GoalDesign, decision |
| `cli-schema-impl` | CLI implementer | CLI gates and tests | worker reports, checks |
| `dashboard-agent` | Dashboard implementer | Goal Learning tab | dashboard snapshot |
| `schema-cli-agent` | Knowledge/docs | AGENTS, docs, skill | docs report |
| `critic-evaluator` | Critic/Evaluator | P0/P1 review and evaluation | critic findings, GoalEvaluation |

## Task Graph

```text
goal-learning-gate-root
  -> goal-learning-cli-check
  -> goal-learning-review-gate
  -> goal-learning-dashboard-status
  -> goal-learning-skill-doc-update
  -> goal-learning-ci-fixtures
  -> goal-learning-evaluation-case
```

## Evidence Plan

- GoalDesign evidence before assignment.
- Assignment messages from Lead to each AgentMember.
- Worker report messages with evidence refs.
- Check evidence for Rust, JS, and pnpm validation.
- Critic findings before Leader decision.
- GoalEvaluation evidence after Leader decision.

## Acceptance Gates

- `goal learning-status` reports missing stages and event-order health.
- `task assign` blocks missing GoalDesign unless an explicit valid waiver
  decision is provided.
- `review gate` can require GoalDesign and GoalEvaluation.
- Dashboard snapshot exposes goal learning status.
- Final critic reports no P0/P1 issues.
