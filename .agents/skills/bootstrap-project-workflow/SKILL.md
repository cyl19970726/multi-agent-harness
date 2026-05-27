---
name: bootstrap-project-workflow
description: "Use when Codex needs to bootstrap, audit, or redesign a project's vision-driven docs, architecture, ADRs, CI/CD, task workflow, agent workflow, skills, tool adapters, responsibility boundaries, or evidence-backed acceptance process. Applies to new projects, new requirements, migrations, governance audits, and self-hosting workflow work."
---

# Bootstrap Project Workflow

Use this skill to make a project agent-operable. The output may include docs,
ADRs, schemas, CLI/API contracts, Dashboard plans, CI/CD gates, skills, or a
docs reorg, but those are not the goal. The goal is to help the project reach
its vision by preserving the critical judgments that future humans and agents
must not misunderstand.

Load [references/governance.md](references/governance.md) when the task asks
for a full docs/CI audit, rubric, directory reorg, knowledge lifecycle, or
responsibility-boundary review.

Load [references/cases.md](references/cases.md) only when you need examples,
case-to-principle extraction, or evaluator calibration. Do not copy case object
names into a different project unless that project actually uses those objects.

## Why This Skill Exists

Projects fail through drift before they fail through missing prose. A team can
have many Markdown files and still miss the decisions that decide whether the
product works.

This skill exists because agents often start from the wrong question:

```text
wrong: which document should I write?
right: what must be understood, decided, implemented, observed, and verified
       for the project's vision to become real?
```

Good project documentation is not a pile of explanations. It is an operating
system for judgment:

- it preserves why the product exists;
- it names the final acceptance standard;
- it identifies the critical mechanisms that decide success;
- it captures the relationships and invariants that code alone will not
  protect;
- it routes important knowledge to the correct surface: docs, ADR, schema,
  code, CLI/API, Dashboard, CI/CD, skill, or adapter;
- it evolves as the project grows, including split, merge, archive, and delete.

## Core Principle

Start with both:

```text
vision: what the project is trying to become
final acceptance: how we will know it actually became that
```

Then work backward:

```text
vision + final acceptance
  -> critical path
  -> vision risks
  -> key mechanisms
  -> key modules
  -> object relationships and invariants
  -> data flow and Dashboard visibility
  -> ADRs and source-of-truth routing
  -> schemas / CLI / code / CI / skills
  -> evaluation and reorg
```

Do not write or reorganize docs until this chain is clear enough for the task.

## Operating Loop

Follow this order:

```text
project state
  -> vision and final acceptance
  -> critical path and risks
  -> key mechanisms and modules
  -> relationships, data flow, and invariants
  -> responsibility boundary
  -> artifact routing
  -> diagrams and directory shape
  -> contracts and CI/CD gates
  -> agent workflow
  -> evaluation and reorg
```

If a step cannot be answered, keep exploring. Do not fill the gap with generic
documents.

## First Step

Work in the target project workspace. Read only the context needed:

- `README*`, `AGENTS.md`, `docs/`, `.agents/skills/`
- `.github/workflows/`, `package.json`, `Cargo.toml`, `pyproject.toml`,
  `Makefile`, `scripts/`
- schema/API/CLI/dashboard entry points
- task, adapter, workflow, ADR, or evaluation artifacts

Classify the request:

| Scenario | Goal |
| --- | --- |
| New project | Establish minimal vision, acceptance, architecture, contracts, CI/CD, and agent workflow |
| New requirement | Identify affected key mechanisms, ADRs, docs, contracts, and checks |
| Migration/productization | Separate generic product from project-specific adapter or provider code |
| Docs/CI audit | Find stale docs, missing checks, source-of-truth drift, and reorg needs |
| Agent workflow | Define skills, tool descriptors, tasks, evidence, review, and decision flow |

Classify maturity:

| Status | Meaning |
| --- | --- |
| `idea` | discussed but not designed |
| `planned` | designed, no executable surface |
| `schema-only` | contract exists, behavior missing |
| `implemented` | code exists and basic tests pass |
| `gateable` | CI/CD verifies the commitment |
| `deprecated` | replaced or retained only for transition |

## Required Questions

Answer the relevant questions before writing:

