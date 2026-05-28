# Agent Dashboard Docs

This directory contains the focused documentation for the Agent Dashboard
control plane. Use the project documentation workflow before adding new
Dashboard docs: decide which surface owns the claim, then place it there.

## Placement Map

| Path | Owns | Refuses |
| --- | --- | --- |
| `../dashboard.md` | Product-level Dashboard purpose, information architecture, object workflow, and user-facing acceptance | Component internals, run commands, detailed layout specs |
| `README.md` | Dashboard docs map, placement rules, and change order | Product semantics or component implementation |
| `design-principles.md` | Core frontend design principles, failure modes, graph/Kanban policy, AgentTeam and AgentMember UI doctrine | Route-level layout details or React module boundaries |
| `layout-variants.md` | Candidate Dashboard layout directions, Designer/Questioner critique, scoring rubric, and decision inputs | Final implementation spec or component internals |
| `layout-decisions.md` | Accepted layout direction, killed alternatives, module decisions, and visual placement constraints | Code implementation or unscored future ideas |
| `frontend-design.md` | Complete canonical Dashboard frontend design: selected shell, route map, page cards, visual placement, read-model needs, safe actions, responsive behavior, implementation sequence, and acceptance pointers | Product PRD, candidate layout critique, React internals, or local run commands |
| `frontend-architecture.md` | React/Vite architecture, component responsibilities, app-local boundaries | Product PRD, visual doctrine, or runbook commands |
| `read-model.md` | Snapshot projections, selectors, advisory warnings, and required read-model fields | Canonical validation rules or Rust implementation |
| `acceptance.md` | Browser screenshot evidence, web-quality gate, and frontend acceptance sequence | Product purpose or local development commands |
| `runbook.md` | Local run, build, snapshot, live API, and safe action entry points | Architecture rationale or UI doctrine |
| `../decisions/*` | Durable architectural decisions and hard-to-reverse tradeoffs | Day-to-day run instructions or unstable sketches |

## Change Order

Use this order for non-trivial Dashboard/frontend work:

```text
product purpose
  -> docs placement decision
  -> subagents restate Vision and selected Goal context
  -> core design principles
  -> three candidate layout variants and critique
  -> Decision Agent / Lead records selected and killed layouts
  -> module-level option loops for high-risk surfaces
  -> complete frontend design draft
  -> read-model/API needs
  -> frontend architecture changes
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

For Dashboard redesigns, keep three layout candidates in
[layout-variants.md](layout-variants.md) until a Decision Agent or Lead records
the selected direction in [layout-decisions.md](layout-decisions.md). Once
selected, move the stable page hierarchy, page cards, visual placement, safe
actions, read-model needs, responsive behavior, and acceptance pointers into
[frontend-design.md](frontend-design.md).

## Skill Boundary

The reusable frontend skill lives under
`.agents/skills/harness-frontend-product-design/`. It owns the agent procedure:
which docs to read, the two-subagent plus Decision Agent design loop, output
artifacts, and acceptance gates.

Canonical product and layout decisions still belong in `docs/`. The skill
should reference these docs instead of embedding the complete Dashboard spec.
When the skill discovers a repeated docs-placement problem, update this README
or the relevant canonical doc before changing frontend code.

## Split Rule

Keep each Markdown file near one reader and under the repository split target.
Split a Dashboard doc when it grows beyond roughly 500 lines, mixes readers, or
starts owning facts that belong to another surface.
