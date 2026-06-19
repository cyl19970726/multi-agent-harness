# Documentation Governance Reference

Use this reference when a project needs a full documentation, CI/CD, or
workflow-governance audit.

## What Good Docs Express

Good documentation expresses system thinking. It is not a storage bucket for
facts and commands.

| Expression | Meaning |
| --- | --- |
| Motivation | why the project exists and what real problem it solves |
| Experience | how users or agents should complete key tasks |
| Boundary | what the system owns, refuses, and delegates |
| Module core ideas | why important modules exist and what must not break |
| Relationships | how modules exchange data, control, and evidence |
| Progression | how details deepen the parent module instead of forming random branches |
| Judgment | what good, blocked, risky, or done means |
| Evolution | when docs become skill, schema, CLI, dashboard, or plugin |

## Required Questions

| Question | Why it matters |
| --- | --- |
| What is the core design basis? | Avoid listing modules without explaining the decomposition. |
| What is each surface's responsibility? | Prevent docs, code, schema, CLI, CI, and dashboard from overlapping incorrectly. |
| What is implemented versus planned? | Prevent future concepts from looking gateable. |
| What is the source of truth for each claim? | Prevent stale docs and unsupported conclusions. |
| Who owns each durable document or contract? | Avoid "whoever notices fixes it" governance. |
| What should be machine-readable? | Move stable contracts out of prose. |
| What should expire or be archived? | Keep the main path clean for agents. |
| Which commitments are CI blockers? | Stop stable high-risk drift early. |

## Documentation Lifecycle Metadata

For durable docs, maintain a lightweight registry or frontmatter with:

```text
path
owner_role
status: idea | planned | stable | deprecated | archival
lifecycle: volatile | stable | archival
canonical_for
depends_on
machine_consumers
review_after
last_verified_with
reorg_trigger
```

In this repository the canonical implementation is `docs/registry.json` (schema
`agent_harness.docs_registry.v1`), enforced by
`scripts/check-doc-governance.mjs`, which requires **camelCase** keys
(`ownerRole`, `canonicalFor`, `dependsOn`, `machineConsumers`, `reviewAfter`,
…). Mirror that shape when emitting registry entries; the snake_case names above
are illustrative only.

CI can start by warning on stale `reviewAfter`, missing owner roles, broken
`dependsOn`, and large docs without a split reason. Promote warnings to
blockers only when the repo relies on them for release or agent operations.

## Reorg Protocol

Reorganize docs when the current tree no longer mirrors the actual system
layers, module relationships, or evidence flow.

Checklist:

1. State the design reason for the reorg.
2. Preserve or update old links.
3. Update `README.md`, `docs/README.md`, skills, and CI roots.
4. Move owner/lifecycle metadata with the document.
5. Delete empty directories and decorative placeholders.
6. Run link checks and any governance checks.

Do not create deep directories for one file unless a tool consumes that path.

## Surface Responsibility Matrix

| Surface | Should own | Should not own |
| --- | --- | --- |
| Docs | design basis, boundaries, scenarios, operating path, exceptions | field truth, command truth, runtime state, test truth |
| Skill | agent operating instructions and reusable workflow | complete product docs or embedded domain implementation |
| Schema | fields, enums, required properties, contract versions | business explanation or unstable experiments |
| Code | real behavior, validation, persistence, API/adapter logic | product thesis or future roadmap |
| CLI | repeatable executable path, JSON output, diagnostic failures | prose-only output, hidden evidence, dashboard replacement |
| CI/CD | current commitment verification and release gates | unproven guesses or every warning as blocker |
| Dashboard | coordination read model and evidence links | project domain UI or direct domain verdicts |
| Adapter | project tools, permissions, evidence policy | generic runtime behavior |

Audit output:

```text
doc_claim -> source_of_truth -> owner_surface -> current_check -> missing_gate
```

## Contract Maturity

Use explicit maturity labels:

