---
name: harness-frontend-product-design
description: "Lead a frontend feature from product definition through UX, UI, architecture, implementation, and visual acceptance. Use when a team must discover what a product module actually contains, decide its pages and states from user journeys, create approved design references, freeze a versioned frontend module spec, plan the full implementation path, or review exact-revision browser evidence against that spec."
---

# Frontend Product Design Harness

Treat a complete product capability—not a ticket, page, component, or example
named in this skill—as the unit of design, implementation, and acceptance.

The Product Manager must first discover what the capability actually contains.
Its pages and states come from users, product rules, jobs, journeys, information
architecture, and risk. There is no universal page list. A module may be one
focused surface or a family of routes, modes, overlays, and responsive states.

Names such as card, detail page, drawer, or dialog are possible UI forms, never
required product objects. Do not add one because this skill, an old implementation,
or a reference example happens to mention it.

## Hard Boundary

Do not embed or infer a specific project's:

- product objects, lifecycle, organization model, or vocabulary;
- required pages, routes, navigation, actions, or state names;
- storage schema, command line, task system, provider, or framework;
- repository paths or document-system hierarchy.

Load project truth at runtime from the user request, authoritative product sources,
the current product, and the repository. Cite those sources in the design artifacts.
If they conflict, expose the decision instead of inventing product meaning.

## Non-Negotiable Contract

For material frontend work, build this evidence chain:

```text
authority and product problem
  -> PM module brief and capability boundary
  -> user journeys and justified page/surface inventory
  -> UX/IA contract
  -> approved UI direction and visual references
  -> architecture and complete implementation path
  -> owner-approved Frontend Module Spec
  -> staged implementation and complete self-review
  -> exact-revision independent spec review
  -> owner acceptance
```

Do not start implementation because one attractive mockup exists. Implementation
starts only after the spec explains what the whole module contains, why each
surface exists, how users move through it, what it looks like, how it is built,
and how it will be accepted.

## Roles And Independence

Use separate working contexts where independent judgment matters:

- Product Manager: owns the problem, users, outcomes, capability boundary,
  product rules, success measures, page/surface inventory, and scope decisions.
- Product Researcher: gathers authority and current-state evidence when needed.
- UX/IA Designer: owns journeys, information architecture, navigation,
  interaction, content hierarchy, responsive behavior, and accessibility.
- UI/Visual Designer: owns composition, visual system, assets, motion, and
  approved expected designs for visually consequential states.
- Critic: challenges completeness, assumptions, usability, coherence, and feasibility.
- Architect: maps the approved product/UX/UI contract to the real code and data path.
- Implementer: builds the complete authorized module and produces revision-bound evidence.
- Independent Reviewer: judges the implementation against the frozen spec and references.
- Owner/User: approves the spec and accepts or rejects the delivered product.

Small modules may combine PM, research, UX, UI, and architecture roles. Never
combine Implementer and Independent Reviewer for an acceptance claim. Changing
the role prompt inside one reasoning context does not create independence.

## Context And Artifact Discipline

Give each role only what it needs. Designers receive reconciled product findings,
not the coordinating session's preferred layout. Critics receive raw candidates
and authority, not a recommendation. Reviewers receive the frozen spec, immutable
reference revisions, deviations, rubric, and exact-revision product—not
implementation effort or test persuasion.

Each material artifact records:

```text
type, id, revision, round, status:
author role and execution-context id:
input authority and immutable references:
scope, non-goals, facts, assumptions, and unknowns:
decisions, rationale, evidence, and open questions:
requested next action and superseded artifact:
```

Use one canonical spec URI/id and revision. Mirrors must identify the canonical
source; repository and external-document copies cannot both claim authority.

## Workflow

### 1. Build the source packet

Gather the user request, product authority and revisions, current product behavior,
research, analytics or support evidence when available, approved brand/design
references, representative real data, implementation entry points, and delivery,
accessibility, responsive, privacy, and technical constraints.

Separate fact, inference, and unknown. Do not reconstruct product truth from the
component tree when a stronger authority exists.

### 2. Have the PM define the module before designing pages

Read [`references/product-module-discovery.md`](references/product-module-discovery.md).
The PM produces a module brief containing:

- problem, target users, jobs/outcomes, success measures, and product principles;
- capabilities and business rules required to achieve the outcome;
- in-scope, non-goals, dependencies, assumptions, unknowns, and risks;
- current-state failures and decisions that need owner authority;
- an initial journey set and a justified candidate page/surface inventory.

Every candidate surface must state which user decision or sustained task it serves
and why it deserves its own place in the experience. Reject inventories copied
from old routes, implementation components, generic dashboard patterns, or examples
in this skill.

The PM gate passes only when the Owner agrees that this is the right product
module to build. Visual design cannot repair a wrong product boundary.

### 3. Turn the product brief into a complete UX/IA contract

The UX/IA Designer walks each journey from entry through success, interruption,
failure, recovery, and return. Define information architecture, navigation,
interaction states, content hierarchy, progressive disclosure, keyboard/focus,
responsive transformation, and honest loading/empty/restricted/error behavior.

Build a graph of the surfaces actually discovered by those journeys. Follow every
affordance to its visible consequence, but do not manufacture pages to satisfy a
generic checklist. The graph is closed when every scoped transition targets a
known state and every included surface has a reason, contract, and coverage plan.

Run the discovery and closure checks in `references/product-module-discovery.md`
before visual concept approval.

### 4. Design the module as one coherent UI system

Ask for divergent concepts when the design premise is genuinely open. Vary
information hierarchy, spatial model, interaction, density, or responsive strategy,
not just colors and radii.

Each concept must show how the discovered module works as a family:

