# GoalDesign

## Goal

- Goal id: `self-host-mvp`
- Objective: make Multi-Agent Harness capable of managing its own development.
- Owner: `leader`
- Success criteria: repository changes should be planned, assigned, reported,
  reviewed, decided, and made visible through harness objects instead of chat
  history alone.

## Scenario

- Scenario summary: correct the product definition after discovering that the
  Lead used the harness mostly as a record layer.
- Non-goals: do not add domain-specific Earning Engine logic to the generic
  core; do not create a large workflow DSL before the basic protocol is stable.
- Risk and permission boundaries: this is docs/skill/product architecture work,
  not live trading or secret-touching work.

## Required Infra

- CLI: existing task/message/evidence/decision commands are enough for this
  design correction.
- Skill: `generic-agent-harness` must include the Lead workflow gate.
- Adapter: none.
- Dashboard: should later show goal design completeness and event ordering.
- CI/CD: docs, links, skill, schema fixture, and Rust tests must stay green.

## Agent Team

| Member | Role | Owns | Evidence |
| --- | --- | --- | --- |
| `leader` | Lead | product decision and task graph | decision record |
| `codex-impl` | Implementation | docs, skills, examples | diff and check outputs |
| `ee-critic-gate` | Critic/Gate | challenge missing workflow protocol | critic findings |

## Task Graph

```text
self-host-mvp
  -> harness-strategy-matrix-agentization
  -> harness-goal-learning-loop-design
```

## Evidence Plan

- PRD and skill diff.
- Critic findings.
- `npx pnpm@9.15.4 check`.
- `cargo test`.

## Acceptance Gates

- PRD states the harness turns scenarios into agent-operable workflows.
- Skill requires Lead workflow order and rejects backfilled harness usage.
- Goal learning loop is documented with case-library storage.