| Question | Why it matters |
| --- | --- |
| What is the vision? | Prevents documentation from becoming folder completion. |
| What is final acceptance? | Defines how the project proves the vision works. |
| Which mechanisms decide success? | Reveals the modules and workflows that need real design. |
| How can each mechanism fail? | Converts concern into architecture risk. |
| Which modules exist to solve those failures? | Prevents decorative module lists. |
| How do the key modules relate? | Drives architecture diagrams and data flow. |
| What object relationships and invariants must hold? | Prevents implementation drift. |
| Which decisions are hard to reverse? | Routes durable tradeoffs into ADRs. |
| What must users see in the Dashboard or UI? | Lets UX needs expose missing state. |
| What belongs in docs, ADR, schema, code, CLI, Dashboard, CI, skill, or adapter? | Keeps source of truth clean. |
| What should be split, merged, archived, or deleted? | Keeps the docs tree aligned with project reality. |

## Genericity Filter

Before adding an insight to this skill or to a target project's main docs,
decide whether it is a principle, a pattern, or a case detail.

| Kind | Keep where | Test |
| --- | --- | --- |
| Principle | `SKILL.md` or governance reference | Applies across unrelated projects without object renaming. |
| Pattern | reference file or target docs | Applies to a class of projects, such as provider integrations or domain adapters. |
| Case detail | `examples/`, case reference, or target project history | Mentions a specific product, object name, incident, market, provider, or command. |

Use this extraction:

```text
case observation
  -> failure mode
  -> generic risk
  -> reusable principle
  -> target-project adaptation
```

Example:

```text
case detail: a project set an assignee field but never delivered the work
generic risk: a state field can fake a workflow event
principle: acceptance-critical workflows need an auditable event, not only a
           latest-state projection
target adaptation: choose the event object appropriate to that project
```

Do not turn a case object name into a universal object model.

## Key Mechanism Card

Use this card for every critical mechanism:

```text
Key Mechanism
  vision_link:
  final_acceptance_signal:
  failure_mode:
  key_modules:
  object_relationships:
  data_flow:
  dashboard_or_ui_view:
  decisions_or_ADRs:
  source_of_truth:
  validation_gate:
  follow_up_infra:
```

For each important module, capture:

```text
Module Core Idea
  purpose:
  vision_risk_solved:
  owns:
  refuses:
  invariants:
  inputs_outputs:
  internal_submodules_when_needed:
  failure_modes:
  evidence:
```

If a module is large enough that its internal model matters, add a focused
subdocument or subdirectory only after the parent module's purpose,
relationships, and invariants are clear. Child docs should deepen the parent
idea, not create a parallel taxonomy.

## Artifact Routing

Route knowledge to the correct surface:

| Surface | Owns | Refuses |
| --- | --- | --- |
| Docs | vision, acceptance, boundaries, relationships, invariants, workflows | field truth, command truth, runtime truth |
| ADR | durable decision, options, tradeoff, consequences | runbooks or unstable brainstorms |
| Schema | stable fields, enums, required properties, cross-surface contracts | product explanation |
| Code | behavior, validation, persistence, runtime logic | product narrative |
| CLI/API | shortest executable path, JSON output, diagnostic failures | prose-only output |
| Dashboard/UI | operational read model and human judgment surface | replacing domain engines or CI |
| CI/CD | stable promise verification and regression gates | every immature warning as blocker |
| Skill | agent operating procedure | complete architecture spec |
| Adapter | project-specific tools, permissions, and evidence policy | generic product runtime |

Stable fields move to schema. Stable operations move to CLI/API. Stable views
move to Dashboard. Stable commitments move to CI/CD. Stable agent procedures
move to skills. Docs keep the reason, boundary, exception, and upgrade rule.

## ADR Routing

Create or update an ADR when a decision:

- changes object relationships or invariants;
- chooses a source of truth;
- defines provider-neutral or platform-neutral interfaces;
- separates generic core from provider-specific or project-specific adapters;
- changes task, PR, review, message, evidence, or acceptance workflow;
- changes Dashboard control-plane responsibilities;
- is hard to reverse after schema, migration, integration, or user workflow
  adoption.

An ADR should include:

```text
Decision
  context:
  options:
  chosen_direction:
  consequences:
  affected_modules:
  affected_contracts:
  validation_or_migration_path:
```

## Diagrams And Directory Shape

Use diagrams when text hides structure. Prefer Mermaid because it is diffable.

| Diagram | Answers |
| --- | --- |
| Context | what is inside or outside the system |
| Architecture | which key modules exist and how they relate |
| Data flow | where data, artifacts, state, and evidence move |
| Workflow | how a scenario reaches acceptance |
| Sequence | how people, agents, CLI/API, provider runtime, and Dashboard interact |
| Lifecycle | how task, message, agent, evidence, proposal, or decision state changes |

