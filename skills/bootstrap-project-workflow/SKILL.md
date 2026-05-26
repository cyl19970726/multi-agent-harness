---
name: bootstrap-project-workflow
description: "Use when Codex needs to bootstrap, audit, or redesign a project's docs, CI/CD, task system, agent workflow, skills, tool adapters, responsibility boundaries, or evidence-backed acceptance process for a new project, new requirement, or project migration. Applies to generic projects and to Multi-Agent Harness itself; keep domain tools behind adapters."
---

# Bootstrap Project Workflow

Use this skill to turn a project or new requirement into a usable agent-facing
workflow: docs explain the system thinking, contracts stabilize behavior,
CI/CD verifies commitments, and skills tell agents how to operate.

Load [references/governance.md](references/governance.md) when the task asks
for a full docs/CI audit, rubric, directory reorg, knowledge lifecycle, or
responsibility-boundary review.

## Core Chain

Follow this order. Do not start by creating directories.

```text
project state
  -> design basis
  -> responsibility boundary
  -> scenario workflow
  -> docs / diagrams
  -> machine contracts
  -> CI/CD gates
  -> agent workflow
  -> evaluation and reorg
```

This chain is the main test of the skill. If a step cannot be answered, keep
exploring before adding docs or checks.

## First Step

Work in the target project workspace. Read only the context needed:

- `README*`, `AGENTS.md`, `docs/`, `skills/`
- `.github/workflows/`, `package.json`, `Cargo.toml`, `pyproject.toml`,
  `Makefile`, `scripts/`
- schema/API/CLI/dashboard entry points
- existing task/adapter/workflow artifacts

Classify the task:

| Scenario | Goal |
| --- | --- |
| New project | Establish minimal docs, contracts, CI/CD, and task workflow |
| New requirement | Add the missing PRD, architecture, acceptance, and checks |
| Migration/productization | Separate generic harness from project adapter |
| Docs/CI audit | Find stale docs, missing checks, and source-of-truth drift |
| Agent workflow | Define skills, tool descriptors, tasks, evidence, and decisions |

Also classify maturity:

| Status | Meaning |
| --- | --- |
| `idea` | only product thinking or rough docs |
| `planned` | design exists but no executable surface |
| `schema-only` | contract exists but behavior is not implemented |
| `implemented` | code and tests exist |
| `gateable` | CI/CD verifies the commitment |
| `deprecated` | kept only for transition or history |

## Design Basis

Before writing a docs tree, write the design basis:

```text
Design Basis
  core_thesis
  layers
  module_core_ideas
  relationships
  progression
  governance
```

For each important module, capture:

```text
Module Core Idea
  purpose
  owns
  refuses
  invariants
  inputs_outputs
  failure_modes
  evidence
```

Good docs express system thinking: why these layers exist, why these modules
exist, how child modules deepen the parent idea, and which commitments must be
validated by code, schema, CLI, dashboard, or CI.

## Responsibility Boundary

Assign every important claim to the right surface. This prevents docs from
pretending to be implementation, and prevents code from hiding product intent.

| Surface | Owns | Refuses |
| --- | --- | --- |
| Docs | why, boundaries, scenarios, design basis, operating path | field truth, command truth, runtime truth |
| Skill | how an agent should use project surfaces | being a second architecture spec |
| Schema | stable fields, enums, required properties, cross-surface contracts | business explanation and unstable experiments |
| Code | real behavior, validation, storage/API/adapter logic | product narrative and unimplemented roadmap |
| CLI | shortest executable path, JSON output, diagnostic failures | prose-only output and hidden evidence |
| CI/CD | verifying current commitments | blocking on unproven guesses |
| Dashboard | coordination read model and links to evidence | replacing project domain dashboards |
| Adapter | project-specific tools, permissions, and evidence policy | generic harness runtime logic |

When auditing, output:

```text
doc_claim -> source_of_truth -> owner_surface -> current_check -> missing_gate
```

Stable fields move to schema. Stable operations move to CLI/API. Stable
commitments move to CI/CD. Stable views move to Dashboard. Docs keep the
reason, boundary, exception, and upgrade rule.

## Docs And Diagrams

Start with the smallest useful docs:

| Doc | Role |
| --- | --- |
| `README.md` | entry point, boundary, fastest path |
| `docs/prd.md` | motivation, scenarios, non-goals, success |
| `docs/design-basis.md` | layers, module core ideas, design reasoning |
| `docs/architecture.md` | modules, data flow, contracts, packages |
| `docs/operations.md` | run, check, release, recover |
| `docs/schemas.md` | machine contracts and maturity |
| `docs/decisions.md` | durable tradeoffs |

Use diagrams for complex systems. Prefer Mermaid because it is diffable and can
be checked by CI. At minimum consider context, architecture, data flow,
workflow, sequence, lifecycle, and deployment diagrams.

The docs tree grows with the project state:

- keep it flat during exploration;
- split when readers, lifecycles, modules, or machine consumers stabilize;
- reorg when the tree no longer reflects actual system layers, relationships,
  or evidence flow;
- delete empty or decorative directories.

## Contracts And CI/CD

Define machine contracts only after the design basis and responsibility
boundary are clear. Prefer JSON schema, CLI JSON output, append-only artifacts,
and dashboard/evidence refs before inventing a workflow DSL.

CI/CD should verify promises, not just run tools.

Current or early gates usually include:

- markdown links;
- JSON/schema parse;
- doc size and split rationale;
- skill metadata;
- Rust/code fmt, lint, test.

Planned gates can include:

- schema fixture validation;
- adapter descriptor conformance;
- CLI help/output snapshots;
- Rust/schema coverage checks;
- dashboard build and fixture render;
- Mermaid render/lint;
- docs governance metadata and stale review checks.

Use `warning` for immature commitments and `blocker` for stable high-risk
commitments.

## Agent Workflow

Expose project capability to agents through skills and adapters:

```text
User Request
  -> Leader creates/updates Task
  -> Leader sends Message kind=task
  -> AgentMember uses Skill + Tool Adapter
  -> AgentMember returns report with Evidence refs
  -> Reviewer/Critic checks evidence
  -> Leader records Decision
```

Every agent member needs role, capabilities, allowed tools, forbidden actions,
expected evidence, and acceptance responsibility.

Every adapter needs command/API/dashboard entry, inputs, outputs, evidence
shape, permission level, failure modes, and compatibility/maturity.

## Knowledge Lifecycle

Knowledge should enter and leave the main path deliberately:

```text
docs -> skill -> schema -> CLI/API -> dashboard -> plugin
```

Archive or delete knowledge when a better source of truth exists. A runbook
replaced by a CLI command should link to the command, not keep duplicating the
procedure. A prose field definition replaced by schema should become a short
explanation and link to the schema.

## Subagent Use

When the user asks for multi-agent review, use bounded independent agents:

| Agent | Review focus |
| --- | --- |
| Architect | layers, module core ideas, directory shape |
| Governance | ownership, lifecycle, stale docs, reorg protocol |
| Boundary | docs/code/schema/CLI/CI/Dashboard responsibility |
| Critic | over-design, missing evidence, hidden coupling |

Pass raw artifacts and questions. Do not pass expected answers. Integrate the
results locally and decide what becomes docs, skill, schema, CLI, or CI.

## Completion Checklist

Report:

- docs, skills, schemas, or CI checks changed;
- current maturity and remaining planned/gateable boundaries;
- each major scenario's path to evidence-backed decision;
- claims still lacking source of truth or CI gates;
- validation commands run.
