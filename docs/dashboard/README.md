# Agent Workbench Docs

This directory contains the focused documentation for the Agent Workbench
control plane. The directory keeps the legacy `dashboard` path for current
commands and links, but the user-facing product surface is a workbench. Use the
project documentation workflow before adding new Workbench docs: decide which
surface owns the claim, then place it there.

## Placement Map

| Path | Owns | Refuses |
| --- | --- | --- |
| `../dashboard.md` | Product-level Workbench purpose, information architecture, object workflow, and user-facing acceptance | Component internals, run commands, page-level layout contracts |
| `README.md` | Workbench docs map, placement rules, and change order | Product semantics or component implementation |
| `design-principles.md` | Core frontend design principles, failure modes, graph/Kanban policy, AgentTeam and AgentMember UI doctrine | Route-level layout details or React module boundaries |
| `layout-variants.md` | Candidate Workbench layout directions, Designer/Questioner critique, scoring rubric, and decision inputs | Final implementation spec or component internals |
| `layout-decisions.md` | Accepted layout direction, killed alternatives, module decisions, and visual placement constraints | Code implementation or unscored future ideas |
| `frontend-design.md` | Workbench frontend design index, reading order, page-spec map, and implementation readiness summary | Page-level details, React internals, or run commands |
| `pages/README.md` | Page-spec index and template | Hard dimensions, ASCII wireframes, or implementation internals |
| `pages/*.md` | One page/workspace product, UX, and layout contract: purpose, object ownership, workflow proof, IA, actions, detailed desktop/tablet/mobile ASCII diagrams, dimensions, scroll ownership, failure modes, screenshot questions | Component implementation |
| `hard-layout-specs/README.md` | Historical/deprecated layout-attempt index | Current implementation gate or page purpose |
| `hard-layout-specs/*.md` | Historical hard-layout attempts kept for failure analysis and migration context | Current page layout contracts, product doctrine, or component implementation details |
| `rejected-implementations/README.md` | Failed frontend implementation record index and template | Active design decisions or release notes |
| `rejected-implementations/*.md` | Rejected browser-visible attempts, violated gates, old-code contamination, restart point | New design specs or implementation instructions |
| `frontend-architecture.md` | React/Vite architecture, component responsibilities, app-local boundaries | Product PRD, visual doctrine, or runbook commands |
| `read-model.md` | Snapshot projections, selectors, advisory warnings, and required read-model fields | Canonical validation rules or Rust implementation |
| `acceptance.md` | Browser screenshot evidence, web-quality gate, and frontend acceptance sequence | Product purpose or local development commands |
| `runbook.md` | Local run, build, snapshot, live API, and safe action entry points | Architecture rationale or UI doctrine |
| `../decisions/*` | Durable architectural decisions and hard-to-reverse tradeoffs | Day-to-day run instructions or unstable sketches |

## Change Order

Use this order for non-trivial Workbench/frontend work:

```text
product purpose
  -> docs placement decision
  -> subagents restate Vision and selected Goal context
  -> core design principles
  -> three candidate layout variants and critique
  -> Reviewer / Lead records selected and killed layouts
  -> module-level option loops for high-risk surfaces
  -> page specs under pages/
  -> frontend design index update
  -> architecture and stack decision
  -> page-local layout contracts under pages/<page>.md
  -> read-model/API needs
  -> implementation
  -> browser and web-quality acceptance
  -> follow-up tasks or ADRs
```

If a design idea cannot be placed in this map, stop and route it first. Do not
put product meaning, layout choices, and component implementation in one file.

For design work that uses subagents, both the Designer and Questioner must first
read and restate the project Vision, final acceptance standard, selected Goal,
and distance-to-vision context. If they cannot do that, their design feedback is
not ready to drive frontend decisions.

For Workbench redesigns, keep three layout candidates in
[layout-variants.md](layout-variants.md) until a Reviewer or Lead records the
selected direction in [layout-decisions.md](layout-decisions.md). Once
selected, move the stable page hierarchy, page cards, visual placement, safe
actions, read-model needs, detailed desktop/tablet/mobile ASCII diagrams,
responsive behavior, and acceptance pointers into page specs under
[pages/](pages/). Keep [frontend-design.md](frontend-design.md) as the index
and implementation-readiness summary.

Implementation may start only from page specs that include accepted
`## Layout Contract` sections. A broad direction such as "Team workspace first"
or a single shell diagram is not enough.

## Skill Boundary

The reusable frontend skill lives under
`.agents/skills/harness-frontend-product-design/`. It owns the agent procedure:
which docs to read, the multi-candidate Designer -> Questioner -> Reviewer
loop, output artifacts, page-local layout contract gate, and acceptance gates.

Canonical product and layout decisions still belong in `docs/`. The skill
should reference these docs instead of embedding the complete Workbench spec.
When the skill discovers a repeated docs-placement problem, update this README
or the relevant canonical doc before changing frontend code.

## Split Rule

Keep each Markdown file near one reader and under the repository split target.
Split a Workbench doc when it grows beyond roughly 500 lines, mixes readers, or
starts owning facts that belong to another surface.