- primary user journey and first-viewport hierarchy;
- visually consequential pages, modes, and transitions;
- representative real-density, long-content, empty, error, permission, and
  responsive states where they materially change the design;
- typography, spacing, color, surface, control, icon/asset, focus, and motion language;
- accessibility, data, and implementation implications;
- what existing behavior or design is preserved, adapted, removed, or added, and why.

Do not overproduce pictures. Require approved visual references for identity-defining,
novel, high-risk, or acceptance-critical states. Lower-risk states may use a precise
approved pattern and written contract. A few correct core frames plus a complete
spec are better than many inconsistent generated images.

### 5. Critique the whole product design and obtain owner direction

Give the Critic the source packet, PM brief, UX graph, and raw UI candidates.
Require findings on product completeness, missing journeys, unnecessary surfaces,
hierarchy, usability, responsive behavior, accessibility, visual coherence,
technical feasibility, and P0/P1/P2 risks.

For adapted or legacy designs, record a decision for each relevant element:
preserve, adapt, remove, or add. Explain product rationale and spatial consequence;
removing an area without reallocating its space is not a complete design decision.

The Owner approves the design direction. Critic preference does not replace owner
approval, and an approved picture does not yet authorize implementation.

### 6. Design architecture and the complete implementation path

The Architect inspects the repository and maps every included journey and surface
to routes, components, shared primitives, assets, state/read models, APIs, cache or
realtime behavior, mutations, permissions, accessibility, dependencies, migration,
old-code disposition, feature flags, tests, and owned paths.

Distinguish semantic reuse, visual reuse, adaptation, and replacement. Reusing
data logic does not make an old visual shell acceptable. Identify missing backend
or design-system capabilities before frontend implementation starts.

Plan all implementation slices and dependencies before coding. Each slice names
the product requirements, surfaces, owned paths, data/API needs, tests/journeys,
screenshot checkpoints, and local stop threshold. UI quality is part of every
slice, not an undefined final polish phase.

### 7. Freeze and validate the Frontend Module Spec

Build one versioned contract using
[`references/frontend-module-spec.md`](references/frontend-module-spec.md) and
[`assets/frontend-module-spec.template.json`](assets/frontend-module-spec.template.json).
It joins PM, UX, UI, architecture, implementation, and review decisions.

The template is a field scaffold, not an example product. Replace its placeholders
with project-derived content; do not preserve its surface count, ids, sequence, or
form unless discovery independently justifies them.

Run `scripts/validate_frontend_module_spec.py <spec.json>`. Structural PASS does
not prove that the product definition or design is good. Require PM, Critic,
Architect, and Owner readiness decisions before implementation.

### 8. Implement through staged internal gates

Use [`references/fidelity-and-review.md`](references/fidelity-and-review.md):

```text
product and data foundation
  -> module composition and navigation
  -> primary journey
  -> remaining discovered surfaces and states
  -> shared UI/interaction system
  -> responsive and accessibility coverage
  -> exact-revision complete-module evidence
```

At each stage, implement the bounded slice, run relevant engineering checks,
capture representative states, compare them with the frozen spec/references, and
repair or return to the responsible design layer. Do not advance while a hard
invariant fails, a stage-blocking frame misses its threshold, or P0/P1 remains.

Do not request independent review after each small change. The Implementer should
finish the planned module, use internal critique during development, and submit
one coherent candidate only after complete self-review.

### 9. Complete exact-revision self-review

Freeze an evidence bundle containing implementation and runtime revisions, spec
revision, approved reference hashes, representative real-data identity, coverage
for every required journey/surface/state/viewport, comparisons for blocking frames,
per-frame diagnostic scores, deviations, console/network/overflow/keyboard/
accessibility/performance checks, and unresolved findings.

The default visual bar is at least 95/100 for every blocking frame, not an average.
Resolve P0/P1 before handoff. Self-review grants review eligibility, not acceptance.

### 10. Run independent spec-conformance review and owner acceptance

Create a fresh Reviewer context. Require one reference-bound observation for every
blocking frame and tested journey plus a systemic diagnosis. Each defect names:

```text
requirement, journey, and surface:
severity and observed evidence:
spec/reference difference:
causal layer: product | UX | UI system | architecture | implementation | evidence
repair and observable recheck condition:
```

Reject verdict-only reviews, generic polish, component-presence acceptance, and
averages that hide a failed frame. Route findings back to the causal role.

Any relevant implementation, runtime, spec, reference, coverage, threshold,
deviation, or owner decision change invalidates affected review evidence. Close
only when independent review passes and the Owner accepts the complete module.

## Stop Conditions

Stop for owner authority when product sources conflict, success or scope is
ambiguous, or a UX-changing business decision is missing.

Stop design when the PM brief is unapproved, journeys do not justify the surface
inventory, core visual references are absent, or visual language is only adjectives.

Stop implementation when the spec is incomplete, architecture/API gaps are hidden
with frontend invention, repeated local styling does not fix systemic composition,
or the implementation is being allowed to redefine the approved product design.

Do not claim independent acceptance without a separate execution context. Report
Implementer self-review only.

## Quality Standard

Judge the delivered module across:

1. product problem, users, outcomes, rules, scope, and success;
2. journey completeness and justified page/surface architecture;
3. UX hierarchy, navigation, interaction, content, responsive behavior, and accessibility;
4. UI composition, density, typography, spacing, surfaces, controls, assets, and motion;
5. fidelity to each approved reference and declared deviation;
6. architecture, data/API truth, permissions, maintainability, and migration;
7. complete implementation, exact-revision evidence, independent review, and owner acceptance.

Passing tests, rendered data, a clean console, a completed checklist, or one good
screenshot cannot alone establish frontend product quality.
