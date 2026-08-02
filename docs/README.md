# Documentation

Do not read this repository as one book. Start with one context pack and follow
links only when the current decision needs them. The placement, authority,
lifecycle and archive rules are defined in
[Documentation Governance](documentation-governance.md).

## Start here

| Need | Smallest useful entry |
| --- | --- |
| Understand the AI Company OS | [Company OS product system map](company-os/product-system-map.md) |
| Change Company OS product behavior | [Company OS contracts](company-os/README.md) |
| Change Mission/Wave or Agent Team orchestration | [Host-plan product contract](product/mission-wave-host-plan.md), [Agent Team foundation](product/agent-team-foundation-closure-plan.md), [Dogfood loop](product/agent-team-dogfood-loop.md), [Member continuation model](member-continuation-model.md), [ADR 0034](decisions/0034-host-plan-waves-and-mission-teams.md), [ADR 0037](decisions/0037-agent-member-autonomy-and-collaboration.md), [ADR 0039](decisions/0039-ordinary-member-planning-and-durable-mailbox-delivery.md), [ADR 0041](decisions/0041-provider-neutral-member-continuation.md), [ADR 0044](decisions/0044-durable-team-supervision-and-typed-mail.md), and [Architecture map](architecture-map.md) |
| Evaluate unresolved product or architecture evidence | [Research index](research/README.md); promote accepted findings into canonical contracts and ADRs before implementation |
| Implement or operate the repository | [Getting started](getting-started.md), [Operations](operations.md), [Schemas](schemas.md) |
| Change repository agent operating rules | Root [AGENTS.md](../AGENTS.md) and [Agent operating rules detail](agent-operating-rules.md) |
| Change frontend visual direction | [Company OS visual inventory](design/company-os-v2/visual-index.md) or [Execution Workbench V3](design/execution-workbench-v3/README.md) |
| Integrate a provider | [Integration index](integration/README.md) |
| Interpret an old decision | The relevant ADR, verified native export, or Git history. |

## Documentation modules

| Module | Entry points |
| --- | --- |
| Product | [PRD](prd.md), [Company OS](company-os/README.md), [Design basis](design-basis.md) |
| Architecture | [Architecture map](architecture-map.md), [Concept model](concept-model.md), [Data model](data-model.md), [ADRs](decisions/README.md) |
| Execution | [Dashboard](dashboard.md), [Workflow runtime](workflow-runtime.md), [Agent runtime](agent-runtime.md), [Agent Team foundation](product/agent-team-foundation-closure-plan.md), [Dogfood loop](product/agent-team-dogfood-loop.md), [Integration](integration/README.md) |
| Design evidence | [`design/`](design/) workstreams; use each workstream README/manifest, not the directory as product authority |
| Operations | [Getting started](getting-started.md), [Operations](operations.md), [Multi-project](multi-project.md), [Governance engine](governance-engine.md) |
| Research | [Research index](research/README.md); active input to decisions, never product authority or default context |
| Historical evidence | Verified native exports and Git history; never default context |

Project-specific tool usage belongs in `examples/adapters/**` or in the
integrating project repository, not in the generic core docs.

## Skills

| Skill | Use |
| --- | --- |
| [orchestrate-mission-waves](../skills/orchestrate-mission-waves/SKILL.md) | Thin Host guidance for durable Mission context, versioned Wave memos, Mission-linked long-lived Teams, assignments, and advance/re-plan. CLI remains the authority. |
| [collaborate-as-agent-team-member](../skills/collaborate-as-agent-team-member/SKILL.md) | Provider-neutral member guidance for assignment ownership, mailbox collaboration, native subagents, evidence, and review acceptance. |
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
