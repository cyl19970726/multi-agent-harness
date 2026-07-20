# Docs

| Topic | Doc |
| --- | --- |
| **Canonical Company OS documentation** | [company-os/README.md](company-os/README.md) |
| Company OS vision | [company-os/vision.md](company-os/vision.md) |
| Company OS concept model | [company-os/concept-model.md](company-os/concept-model.md) |
| Docs and module architecture | [company-os/document-system.md](company-os/document-system.md) / [company-os/module-design.md](company-os/module-design.md) |
| Organization, humans, and Standing Agents | [company-os/organization-and-actors.md](company-os/organization-and-actors.md) |
| WorkItems and approvals | [company-os/work-items-and-approvals.md](company-os/work-items-and-approvals.md) |
| Finance relations and governance | [company-os/financial-relations.md](company-os/financial-relations.md) / [company-os/governance.md](company-os/governance.md) |
| Company OS frontend IA and page matrix | [company-os/frontend-information-architecture.md](company-os/frontend-information-architecture.md) / [company-os/core-page-matrix.md](company-os/core-page-matrix.md) |
| Company OS visual contract | [design/company-os-v1/README.md](design/company-os-v1/README.md) |
| Company OS implementation waves | [company-os/implementation-waves.md](company-os/implementation-waves.md) |
| Company OS shared fixture contract | [design/company-os-v1/fixture-contract.md](design/company-os-v1/fixture-contract.md) / [fixture JSON](design/company-os-v1/fixtures/company-os-trademark-v1.json) |
| Company OS implementation acceptance | [design/company-os-v1/implementation-acceptance.md](design/company-os-v1/implementation-acceptance.md) |
| Getting started: author + run a workflow | [getting-started.md](getting-started.md) |
| Agent operating rules | [../AGENTS.md](../AGENTS.md) |
| Product requirements | [prd.md](prd.md) |
| MVP: self-hosting and LetMeTry strategy pilot | [mvp.md](mvp.md) |
| Design basis, layers, and module core ideas | [design-basis.md](design-basis.md) |
| Concept model and object relationship invariants | [concept-model.md](concept-model.md) |
| Architecture, modules, task flow, and package plan | [architecture.md](architecture.md) |
| Data model, source-of-truth rules, and projections | [data-model.md](data-model.md) |
| Starlark workflow runtime (run-script) | [workflow-runtime.md](workflow-runtime.md) |
| Doc/skill governance engine (harness governance) | [governance-engine.md](governance-engine.md) |
| Provider-neutral Agent Runtime Object Model | [agent-runtime.md](agent-runtime.md) |
| Provider/platform adaptation (launch spec, capabilities) | [agent-integration-model.md](agent-integration-model.md) |
| Per-provider runtime health observability | [member-runtime-observability.md](member-runtime-observability.md) |
| Resident Claude runtime (opt-in) | [resident-claude.md](resident-claude.md) |
| Multi-project central store + control plane | [multi-project.md](multi-project.md) |
| Agent Workbench information architecture | [dashboard.md](dashboard.md) |
| Agent Workbench docs placement map | [dashboard/README.md](dashboard/README.md) |
| Agent Workbench frontend design index | [dashboard/frontend-design.md](dashboard/frontend-design.md) |
| Agent Workbench page specs and layout contracts | [dashboard/pages/README.md](dashboard/pages/README.md) |
| Agent Workbench frontend architecture | [dashboard/frontend-architecture.md](dashboard/frontend-architecture.md) |
| Agent Workbench runbook | [dashboard/runbook.md](dashboard/runbook.md) |
| Git, PR, worktree, review, and decision workflow | [workflow-git-pr.md](workflow-git-pr.md) |
| External-system architecture research | [research/README.md](research/README.md) |
| Provider integration rules | [integration/README.md](integration/README.md) |
| Host Agent MCP control contract | [integration/host-agent-mcp.md](integration/host-agent-mcp.md) |
| Codex provider integration | [integration/codex.md](integration/codex.md) |
| Codex message delivery | [integration/codex-message-delivery.md](integration/codex-message-delivery.md) |
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
| [star-workflow](../skills/star-workflow/SKILL.md) | Optional Dynamic Workflow authoring capability; not a Mission/Wave planning authority. |
| [bootstrap-project-workflow](../skills/bootstrap-project-workflow/SKILL.md) | Current doc-sync compatibility methodology. It is no longer a mandatory Lead skill or default install. |
| [multi-agent-system-design](../.agents/skills/multi-agent-system-design/SKILL.md) | Reusable mailbox, runtime lifecycle, permission, recovery, and dashboard-proof design guidance. |

## Split Rule

Keep docs merged until a file is stable above roughly 500 lines, has a clearly
different reader, or is consumed by CI/tooling.

Canonical repository documentation belongs under `docs/`. Module internals may
use `docs/<module>/` when the parent module is too large or has different
readers. App and package directories should not become parallel documentation
systems; they should contain implementation files and generated artifacts.
