# Star Harness — AI Company OS

Star Harness is building an operating system for an AI-native company.

The product has two primary systems:

1. **Docs** — the company's memory, business structure, data relationships,
   decisions, and default place to initiate work.
2. **Organization** — humans, standing Agents, external collaborators, and
   services arranged into accountable, permissioned operating units.

Documents initiate or reference authoritative `Work` and explicit `Approval`
records. Work lifecycle and ownership stay in the selected Execution Space;
Company OS only aggregates it for company-wide reading and routing. Results,
evidence, metrics, and financial effects return to related business records.

```text
Document / Business Module
  -> Work / Approval
     -> Human or Standing Agent responsibility
     -> Mission/Wave | Agent Team | Dynamic Workflow | direct work
  -> Result / Evidence / Metric / FinancialRecord
  -> Document and organization evolution
```

Mission/Wave, Agent Team, Dynamic Workflow, Host execution, provider runtimes,
plugins, and MCP are the execution foundation. They remain important, but they
are not the top-level product information architecture.

## Current implementation status

The provider-neutral execution foundation is substantially implemented.
Mission is durable intent, ordered Waves preserve the Host's evolving plan and
judgment, and independent Agent Teams can remain active across multiple Waves.
Every live TeamRun has one durable Supervisor generation that owns provider
connections, delivery claims, and real controls. Members keep their
provider-native sessions while idle; typed Host, Member, Agent, and Operator
mail remains durable across service restarts, and only an explicit Host Close
ends a member runtime.
The Company OS product contracts, document system, mixed human/Agent
organization, Work/Approval model, and new frontend information
architecture are the current product-development focus.

The superseded coordination stack is being frozen, exported, verified, and
removed under ADR 0028. It is not part of the active product model.

## Product surfaces

Primary:

- **Home** — a document-composed company overview and decision queue.
- **Docs** — Notion-like pages, databases, typed records, relations, views, and
  module templates.
- **Organization** — structure containing people, standing Agents, and
  external participants.

Shared operating views:

- **Work** — a read-only company aggregate over authoritative TeamWork, with
  explicit lifecycle, condition, ownership, reports, evidence, and decisions.
- **Approvals** — human and policy gates for legal, finance, permissions, and
  organization changes.
- **Finance** — typed budgets, commitments, invoices, payments, and refunds
  linked to their originating business records.

Execution tools:

- Missions and ordered Host-plan Waves;
- independent Agent Teams, Mission-scoped TeamRuns, and MemberRuns;
- durable Team Supervisors, typed mailboxes, delivery receipts, and native
  session lifecycle;
- Dynamic Workflows;
- provider sessions, plugins, MCP, artifacts, and events.

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

- [Company OS documentation](docs/current/company-os/README.md)
- [Document system](docs/current/company-os/document-system.md)
- [Organization and actors](docs/current/company-os/organization-and-actors.md)
- [Work records and approvals](docs/current/company-os/work-items-and-approvals.md)
- [Module design](docs/current/company-os/module-design.md)
- [Governance](docs/current/company-os/governance.md)
- [Execution foundation](docs/current/company-os/execution-foundation.md)
- [Mission/Wave Host-plan product contract](docs/current/product/mission-wave-host-plan.md)
- [Agent Team control foundation](docs/current/product/agent-team-foundation-closure-plan.md)
- [Durable Team supervision and typed mail](docs/decisions/0044-durable-team-supervision-and-typed-mail.md)
- [Host-plan Wave and Mission Team decision](docs/decisions/0034-host-plan-waves-and-mission-teams.md)
- [Product requirements](docs/current/product/prd.md)
- [Architecture map](docs/current/architecture/architecture-map.md)
- [Provider integrations](docs/current/integration/README.md)
- [Operations](docs/current/operations/operations.md)
- [Architecture decisions](docs/decisions/README.md)

## Repository layout

| Path | Purpose |
| --- | --- |
| `docs/current/company-os/` | Canonical Company OS product and architecture contracts. |
| `docs/current/dashboard/` | Current layout contracts, frontend design, page contracts, and runbook. |
| `schemas/` | Stable wire schemas for implemented objects. |
| `crates/` | Rust store, core, CLI, execution, and provider infrastructure. |
| `apps/agent-dashboard/` | React/Vite Company OS and execution workbench frontend. |
| `skills/` | Optional capabilities, including Dynamic Workflow authoring and thin Mission/Wave Host orchestration. |
| `examples/adapters/` | Domain adapters; business-specific logic stays outside the generic core. |

## Core boundary

The generic core may define document, organization, work, relation, finance,
governance, and execution contracts. Domain-specific record types such as a
trademark jurisdiction or a content-platform metric belong to Company Modules,
templates, adapters, and typed schemas—not hard-coded provider or project logic.
