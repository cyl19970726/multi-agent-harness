# Multi-Agent Harness

Generic multi-agent harness for agent members, message-first workflows, tool
adapters, evidence-backed reports, and an Agent Dashboard.

This repository is intentionally separate from any specific trading, research,
or product codebase. A project such as Earning Engine should be integrated
through an adapter: skills and tool descriptors teach agent members how to use
that project's CLI, Dashboard, artifacts, and permission rules.

## Product Boundary

```text
Multi-Agent Harness
  AgentRole / AgentMember / Message / Task / Report / Claim / Blocker /
  Decision / Permission / ProviderSession / Agent Dashboard

Project Adapter
  CLI commands / Dashboard links / artifact readers / domain acceptance /
  permissions / evidence policy
```

The generic core must not import project-specific runtime code.

## Repository Layout

| Path | Purpose |
| --- | --- |
| `docs/` | PRD, architecture, operations, schemas, and decisions. |
| `schemas/` | Stable JSON schemas shared by API, CLI, adapters, and Dashboard. |
| `crates/` | Rust backend crates. |
| `apps/agent-dashboard` | Dashboard product plan and future app. |
| `skills/` | Generic agent skills. |
| `examples/adapters/earning-engine` | First project adapter example. |

## Start Here

- [Product requirements](docs/prd.md)
- [Architecture](docs/architecture.md)
- [Operations](docs/operations.md)
- [Schemas](docs/schemas.md)
- [Decisions](docs/decisions.md)

## Initial Commands

```bash
npx pnpm@9.15.4 check
```
