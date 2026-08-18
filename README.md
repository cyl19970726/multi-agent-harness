# Star Harness — AgentFirm execution foundation

Star Harness is the provider-neutral execution foundation for an AI-native
company: durable flat AgentTeams of standing AgentMembers, accountable Work,
identity-first Messages, and provider-native session runtimes, across machines
through the remote fabric.

The current product model and operating control plane live in Notion
(AgentFirm Home). This repository owns the implemented execution truth: code,
schemas, stores, CLI/API/MCP surfaces, tests, and CI.

The legacy Company OS layer — the Company Store registry, built-in Docs,
Organization, Finance, generic Approval, and the legacy Mission, Wave, and
Mission Log — is retired by DOC-108. Its writers are closed on every surface; its historical
rows remain readable as legacy provenance and are export/verify-only through
`harness legacy-company-os export|verify`. None of it is current authority.

```text
AgentTeam (durable, flat, one immutable node_id placement)
  -> TeamMembership generations bind AgentMembers
  -> AgentTeamRun / MemberRun attempt execution
  -> Work (durable responsibility, WorkOperation/WorkEvent/WorkDelivery history)
  -> identity-first Message -> MessageSubscription -> CanonicalMessageDelivery
  -> AgentSession -> provider-native session (sole execution transcript truth)
  -> NodeDaemon -> durable RuntimeCommand -> provider effect
```

## Current implementation status

The execution foundation is substantially implemented and is the repository's
active development surface:

- one durable AgentTeam per flat team, placed immutably on one machine-scoped
  NodeDaemon, with TeamMembership as roster authority;
- TeamRun/MemberRun projections over provider-native sessions; one execution
  driver per member (`host_driven` or `provider_driven`); explicit Host
  create/message/interrupt/close/reopen/retire member control;
- durable Work with WorkOperation history, versioned delivery, submission,
  review, and Host acceptance;
- peer-Team and member messaging over the canonical Message/subscription/
  delivery fabric, locally and across machines;
- Execution Spaces own coordination; Project Bindings own provider cwd,
  instructions, Skills, plugins, and MCP configuration;
- Dynamic Workflows, provider admission gates, plugins, MCP, artifacts, and
  events.

Legacy Mission/Wave/Mission Log rows and the retired Company OS ledgers are
historical evidence only, proven by the export/verify round-trip.

## Repository development

Development intent and implementation Specs are canonical in Notion. Each
delivery batch is claimed by one Primary Codex Session and lands through one
clean worktree, branch, umbrella Issue, and final PR:

```text
Notion Spec -> GitHub Issue -> Codex claim -> clean worktree -> implementation
  -> PR -> final-SHA self-review + CI -> Host Gate when required -> merge
  -> Notion closeout
```

Harness Members are temporarily not used for repository repair until the
dogfood admission standard passes. Temporary Sub-Agents are internal execution
resources, not separate owners. This developer workflow does not remove or
weaken product TeamWork acceptance and Gate semantics. See
[the Git/PR workflow](docs/current/operations/workflow-git-pr.md).

## Quickstart: current execution foundation

```bash
scripts/install-skill.sh --agent both --skill star-workflow
cargo build -p firm-cli
./target/debug/firm serve --addr 127.0.0.1:8787
pnpm install
pnpm dashboard:dev
```

Run a Dynamic Workflow:

```bash
./target/debug/firm workflow run-script prog.star \
  --timeout-ms 300000 --max-budget-usd 2.00
```

One service can manage many projects. See [multi-project](docs/current/operations/multi-project.md)
and [getting started](docs/current/operations/getting-started.md).

## Start here

- [Product requirements](docs/current/product/prd.md)
- [Agent Team Work](docs/current/product/agent-team-works.md)
- [Architecture map](docs/current/architecture/architecture-map.md)
- [Concept model](docs/current/architecture/concept-model.md)
- [Member continuation model](docs/current/architecture/member-continuation-model.md)
- [Provider integrations](docs/current/integration/README.md)
- [Operations](docs/current/operations/operations.md)
- [Architecture decisions](docs/decisions/README.md)
- [Durable Team supervision and typed mail](docs/decisions/0044-durable-team-supervision-and-typed-mail.md)

## Repository layout

| Path | Purpose |
| --- | --- |
| `docs/current/product/` | Product requirements and Work/Team product contracts. |
| `docs/current/architecture/` | Implemented boundaries and durable design contracts. |
| `docs/current/dashboard/` | Current layout contracts, frontend design, page contracts, and runbook. |
| `docs/decisions/` | ADR history; superseded decisions are marked, never deleted. |
| `schemas/` | Stable wire schemas for implemented objects, plus legacy read-compatibility schemas. |
| `crates/` | Rust store, core, CLI, execution, and provider infrastructure. |
| `apps/agent-dashboard/` | React/Vite operator dashboard for harness state. |
| `skills/` | Optional capabilities, including Dynamic Workflow authoring and Agent Team member operation. |
| `archive/` | Retired skills and packages kept as historical references only. |
| `examples/adapters/` | Domain adapters; business-specific logic stays outside the generic core. |

## Core boundary

The generic core defines team, membership, work, message, session, runtime,
and execution contracts. Domain-specific record types belong to adapters,
templates, and typed schemas — not hard-coded provider or project logic.
