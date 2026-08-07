# Documentation

Do not read this repository as one book. Start with one context pack and follow
links only when the current decision needs them. The placement, authority,
lifecycle and archive rules are defined in
[Documentation Governance](current/documentation-governance.md).

## Directory layout

```
docs/
├── mental/          # AUTHORITATIVE mental models (single source of truth)
├── current/         # Live documentation that reflects the current system
│   ├── product/     # Product contracts (PRD, Agent Team Works)
│   ├── architecture/# Architecture maps, data model, schemas
│   ├── operations/  # Getting started, operations, governance engine
│   ├── integration/ # Provider integrations
│   ├── dashboard/   # Dashboard frontend architecture
│   └── company-os/  # Company OS product contracts
├── decisions/       # ADRs (historical decisions, never deleted)
└── archive/         # Retired docs (design workstreams, old research)
    ├── design/      # Historical design versions (v1-v6)
    ├── research/    # Old exploration records
    ├── company-os/  # Retired Company OS designs
    └── product/     # Retired product docs
```

**Authority order**: `mental/` > `current/` > `decisions/` > `archive/`.
When `mental/` contradicts any other doc, `mental/` wins.

## Start here

| Need | Smallest useful entry |
| --- | --- |
| Understand the product architecture | [Agent Firm Mental Model](mental/agent-firm-mental-model.md) — canonical |
| Understand the AI Company OS | [Company OS product system map](current/company-os/product-system-map.md) |
| Change Company OS product behavior | [Company OS contracts](current/company-os/README.md) |
| Change Agent Team orchestration | [Host-plan contract](current/product/mission-wave-host-plan.md), [Agent Team Works](current/product/agent-team-works.md), [Member continuation](current/architecture/member-continuation-model.md), [ADR 0034](decisions/0034-host-plan-waves-and-mission-teams.md), [ADR 0044](decisions/0044-durable-team-supervision-and-typed-mail.md), [ADR 0050](decisions/0050-agent-team-work-board-and-message-boundary.md), and [Architecture map](current/architecture/architecture-map.md) |
| Implement or operate the repository | [Getting started](current/operations/getting-started.md), [Operations](current/operations/operations.md), [Schemas](current/architecture/schemas.md) |
| Change repository agent operating rules | Root [AGENTS.md](../AGENTS.md) and [Agent operating rules detail](current/product/agent-operating-rules.md) |
| Integrate a provider | [Integration index](current/integration/README.md) |
| Interpret an old decision | The relevant ADR, verified native export, or Git history. |

## Documentation modules

| Module | Entry points |
| --- | --- |
| Mental models | [`mental/`](mental/) — authoritative, single source of truth |
| Product | [PRD](current/product/prd.md), [Company OS](current/company-os/README.md), [Design basis](current/architecture/design-basis.md) |
| Architecture | [Architecture map](current/architecture/architecture-map.md), [Concept model](current/architecture/concept-model.md), [Data model](current/architecture/data-model.md), [ADRs](decisions/README.md) |
| Execution | [Dashboard](current/operations/dashboard.md), [Workflow runtime](current/operations/workflow-runtime.md), [Agent runtime](current/architecture/agent-runtime.md), [Agent Team Works](current/product/agent-team-works.md), [Integration](current/integration/README.md) |
| Operations | [Getting started](current/operations/getting-started.md), [Operations](current/operations/operations.md), [Multi-project](current/operations/multi-project.md), [Governance engine](current/operations/governance-engine.md) |
| Historical design | [`archive/design/`](archive/design/) — retired workstreams, read-only |
| Historical research | [`archive/research/`](archive/research/) — retired exploration, read-only |
| Historical evidence | Verified native exports and Git history; never default context |

## Governance

When adding, moving, or retiring documentation:

1. **Mental models are canonical.** Put durable product architecture in
   `docs/mental/`. Other docs that contradict it must be corrected or retired.
2. **Live docs live in `current/`.** New product/architecture/operations docs
   go under the matching `current/` subdirectory, not the root.
3. **Retired docs go to `archive/`.** Move (not delete) obsolete docs to the
   matching archive subdirectory. Keep ADRs in `decisions/` permanently.
4. **Registry must match.** Update `docs/registry.json` when adding, moving, or
   removing any doc. `harness governance check` validates this.
5. **Skills sync.** Skills live in `skills/` and mirror to
   `plugins/star-harness/skills/` via `sync-star-harness-plugin-skills.mjs`.
6. **Cross-layer check.** `node scripts/check-cross-layer-consistency.mjs`
   verifies skills, code prompts, and plugin manifests stay consistent.

Project-specific tool usage belongs in `examples/adapters/**` or in the
integrating project repository, not in the generic core docs.

## Skills

| Skill | Use |
| --- | --- |
| [orchestrate-mission-waves](../skills/orchestrate-mission-waves/SKILL.md) | Thin Host guidance for durable Mission context, versioned Wave memos, shared Works, Mission-linked long-lived Teams, and advance/re-plan. CLI remains the authority. |
| [collaborate-as-agent-team-member](../skills/collaborate-as-agent-team-member/SKILL.md) | Provider-neutral member guidance for Work claim/start/block/submit, Work-linked conversation, native subagents, evidence, and Host acceptance. |
| [star-workflow](../skills/star-workflow/SKILL.md) | Optional Dynamic Workflow authoring capability; not a Mission/Wave planning authority. |
| [bootstrap-project-workflow](../skills/bootstrap-project-workflow/SKILL.md) | Current doc-sync compatibility methodology. It is no longer a mandatory Lead skill or default install. |
| [multi-agent-system-design](../.agents/skills/multi-agent-system-design/SKILL.md) | Reusable mailbox, runtime lifecycle, permission, recovery, and dashboard-proof design guidance. |

## Split rule

Keep docs merged until a file is stable above roughly 500 lines, has a clearly
different reader, or is consumed by CI/tooling.

Canonical repository documentation belongs under `docs/`. Extend the owning
contract before creating a new file. Split only when owner, reader, lifecycle or
machine consumer materially differs. App and package directories must not become
parallel product-documentation systems.
