# 0016: Tailwind v4 + shadcn/ui Adoption For Agent Workbench

## Status

Accepted.

Supersedes in part [0014](0014-react-vite-agent-dashboard.md): React + TypeScript
+ Vite remain the build/runtime shell, but the styling and UI-primitive choices
in 0014 (hand-rolled CSS, no UI kit) are replaced by this decision.

## Context

The first Agent Workbench shell was built with hand-rolled CSS and custom
Workbench primitives and no UI kit. That shell was rejected (PR #6): the
browser-visible surface read as a dense dashboard/card dump rather than a
Feishu-like collaboration workspace, and the failure was structural information
architecture, not spacing or color polish. See
[../dashboard/layout-history.md](../dashboard/layout-history.md) for the full
rejected/selected ledger.

Maintaining a bespoke CSS layer and reinventing accessible primitives (dialogs,
menus, tabs, tooltips) slowed the rebuild and produced inconsistent, hard-to-test
UI behavior. The product needed an accessible, token-driven base it could own and
adapt without reinventing primitive behavior.

## Decision

Adopt the following frontend stack for the Agent Workbench, on top of the
React 18 + TypeScript + Vite shell kept from 0014:

- **Tailwind CSS v4** via `@tailwindcss/vite` for styling and design tokens; a
  dark operator-console theme lives in `src/index.css`.
- **shadcn/ui primitives over Radix** for accessible base components, configured
  in `apps/agent-dashboard/components.json` (style `new-york`) and generated into
  `src/components/ui`.
- **Product atoms** in `src/components/workbench`, composed from the shadcn/ui
  primitives.
- **lucide-react** for icons and **Geist + Geist Mono** for fonts.
- **Dependencies declared in the ROOT `package.json`**; there is no
  `apps/agent-dashboard/package.json`.
- **Module boundary**: `src/app` (composition, shell, selection state),
  `src/surfaces` (page surfaces), `src/model` (snapshot types, read-model
  selectors, warnings), and `src/components` (`ui/` primitives and `workbench/`
  atoms).

## Consequences

- The rebuild merged as PR #7 and is the shipped Agent Workbench frontend.
- Accessible primitive behavior (focus, keyboard, dismiss) comes from Radix
  through shadcn/ui instead of bespoke code.
- Styling is token-driven and consistent across surfaces; the operator-console
  theme is centralized in `src/index.css`.
- A single dependency surface (root `package.json`) keeps the gated monorepo
  build deterministic.
- shadcn/ui primitives are copy-in and owned by this repo, so they can be adapted
  to Workbench needs without waiting on an upstream component framework.
- 0014 stays valid for the React/Vite build path and source-of-truth boundary;
  only its styling and UI-kit stance is superseded here.

## Validation

```bash
npx pnpm@9.15.4 check:dashboard
npx pnpm@9.15.4 check
```

The dashboard check builds the Workbench (tsc + Vite). Browser screenshot-first
acceptance follows [../dashboard/acceptance.md](../dashboard/acceptance.md).
