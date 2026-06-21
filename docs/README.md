# Docs

| Topic | Doc |
| --- | --- |
| Vision: the closed self-advancing loop + roadmap | [VISION.md](VISION.md) |
| Getting started: author + run a workflow | [getting-started.md](getting-started.md) |
| Agent operating rules | [../AGENTS.md](../AGENTS.md) |
| Product requirements | [prd.md](prd.md) |
| MVP: self-hosting and LetMeTry strategy pilot | [mvp.md](mvp.md) |
| Design basis, layers, and module core ideas | [design-basis.md](design-basis.md) |
| Concept model and object relationship invariants | [concept-model.md](concept-model.md) |
| Architecture, modules, task flow, and package plan | [architecture.md](architecture.md) |
| Core module PRD and architecture narrative | [core-modules.md](core-modules.md) |
| Data model, source-of-truth rules, and projections | [data-model.md](data-model.md) |
| Goal → phase → task → workflow loop (run-phases) | [goal-phase-loop.md](goal-phase-loop.md) |
| Starlark workflow runtime (run-script) | [workflow-runtime.md](workflow-runtime.md) |
| Doc/skill governance engine (harness governance) | [governance-engine.md](governance-engine.md) |
| Provider-neutral Agent Runtime Object Model | [agent-runtime.md](agent-runtime.md) |
| Agent control plane, lifecycle, queues, and Workbench operations | [agent-control-plane.md](agent-control-plane.md) |
| Provider/platform adaptation (launch spec, capabilities) | [agent-integration-model.md](agent-integration-model.md) |
| Per-provider runtime health observability | [member-runtime-observability.md](member-runtime-observability.md) |
| Resident Claude runtime (opt-in) | [resident-claude.md](resident-claude.md) |
| Multi-project central store + control plane | [multi-project.md](multi-project.md) |
| Agent Workbench information architecture | [dashboard.md](dashboard.md) |
| Agent Workbench docs placement map | [dashboard/README.md](dashboard/README.md) |
| Agent Workbench frontend design principles | [dashboard/design-principles.md](dashboard/design-principles.md) |
| Agent Workbench layout history | [dashboard/layout-history.md](dashboard/layout-history.md) |
| Agent Workbench frontend design index | [dashboard/frontend-design.md](dashboard/frontend-design.md) |
| Agent Workbench page specs and layout contracts | [dashboard/pages/README.md](dashboard/pages/README.md) |
| Agent Workbench frontend architecture | [dashboard/frontend-architecture.md](dashboard/frontend-architecture.md) |
| Agent Workbench read model | [dashboard/read-model.md](dashboard/read-model.md) |
| Agent Workbench acceptance | [dashboard/acceptance.md](dashboard/acceptance.md) |
| Agent Workbench runbook | [dashboard/runbook.md](dashboard/runbook.md) |
| Git, PR, worktree, review, and decision workflow | [workflow-git-pr.md](workflow-git-pr.md) |
| Goal learning loop and reusable cases | [goal-learning-loop.md](goal-learning-loop.md) |
| External-system architecture research | [research/README.md](research/README.md) |
| Provider integration rules | [integration/README.md](integration/README.md) |
| Codex provider integration | [integration/codex.md](integration/codex.md) |
| Codex message delivery | [integration/codex-message-delivery.md](integration/codex-message-delivery.md) |
| Codex source-audit findings | [integration/codex-source-audit.md](integration/codex-source-audit.md) |
| Claude Code provider integration | [integration/claude.md](integration/claude.md) |
| Kimi (Moonshot) provider integration | [integration/kimi.md](integration/kimi.md) |
| Operations | [operations.md](operations.md) |
| Schemas and minimal object contracts | [schemas.md](schemas.md) |
| Design decisions | [decisions/README.md](decisions/README.md) |

Project-specific tool usage belongs in `examples/adapters/**` or in the
integrating project repository, not in the generic core docs.

## Skills

| Skill | Use |
| --- | --- |
| [bootstrap-project-workflow](../skills/bootstrap-project-workflow/SKILL.md) | Make a project agent-operable: vision-driven docs whose tree projects the key-mechanism/key-module decomposition, CI/CD, diagrams, task workflow, and project governance (shipped built-in skill; the doc-governance skill the doc-sync built-in phase runs, see [goal-phase-loop.md](goal-phase-loop.md)). |
| [generic-agent-harness](../.agents/skills/generic-agent-harness/SKILL.md) | Operate or extend the generic harness objects and message-first workflow. |
| [multi-agent-system-design](../.agents/skills/multi-agent-system-design/SKILL.md) | Design or audit durable multi-agent mailboxes, delivery, runtime lifecycle, permission messages, and dashboard proof. |

## Split Rule

Keep docs merged until a file is stable above roughly 500 lines, has a clearly
different reader, or is consumed by CI/tooling.

Canonical repository documentation belongs under `docs/`. Module internals may
use `docs/<module>/` when the parent module is too large or has different
readers. App and package directories should not become parallel documentation
systems; they should contain implementation files and generated artifacts.
