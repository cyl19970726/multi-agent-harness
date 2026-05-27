# Docs

| Topic | Doc |
| --- | --- |
| Agent operating rules | [../AGENTS.md](../AGENTS.md) |
| Product requirements | [prd.md](prd.md) |
| MVP: self-hosting and LetMeTry strategy pilot | [mvp.md](mvp.md) |
| Design basis, layers, and module core ideas | [design-basis.md](design-basis.md) |
| Concept model and object relationship invariants | [concept-model.md](concept-model.md) |
| Architecture, modules, task flow, and package plan | [architecture.md](architecture.md) |
| Core module PRD and architecture narrative | [core-modules.md](core-modules.md) |
| Data model, source-of-truth rules, and projections | [data-model.md](data-model.md) |
| Provider-neutral Agent Runtime Object Model | [agent-runtime.md](agent-runtime.md) |
| Agent control plane, lifecycle, queues, and Dashboard operations | [agent-control-plane.md](agent-control-plane.md) |
| Agent Dashboard information architecture | [dashboard.md](dashboard.md) |
| Git, PR, worktree, review, and decision workflow | [workflow-git-pr.md](workflow-git-pr.md) |
| Goal learning loop and reusable cases | [goal-learning-loop.md](goal-learning-loop.md) |
| Provider integration rules | [integration/README.md](integration/README.md) |
| Codex provider integration | [integration/codex.md](integration/codex.md) |
| Codex source-audit runtime notes | [codex-agent-runtime.md](codex-agent-runtime.md) |
| Operations | [operations.md](operations.md) |
| Schemas and minimal object contracts | [schemas.md](schemas.md) |
| Design decisions | [decisions.md](decisions.md) |

Project-specific tool usage belongs in `examples/adapters/**` or in the
integrating project repository, not in the generic core docs.

## Skills

| Skill | Use |
| --- | --- |
| [bootstrap-project-workflow](../.agents/skills/bootstrap-project-workflow/SKILL.md) | Bootstrap or audit docs, CI/CD, diagrams, task workflow, and project governance. |
| [generic-agent-harness](../.agents/skills/generic-agent-harness/SKILL.md) | Operate or extend the generic harness objects and message-first workflow. |

## Split Rule

Keep docs merged until a file is stable above roughly 500 lines, has a clearly
different reader, or is consumed by CI/tooling.
