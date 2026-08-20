# Documentation

Do not read this repository as one book. Start with one context pack and follow
links only when the current decision needs them. The placement, authority,
lifecycle and retirement rules are defined in
[Documentation Governance](current/documentation-governance.md).

## Notion and repository boundary

The [AgentFirm Home](https://app.notion.com/p/3b849a4fa3798115939cca2b0b9e6f2d)
is the current authority for the product mental model, Architecture Decisions,
Implementation Crosswalk, and the live Development System. The
[Development Playbook](https://app.notion.com/p/3b849a4fa37981a990a5cf0059dcfa4a)
defines the operating path from one Development Task through Dev work,
exact-version submission, immutable Review, merge, and closeout. The existing
Development Documents table contains readable Dev/Spec and Review documents;
there is no separate submission or transport ledger.

This repository remains authoritative for versioned implementation facts:
source code, schemas, executable contracts, tests, CI, release artifacts, and
the documentation snapshot required to understand the checked-out revision.
Repository docs should explain the shipped revision and link to Notion for
current intent; Notion should link back to issues, PRs, SHAs, checks, and repo
contracts for implementation proof. Do not copy live Work or Run state into
repository prose, and do not infer shipped behavior from Notion alone.

## Directory layout

```
docs/
├── mental/          # checked-out mental-model projections; current intent is in Notion
├── current/         # Live documentation that reflects the current system
│   ├── product/     # Product contracts (PRD, Agent Team Works)
│   ├── architecture/# Architecture maps, data model, schemas
│   ├── operations/  # Getting started, operations, governance engine
│   ├── integration/ # Provider integrations
│   └── dashboard/   # Dashboard frontend architecture
├── decisions/       # ADRs (historical decisions, never deleted)
```

The legacy `current/company-os/` directory was removed with the DOC-108
cutover; its reusable Work contract lives in
[current/product/agent-team-works.md](current/product/agent-team-works.md) and
the rest is recoverable from Git history.

There is no global directory precedence. Route each question to its owner:
accepted Notion Docs own product intent; code/schema/config and applicable
`AGENTS.md` own executable constraints at this checkout; registered repository
docs explain that revision; ADRs preserve accepted and superseded decisions.
Record mismatches in the Implementation Crosswalk and one Development Task.

## Start here

| Need | Smallest useful entry |
| --- | --- |
| Understand the product architecture | Start at [AgentFirm Home](https://app.notion.com/p/3b849a4fa3798115939cca2b0b9e6f2d), then use the checked-out [Agent Firm Mental Model](mental/agent-firm-mental-model.md) and implementation docs for this revision |
| Understand the execution foundation | [Product requirements](current/product/prd.md) and [Architecture map](current/architecture/architecture-map.md) |
| Change Agent Team orchestration | [Agent Team Works](current/product/agent-team-works.md), [Member continuation](current/architecture/member-continuation-model.md), [ADR 0044](decisions/0044-durable-team-supervision-and-typed-mail.md), [ADR 0050](decisions/0050-agent-team-work-board-and-message-boundary.md), and historical [ADR 0034](decisions/0034-host-plan-waves-and-mission-teams.md) |
| Change cross-machine Team collaboration | [Cross-machine collaboration architecture](current/architecture/cross-machine-team-collaboration.md), [operations](current/operations/cross-machine-collaboration.md), and [Remote Node Fabric](current/architecture/remote-node-fabric.md) |
| Implement or operate the repository | [Getting started](current/operations/getting-started.md), [Operations](current/operations/operations.md), [Schemas](current/architecture/schemas.md) |
| Change repository agent operating rules | Root [AGENTS.md](../AGENTS.md) and [Agent operating rules detail](current/product/agent-operating-rules.md) |
| Integrate a provider | [Integration index](current/integration/README.md) |
| Interpret an old decision | The relevant ADR, verified native export, or Git history. |

## Documentation modules

| Module | Entry points |
| --- | --- |
| Mental models | [`mental/`](mental/) — checked-out product-model projection; accepted Notion Docs own current intent |
| Product | [PRD](current/product/prd.md), [Design basis](current/architecture/design-basis.md) |
| Architecture | [Architecture map](current/architecture/architecture-map.md), [Concept model](current/architecture/concept-model.md), [Data model](current/architecture/data-model.md), [ADRs](decisions/README.md) |
| Execution | [Dashboard](current/operations/dashboard.md), [Workflow runtime](current/operations/workflow-runtime.md), [Agent runtime](current/architecture/agent-runtime.md), [Agent Team Works](current/product/agent-team-works.md), [Integration](current/integration/README.md) |
| Operations | [Getting started](current/operations/getting-started.md), [Operations](current/operations/operations.md), [Multi-project](current/operations/multi-project.md), [Governance engine](current/operations/governance-engine.md) |
| Historical evidence | Verified native exports and Git history; never default context |

## Governance

When adding, moving, or retiring documentation:

1. **Classify authority by question.** Put current product meaning in accepted
   Notion Docs. Keep `docs/mental/` and other repository docs aligned with the
   checked-out applied revision; record target/applied drift instead of
   silently choosing one prose copy.
2. **Live docs live in `current/`.** New product/architecture/operations docs
   go under the matching `current/` subdirectory, not the root.
3. **Retired docs are deleted.** Obsolete docs are removed (git history is the
   archive). ADRs in `decisions/` are kept permanently as historical records.
4. **Registry must match.** Update `docs/registry.json` when adding, moving, or
   removing any doc. `firm governance check` validates this.
5. **Registry is repository-only.** `docs/registry.json` classifies repository
   documents and their machine consumers; it does not own product intent,
   development Skill lifecycle, Crosswalk state, or Task status.
6. **Skills sync.** Product skills live in `skills/` and mirror to
   `plugins/star-harness/skills/` via `sync-star-harness-plugin-skills.mjs`.
   The repository-only `.agents/skills/agentfirm-development-loop` follows the
   Development Playbook through reviewed repository changes and is not governed
   by the docs registry.
7. **Cross-layer check.** `node scripts/check-cross-layer-consistency.mjs`
   verifies skills, code prompts, and plugin manifests stay consistent.

Project-specific tool usage belongs in `examples/adapters/**` or in the
integrating project repository, not in the generic core docs.

## Skills

| Skill | Use |
| --- | --- |
| [agentfirm-development-loop](../.agents/skills/agentfirm-development-loop/SKILL.md) | The repository's only default development loop: Brain -> one Task -> Dev -> exact-revision Review; rejected work returns to the same Task. |
| [collaborate-as-agent-team-member](../skills/collaborate-as-agent-team-member/SKILL.md) | Provider-neutral member guidance for Work claim/start/block/submit, Work-linked conversation, native subagents, evidence, and Host acceptance. |
| [star-workflow](../skills/star-workflow/SKILL.md) | Optional Dynamic Workflow authoring capability; not the authority for Work planning, and never the retired Mission or Mission Log planning authority. |
| [bootstrap-project-workflow](../skills/bootstrap-project-workflow/SKILL.md) | Current doc-sync compatibility methodology. It is no longer a mandatory Lead skill or default install. |
| [design-notion-information-architecture](../.agents/skills/design-notion-information-architecture/SKILL.md) | Audit, redesign, migrate, and review governed Notion systems with explicit authority, reader journeys, semantic relations, cutover, and rollback. |

## Split rule

Keep docs merged until a file is stable above roughly 500 lines, has a clearly
different reader, or is consumed by CI/tooling.

Canonical repository documentation belongs under `docs/`. Extend the owning
contract before creating a new file. Split only when owner, reader, lifecycle or
machine consumer materially differs. App and package directories must not become
parallel product-documentation systems.
