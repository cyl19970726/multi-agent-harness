# Product Module Discovery

Use this reference before page design. It helps a Product Manager and UX Designer
discover the right frontend module without copying an old route map or a generic
UI checklist.

## Start with capability, not interface form

Define:

- target users and the situation that triggers the need;
- the job, decision, or outcome they need to complete;
- required product capabilities and business rules;
- success measures and unacceptable failure;
- scope, non-goals, dependencies, assumptions, unknowns, and risks.

Do not start with “build a dashboard,” “add a detail page,” or “reuse these cards.”
Those are possible solutions, not the product definition.

## Map journeys and information needs

For each journey, record:

```text
actor and trigger
-> current question or task
-> information and authority needed
-> action or decision
-> system response
-> next question, recovery, or completion
```

Include primary success, first use, interruption, no data, partial data, permission,
failure, recovery, long/dense content, and responsive conditions only when relevant
to the actual capability.

## Decide what deserves a distinct surface

A separate page, mode, region, overlay, or transient state may be justified when
the user changes primary task, needs sustained focus, needs a different information
hierarchy, crosses a permission or destructive boundary, or must preserve context
while examining something else.

Keep behavior within an existing surface when it answers the same question, uses
the same context, and separation would add navigation or cognitive cost.

The PM/UX inventory records for every proposed surface:

```text
id and product-facing name:
journeys and target users:
primary question, decision, or task:
why this is a distinct surface:
entry, exit, return, and neighboring surfaces:
information, actions, rules, and authority:
required states and responsive behavior:
product, usability, visual, and technical risk:
coverage: dedicated design | approved pattern | accepted existing | excluded
```

“The old app has it,” “the component already exists,” and “most products use one”
are not sufficient rationale.

## Close the discovered graph

Walk every scoped journey and every user-visible transition. Each transition must
target a known surface/state and define return or completion behavior. Each included
surface must be reachable from a journey entry and have one coverage decision:

- `designed`: approved expected design plus surface contract;
- `pattern`: exact approved design-system reference plus applicability proof;
- `existing-accepted`: reviewed existing surface plus compatibility evidence;
- `excluded`: rationale, journey impact, approver, and follow-up when needed.

These coverage labels do not prescribe what surfaces exist. They only prevent a
surface already discovered by product/UX work from becoming an implementation guess.

## Product and UX readiness questions

- Can the PM explain the module without naming implementation components?
- Does each surface trace to a user outcome and journey?
- Is every separate surface justified, and is every unnecessary one removed?
- Can users complete the scoped outcome without entering an unlisted state?
- Are product rules, authority, failure, and recovery visible where they matter?
- Does responsive behavior preserve the same task or deliberately change it?
- Are core frames identified by visual/product risk rather than by a fixed quota?
- Has the Owner approved the module boundary and page/surface inventory?

Do not move to implementation while any material answer is unknown.
