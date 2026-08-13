# Frontend Module Spec

The Frontend Module Spec is one versioned contract joining product management,
UX, UI, architecture, implementation, and review. It is not a mockup gallery,
route inventory, or component checklist.

## 1. Identity and authority

Record module id, product-facing title, owner, canonical spec URI/id, revision,
status, superseded revision, source revisions, approvals, and conflict decisions.
Only one location is canonical; mirrors identify it.

## 2. PM product definition

Record problem statement, target users, jobs/outcomes, measurable success,
capabilities, business rules, product principles, in-scope, non-goals, dependencies,
assumptions, unknowns, risks, and representative real-data scenarios.

Include a page/surface decision table. Every entry states its primary user question
or task, linked journeys, rationale for being distinct, and why a simpler placement
would not serve the outcome. Product vocabulary comes from project authority, not
this skill or the template.

## 3. UX and information architecture

Record journeys, information architecture, navigation and return behavior,
interaction model, content hierarchy, disclosure, keyboard/focus, accessibility,
responsive transformations, and relevant state/recovery behavior.

Include the discovered surface graph from `product-module-discovery.md`. The spec
does not require particular surface kinds; it requires that every included surface
is justified, reachable, contracted, and covered.

## 4. Approved UI design

For each reference record id, URI/path, kind, immutable hash/revision, approval
owner/time, and surface/state/viewport coverage. Record approved deviations
separately; never overwrite a reference to make implementation appear conformant.

Define the shared UI language: composition and alignment anchors, typography,
density and spacing rhythm, color roles, surfaces and hairlines, radius/elevation,
controls, selected/focus/disabled/error behavior, icon and asset vocabulary,
motion and reduced motion, and forbidden fallback treatments.

For each included surface define first viewport, regions, dimensions, scroll
ownership, data density, wrapping/truncation, interaction, content, responsive
behavior, relevant states, reference ids, allowed deviations, blocking frames,
and observable pass/fail.

ASCII can preserve geometry; it cannot replace approved visual references for
identity-defining composition, typography, density, material, controls, or assets.
Adjectives such as `premium` or `clean` are not a UI system.

## 5. Architecture and complete implementation path

Record routes/navigation, component boundaries and shared primitives, reuse/adapt/
replace decisions, state/read-model and API contracts, permissions, cache/realtime
and mutation behavior, accessibility implementation, dependencies, assets,
migration, flags, rollback, and old-code disposition.

Plan all ordered slices before coding. Every slice names dependencies, owned paths,
requirements, journeys, surfaces, data/API needs, tests, screenshot checkpoints,
and stop threshold. Unknown visual polish is not a valid final slice.

## 6. Traceability and acceptance

Maintain a matrix:

```text
product requirement -> journey -> justified surface -> UI reference
  -> component/route -> API/read model -> test/journey
  -> exact-revision frame/evidence -> review result
```

Separate hard product/authority/privacy/accessibility pass-fail invariants from
visual scores. Declare blocking frames, per-frame and per-dimension thresholds,
P0/P1 policy, waivers, review roles, owner acceptance, and invalidation triggers.
A threshold below the default 95 per blocking frame requires an owner-approved
exception frozen before implementation.

## Readiness gate

Before implementation require:

- approved PM problem, capability boundary, scope, and success definition;
- journeys that justify a closed surface graph with no inherited or invented pages;
- approved UX/IA contract and core UI references with immutable revisions;
- complete design/pattern coverage for included surfaces and approved exclusions;
- architecture, data/API, permission, and migration feasibility;
- ordered slices covering the entire module;
- complete requirement-to-evidence traceability and acceptance rules;
- PM, Critic, Architect, and Owner `continue` decisions.

Use `../scripts/validate_frontend_module_spec.py` for structural validation. PASS
does not substitute for product judgment, design quality, or owner approval.
The bundled JSON template is intentionally domain-neutral. Its one placeholder
surface demonstrates shape only; it does not imply that a real module has one
surface or should use the same form.
