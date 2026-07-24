---
name: company-page-builder
description: Design, generate, implement, and visually verify a governed custom Company OS document page from an approved ModuleDesign. Use when a core page must combine multiple documents, typed records, relations, actors, work, approvals, finance, or decision controls beyond ordinary document blocks and standard views, while preserving scoped reads, governed Actions, human approvals, auditability, and a standard-view fallback.
---

# Company Page Builder

Build a custom page only when basic documents and standard views cannot express
the core workflow clearly. Treat the approved `ModuleDesign`, fixture, and
visual contract as inputs; never invent business facts to improve the mockup.

## Load the contracts

Read:

- the approved ModuleDesign and approval reference;
- `docs/company-os/agent-programmable-pages.md`;
- `docs/company-os/frontend-information-architecture.md`;
- `docs/company-os/skill-contracts.md`;
- `docs/decisions/0029-agent-programmable-document-runtime.md`;
- [the package contract](references/custom-page-contract.md).

Stop if the ModuleDesign is still proposed, lacks a named human approval, or
does not define a fallback view.

## Screenshot-first workflow

1. Build a deterministic fixture containing stable IDs, exact actor roles,
   timestamps, approvals, finance types, and negative assertions.
2. Capture the current page before implementation when a predecessor exists.
3. Define routes, viewport, prompt, artifact paths, truth assertions, and
   approval state in a visual manifest.
4. Generate the expected image before frontend implementation. Keep candidates
   separate. Generation is not approval; record a content hash and request
   explicit human approval.
5. Implement the page package using the approved expected image and the shared
   Company OS design system. Compose queries and standard Views rather than
   duplicating records in component state.
6. Declare every query and Action Command in both the definition and package.
   Use only the intersection granted by the runtime.
7. Route writes through the Action dispatcher. Never import a store writer,
   mutate a ledger, synthesize an Approval, or turn Commitment into Payment.
8. Provide loading, empty, error, permission-denied, approval-required, and
   fallback states. A render or package failure must open the same records in
   ordinary document/standard views.
9. Capture the implemented page with the same fixture and viewport. Check
   console errors, horizontal overflow, responsive behavior, and fixture truth.
10. Create a labeled expected-versus-actual comparison and record deviations.
    Do not mark the page accepted until P0 findings are closed.

Current Harness implementation boundary: `harness company docs page publish`
records candidate `CustomPagePackage` metadata only. It does not switch the
active `CustomPageDefinition.package_ref`. The Docs module UI should be used
to inspect active package, candidate package, fallback View, declared queries,
declared Actions, and visual contract before a future governed promotion path
exists.

## Package and validate

Start from
[`assets/custom-page-package.example.json`](assets/custom-page-package.example.json)
and keep the manifest declarative. Run:

```bash
python3 skills/company-page-builder/scripts/validate_page_package.py <package.json>
```

## Non-negotiable boundaries

- Do not use arbitrary server-side code execution.
- Do not request undeclared data or Actions.
- Do not bypass policy, permissions, named human approval, or audit output.
- Do not persist private model thinking. A sanitized live preview, when
  supported, is transient and cannot become evidence or history.
- Do not show availability, ownership, payment, settlement, or approval unless
  the fixture and canonical records state it explicitly.
- Do not replace a standard page with custom code when the standard page is
  already sufficient.