Architecture diagrams should follow key mechanisms. Once key modules are known,
draw their relationships directly. When a key module becomes internally
complex, add a focused internal diagram for its submodules, state, or data
flow.

Use questions to choose diagrams:

| Key question | Natural diagram | Example |
| --- | --- | --- |
| Which modules exist and how do they depend on each other? | Architecture graph | Product intent, workflow engine, integration layer, evidence store, UI, decision gate. |
| Where does state or evidence move? | Data-flow graph | External events become normalized records, evidence, review inputs, and UI warnings. |
| How does a scenario finish? | Workflow graph | User request to design, assignment, execution, report, review, decision, learning. |
| What happens over time between actors? | Sequence diagram | Coordinator assigns work, executor runs tools, system records events, reviewer decides. |
| How does one object change state? | Lifecycle diagram | Work item proposed, assigned, running, blocked, review, accepted, archived. |
| What is inside a large module? | Internal module diagram | Runtime split into supervisor, queue, event reducer, context packer, integration client. |

Example mapping:

```text
Question: How do work item, communication event, executor, and runtime relate?
Diagrams:
  architecture graph for module relationships
  sequence diagram for assignment delivery
  lifecycle diagram for communication or delivery status
Docs:
  concept model for meaning
  data-model.md for source-of-truth rules
  runtime/integration doc for delivery
  dashboard or UI doc for what users must see
ADR:
  assignment is recorded by an auditable event, not only latest-state mutation
```

Docs should grow with project state:

- keep the tree flat during exploration;
- split when reader, lifecycle, module, or machine consumer differs;
- split when a key module's internal details make the parent diagram
  unreadable;
- use `docs/integration/<provider>.md` for provider-specific integrations;
- reorg when the tree no longer reflects system layers, relationships, or
  evidence flow;
- archive or delete stale and decorative docs.

## Provider And Adapter Boundary

For integration-heavy projects, separate generic contracts from concrete
implementations:

```text
docs/<runtime-or-interface>.md        # provider-neutral or platform-neutral contract
docs/integration/<provider>.md        # provider-specific implementation
examples/adapters/<project>/          # project-specific tool and evidence policy
```

Do not let the first implementation define the generic architecture.

## CI/CD

CI/CD should verify stable project commitments, not only file format.

Early gates usually include:

- markdown links;
- JSON/schema parse;
- doc size and split rationale;
- skill metadata;
- code format, lint, and tests.

Promote deeper gates when stable:

- schema fixture validation;
- CLI/API output snapshots;
- adapter descriptor conformance;
- dashboard fixture render;
- critical diagram parse/render;
- ADR and docs governance metadata;
- architecture invariants such as "accepted task has evidence" or "assigned
  task has delivery event".

Use `warning` for immature commitments and `blocker` for stable high-risk
commitments.

## Evaluator Mechanism

Use an independent evaluator when the quality of the docs, architecture, CI/CD,
or skill itself matters. The evaluator should test whether the skill can drive
another agent toward the desired outcome, not whether the original author can
explain the answer.

Load [references/evaluator.md](references/evaluator.md) for the full evaluator
protocol, prompt shape, passing criteria, and a concrete reference case.
The minimum metadata is:

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

`expected_outcome_class` names the kind of result expected, not the exact
answer. Record base commit and branch so the evaluation is reproducible.

## Examples And Cases

Keep examples out of the main workflow unless they are needed. Load
[references/cases.md](references/cases.md) when you need concrete examples of:

- turning a project-specific incident into a generic principle;
- separating generic contracts from concrete integrations;
- designing UI backward from acceptance-critical visibility;
- extracting historical execution notes into reusable cases instead of the
  current spec.

## Agent Workflow

Expose project capability through skills and adapters:

```text
User request
  -> coordinator identifies vision link and acceptance
  -> coordinator identifies key mechanisms and risks
  -> coordinator routes docs / ADR / contracts / checks
  -> coordinator creates work plan or task graph
  -> coordinator sends an auditable assignment event
  -> executor uses skill + adapter or project tools
  -> executor returns report with evidence refs
  -> reviewer checks evidence
  -> decision owner records outcome
  -> evaluation updates docs, ADRs, skills, tools, or gates
```

## Completion Checklist

Report:

- vision and final acceptance used;
- key mechanisms and modules identified;
- important relationships, invariants, and data flows captured;
- ADRs created or declared unnecessary;
- docs, schemas, CLI/API, Dashboard, CI/CD, skills, or adapters changed;
- reorg, archive, or delete decisions;
- validation commands run;
- remaining risks that still threaten final acceptance.
