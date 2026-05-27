---
name: bootstrap-project-workflow
description: "Use when Codex needs to bootstrap, audit, or redesign a project's vision-driven docs, architecture, ADRs, CI/CD, task workflow, agent workflow, skills, tool adapters, responsibility boundaries, or evidence-backed acceptance process. Applies to new projects, new requirements, migrations, governance audits, and Multi-Agent Harness itself."
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
| Which modules exist and how do they depend on each other? | Architecture graph | Goal, Task Graph, Message System, Agent Runtime, Dashboard, Evidence, Decision. |
| Where does state or evidence move? | Data-flow graph | Provider events become AgentEvents, Evidence, Proposal updates, Dashboard warnings. |
| How does a scenario finish? | Workflow graph | User request to GoalDesign, task assignment, worker report, critic review, Leader decision. |
| What happens over time between actors? | Sequence diagram | Leader sends Message(kind=task), runtime delivers turn, provider emits events, harness records report. |
| How does one object change state? | Lifecycle diagram | Message queued, delivered, acknowledged, failed; Task planned, assigned, running, review, done. |
| What is inside a large module? | Internal module diagram | Agent Runtime split into provider gateway, event reducer, queue, context packer, supervisor. |

Example mapping:

```text
Question: How do Task, Message, AgentMember, and AgentRuntime relate?
Diagrams:
  architecture graph for module relationships
  sequence diagram for assignment delivery
  lifecycle diagram for message status
Docs:
  concept-model.md for meaning
  data-model.md for source-of-truth rules
  agent-runtime.md for runtime delivery
  dashboard.md for what users must see
ADR:
  task assignment is message-delivered, not field-mutated
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

## Examples

### Example: Multi-Agent Harness Self-Development

Vision:

```text
Use persistent agent members, task graph, message delivery, evidence, review,
decision, and goal evaluation to develop the harness itself.
```

Final acceptance:

```text
The system can create agent members, send tasks through messages, observe
runtime state, collect reports/evidence, run critic review, record decisions,
and show the chain in the Agent Dashboard.
```

Key mechanism:

```text
Task assignment
  failure_mode: directly setting assignee makes fake assignment look real
  modules: Task Graph, Message System, Agent Control Plane, Dashboard
  invariant: Message(kind=task) is the assignment event
  ADR: task assignment is message-delivered, not field-mutated
  gate: accepted assigned task has prior delivered task message
```

### Example: Provider Integration

Vision:

```text
Support Codex first, then other providers without rewriting core workflow.
```

Routing:

```text
docs/agent-runtime.md        # provider-neutral AgentProvider, MessageDelivery, EventReducer
docs/integration/codex.md    # Codex app-server, hooks, plugins, fallback modes
docs/integration/openclaw.md # OpenClaw-specific implementation when added
ADR                         # provider transcript is evidence, harness store is canonical
```

### Example: Dashboard Backward Design

Question:

```text
What must the user see to know agents are really working?
```

Implication:

```text
Dashboard needs task graph, team roster, member status, inbox/outbox, delivery
state, runtime timeline, reports, evidence, proposals, review, and decisions.
Therefore the data model needs message delivery state and runtime events.
```

### Example: External Project Adapter

Vision:

```text
Let agents operate a domain project through stable tools without importing
domain logic into the generic harness.
```

Routing:

```text
adapter docs: project CLI/API/dashboard/artifacts/evidence policy
skill: how agents should use those tools
schema/CLI: stable machine-readable outputs
dashboard link: domain evidence view
CI: adapter descriptor and command fixture checks
```

## Agent Workflow

Expose project capability through skills and adapters:

```text
User request
  -> Lead identifies vision link and acceptance
  -> Lead identifies key mechanisms and risks
  -> Lead routes docs / ADR / contracts / checks
  -> Lead creates task graph
  -> Lead sends Message(kind=task)
  -> AgentMember uses skill + adapter
  -> AgentMember returns report with evidence refs
  -> Reviewer/Critic checks evidence
  -> Leader records decision
  -> Goal evaluation updates docs, ADRs, skills, or gates
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
