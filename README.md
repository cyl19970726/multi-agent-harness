# Star Harness

Provider-neutral execution and collaboration tools for resident Host Agents.

The product direction is `Mission -> ordered Wave -> executor`. Each Wave uses
an Agent Team, a Dynamic Workflow, or direct Host execution. Wave stays
lightweight: executor-specific planning belongs to the executor, and Agent Team
ownership is expressed through assignment-message correlation rather than a
mandatory Task Graph.

This repository is intentionally separate from any specific trading, research,
or product codebase. A project such as Earning Engine should be integrated
through an adapter: skills and tool descriptors teach agent members how to use
that project's CLI, Dashboard, artifacts, and permission rules.

## Product Boundary

```text
Star Harness
  Mission / Wave / executor selection
  Agent Team / Dynamic Workflow / Host execution
  Provider sessions / messages / artifacts / events
  Plugin / MCP / CLI / Agent Dashboard

Project Adapter
  CLI commands / Dashboard links / artifact readers / domain acceptance /
  permissions / evidence policy
```

The generic core must not import project-specific runtime code.

Current Goal, GoalPhase, Task, Evidence, and Decision objects remain
self-hosting compatibility surfaces while the non-destructive Mission/Wave
migration proceeds.

## Quickstart

Install the optional `star-workflow` authoring capability, start the harness
service, then ask your agent to author and run a Dynamic Workflow. Full walkthrough:
**[docs/getting-started.md](docs/getting-started.md)**.

```bash
# 1. install the Dynamic Workflow authoring skill (Claude Code + Codex)
scripts/install-skill.sh --agent both
#    explicit:         scripts/install-skill.sh --agent both --skill star-workflow
#    or standalone:   curl -fsSL .../scripts/install-skill.sh | bash -s -- --agent both
#    or:              npx skills add cyl19970726/multi-agent-harness --skill star-workflow --agent codex
#    or (Claude):     /plugin marketplace add cyl19970726/multi-agent-harness && /plugin install star-workflow

# 2. start the service
cargo build -p harness-cli
./target/debug/harness serve --addr 127.0.0.1:8787   # API + store
pnpm install && pnpm dashboard:dev                    # dashboard UI (watch runs live)

# 3. run a workflow your agent authored
./target/debug/harness workflow run-script prog.star \
    [--timeout-ms 300000] [--max-budget-usd 2.00] [--resume <prior_run_id>]
```

Verify the whole install → service → run journey at any time:

```bash
pnpm acceptance:skill-install            # local checks (no network)
pnpm acceptance:skill-install --remote   # + the anonymous curl|bash install path
```

One `serve` / dashboard can manage many projects (each with its own
goals/tasks/runs in a centralized store under `~/.harness/projects/<id>/`) plus a
reserved GLOBAL `~/` project. Register with `harness init`, switch with
`harness project switch <id|path>`, migrate a legacy repo-local `.harness` with
`harness project migrate`. See **[docs/multi-project.md](docs/multi-project.md)**.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `docs/` | PRD, architecture, operations, schemas, and decisions. |
| `schemas/` | Stable JSON schemas shared by API, CLI, adapters, and Dashboard. |
| `crates/` | Rust backend crates. |
| `apps/agent-dashboard` | React/Vite Agent Dashboard control-plane app and static build output. |
| `skills/` | Optional shipped capabilities; currently Dynamic Workflow authoring plus compatibility governance material. |
| `.agents/skills/` | This repo's internal runtime skills (auto-discovered by Codex / harness-spawned workers). |
| `examples/adapters/earning-engine` | First project adapter example. |

## Start Here

- [Agent operating rules](AGENTS.md)
- [Product requirements](docs/prd.md)
- [MVP](docs/mvp.md)
- [Design basis](docs/design-basis.md)
- [Concept Model](docs/concept-model.md)
- [Architecture](docs/architecture.md)
- [Core Modules](docs/core-modules.md)
- [Data Model](docs/data-model.md)
- [Agent Runtime](docs/agent-runtime.md)
- [Agent Control Plane](docs/agent-control-plane.md)
- [Agent Dashboard](docs/dashboard.md)
- [Agent Dashboard Frontend Architecture](docs/dashboard/frontend-architecture.md)
- [Agent Dashboard Runbook](docs/dashboard/runbook.md)
- [Git / PR Workflow](docs/workflow-git-pr.md)
- [Multi-Project Harness](docs/multi-project.md)
- [Provider Integrations](docs/integration/README.md)
- [Goal Learning Loop](docs/goal-learning-loop.md)
- [Codex Integration](docs/integration/codex.md)
- [Operations](docs/operations.md)
- [Schemas](docs/schemas.md)
- [Decisions](docs/decisions/README.md)

## Skills

**Shipped:**

- [**Star workflow**](skills/star-workflow/SKILL.md): teach a shell-capable
  agent (Claude Code / Codex) to author a Starlark multi-agent workflow and run
  it with `harness workflow run-script`.

Install it with `scripts/install-skill.sh --agent both`.

**Compatibility and specialist capabilities (opt in only when explicitly
needed):**

- [Bootstrap project workflow](skills/bootstrap-project-workflow/SKILL.md):
  legacy doc-sync and project-governance guidance. It is retained for current
  compatibility work, but is not installed by default and is not Lead policy.
- [Multi-agent system design](.agents/skills/multi-agent-system-design/SKILL.md):
  reusable runtime, mailbox, permission, recovery, and observability design
  guidance; it is not a product orchestration authority.

## Initial Commands

```bash
npx pnpm@9.15.4 check
pnpm dashboard:build
cargo test
cargo run -p harness-cli -- --help
cargo run -p harness-cli -- dashboard snapshot
```