| Label | Meaning |
| --- | --- |
| `idea` | discussed but not designed |
| `planned` | designed, no executable surface |
| `schema-only` | machine contract exists, behavior missing |
| `example-only` | example adapter or fixture only |
| `implemented` | code exists and basic tests pass |
| `gateable` | CI/CD verifies the commitment |
| `deprecated` | replaced or retained only for transition |

Do not call something stable unless the relevant surface owns it and CI can at
least detect basic drift.

## Knowledge Lifecycle

Knowledge enters the main path only if it helps future humans or agents make a
better decision.

Progression:

```text
note -> docs -> skill -> schema -> CLI/API -> dashboard -> plugin
```

Exit rules:

- Delete duplicate prose once schema or CLI owns the truth.
- Archive runbooks replaced by safer commands.
- Mark obsolete design docs as deprecated and link to replacements.
- Remove empty directories and placeholder docs.
- Keep `decisions` only for tradeoffs that future work should respect.

## Diagram Governance

Use diagrams when text alone hides system structure.

| Diagram | Answers |
| --- | --- |
| Context | what is inside or outside the system |
| Architecture | which modules exist and how they depend |
| Data flow | where data, artifacts, and evidence move |
| Workflow | how a scenario reaches acceptance |
| Sequence | how agents, CLI, API, and dashboard interact over time |
| State/lifecycle | how task, message, evidence, decision, or agent status changes |
| Deployment | how runtime components run locally, in CI, and in production |

Prefer Mermaid. Over roughly 12 nodes, split the diagram.

Diagram review:

- purpose is stated;
- boundaries are visible;
- arrows have meaningful direction;
- node names match docs/schema/code;
- source is diffable or regenerable;
- CI can at least parse or render critical diagrams when they become release
  commitments.

## Documentation Rubric

Score each item `0-3`: `0` missing, `1` present but vague, `2` usable, `3`
verified and low-friction.

| Dimension | Good | Bad |
| --- | --- | --- |
| Motivation and boundary | explains why, for whom, and non-goals | slogans and folders only |
| Design basis | explains layers, module core ideas, and tradeoffs | lists modules without why |
| Scenario coverage | trigger to acceptance is traceable | module descriptions without workflows |
| Executability | new agent can find commands, entry points, evidence | requires chat history |
| Accuracy | paths, commands, schemas, statuses match reality | stale commands, broken links |
| Evidence | claims have proof or validation path | unsupported "done" statements |
| Layering | docs have distinct roles | all content in one large file |
| Context cost | small set of docs is enough to start | reader must scan everything |
| Evolution | repeated work moves to contracts/tools | prose grows forever |
| Diagrams | structure is visually understandable | text-only or stale images |

## CI/CD Governance Rubric

Score each item `0-3`.

| Dimension | Good | Bad |
| --- | --- | --- |
| Commitment coverage | checks map to real product promises | generic checks only |
| Drift protection | docs/schema/code/CLI/dashboard stay aligned | each evolves separately |
| Risk levels | high-risk actions need permission/dry-run/audit | destructive actions are silent |
| Feedback quality | failure points to owner and repair path | red check with no diagnosis |
| Stability | PR checks are fast and reliable | slow or flaky gates |
| Contract tests | fixtures validate schema/CLI/API/dashboard | unit tests only |
| Diagram checks | critical diagrams parse/render | diagrams rot silently |
| Release discipline | version, migration, smoke tests are explicit | breaking changes publish silently |
| Governance loop | repeated incidents become gates or runbooks | repeated manual debugging |

## Evaluation Output

```text
Evaluation
  status: pass | warn | block
  docs_score: <0-30>
  cicd_score: <0-27>
  best_parts:
    - <what works and why>
  weak_parts:
    - <gap, evidence, consequence>
  responsibility_gaps:
    - <claim without clear owner surface>
  maturity_gaps:
    - <planned concept presented as stable>
  diagram_gaps:
    - <missing or stale diagram>
  missing_contracts:
    - <doc claim that should become schema/CLI/test>
  missing_checks:
    - <commitment CI/CD does not verify>
  next_changes:
    - <smallest durable change>
```
