# Multi-Agent Harness

Generic multi-agent harness for turning a project or business domain into an
agent-operable system.

The harness starts from a goal, models the domain scenario workflow, identifies
the missing infrastructure that would shorten agent work, designs the right
agent team and task graph, and then drives execution through messages,
evidence-backed reports, critic review, decisions, and follow-up requirements.

This repository is intentionally separate from any specific trading, research,
or product codebase. A project such as Earning Engine should be integrated
through an adapter: skills and tool descriptors teach agent members how to use
that project's CLI, Dashboard, artifacts, and permission rules.

## Product Boundary

```text
Multi-Agent Harness
  Goal / AgentTeam / AgentMember / AgentRuntime / AgentEvent / Task / Message
  Proposal / Evidence / Decision / ProviderSession
  Skill files / Tool descriptors / Agent Dashboard

Project Adapter
  CLI commands / Dashboard links / artifact readers / domain acceptance /
  permissions / evidence policy
```

The generic core must not import project-specific runtime code.

Future objects such as `Report`, `Claim`, `Blocker`, and `Permission` are not
first-version gateable contracts until they have schemas, implementation, and
checks.

## Quickstart

Install the `author-workflow` skill into your agent, start the harness service,
then ask your agent to author and run a workflow. Full walkthrough:
**[docs/getting-started.md](docs/getting-started.md)**.

```bash
# 1. install the skill (Claude Code + Codex)
scripts/install-skill.sh --agent both
#    or standalone:  curl -fsSL .../scripts/install-skill.sh | bash -s -- --agent both
#    or:             npx skills add cyl19970726/multi-agent-harness --skill author-workflow --agent codex
#    or (Claude):    /plugin marketplace add cyl19970726/multi-agent-harness && /plugin install author-workflow

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

## Repository Layout

| Path | Purpose |
| --- | --- |
| `docs/` | PRD, architecture, operations, schemas, and decisions. |
| `schemas/` | Stable JSON schemas shared by API, CLI, adapters, and Dashboard. |
| `crates/` | Rust backend crates. |
| `apps/agent-dashboard` | React/Vite Agent Dashboard control-plane app and static build output. |
| `skills/` | Shipped, installable skills (the `author-workflow` deliverable). |
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
- [Provider Integrations](docs/integration/README.md)
- [Goal Learning Loop](docs/goal-learning-loop.md)
- [Codex Integration](docs/integration/codex.md)
- [Operations](docs/operations.md)
- [Schemas](docs/schemas.md)
- [Decisions](docs/decisions/README.md)

## Skills

**Shipped (install into your own project — see the Quickstart):**

- [**Author workflow**](skills/author-workflow/SKILL.md): teach a shell-capable
  agent (Claude Code / Codex) to author a Starlark multi-agent workflow and run
  it with `harness workflow run-script`. Install with
  `scripts/install-skill.sh --agent both`.

**Internal (this repo's own agents use these via `.agents/skills/`):**

- [Bootstrap project workflow](.agents/skills/bootstrap-project-workflow/SKILL.md):
  create or audit a project's docs, CI/CD, diagrams, task workflow, and
  evidence-backed governance.
- [Generic agent harness](.agents/skills/generic-agent-harness/SKILL.md): operate and
  extend the generic harness objects and workflow.

## Initial Commands

```bash
npx pnpm@9.15.4 check
pnpm dashboard:build
cargo test
cargo run -p harness-cli -- --help
cargo run -p harness-cli -- dashboard snapshot
```
