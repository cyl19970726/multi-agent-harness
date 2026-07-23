# Agent Workbench Frontend Architecture

This document owns the implemented frontend stack, module boundaries, data
flow, and component policy. Product semantics live in
[the Workbench product contract](../dashboard.md); page behavior lives in
[page specs](pages/README.md); the approved visual baseline lives in
[`execution-workbench-v3`](../design/execution-workbench-v3/README.md).

## Implemented Decision

```text
React 18 + strict TypeScript + Vite
Tailwind CSS v4 + owned shadcn/Radix primitives
lucide-react icons + Geist fonts + generated identity portraits
one responsive Workbench shell
pure read-model selectors over snapshot + SSE
typed action descriptors over the Rust HTTP API
root package.json owns dependencies
```

The source directory remains named `apps/agent-dashboard` for package and
command stability; the product is Agent Workbench. The frontend never owns
canonical Mission, Wave, AgentTeamRun, Company OS, assignment, approval, or
financial state.

## Data Flow

```text
Harness store / provider adapters
  -> Rust snapshot and action APIs
  -> project-scoped SSE deltas
  -> pure read-model selectors
  -> Mission, Team, Workflow, and Company OS surfaces
  -> screenshot and behavior acceptance
```

- A full snapshot establishes authority.
- SSE merges newer durable events and transient expiring member activity.
- Reconnect fetches a fresh snapshot; stale overlapping reads cannot overwrite
  newer action responses or live deltas.
- Project selection is explicit. URL selection state never substitutes for a
  canonical object relation.
- Thinking is sanitized transient state and is absent after expiry/reload.

## Module Boundary

```text
apps/agent-dashboard/src/
  app/               shell, selection, snapshot/SSE lifecycle
  surfaces/          Missions, Agent Teams, Team War Room, MemberRuns, Workflows
  company-os/        Docs, Organization, Work, Approvals, Finance, Governance
  model/             pure selectors and projection helpers
  components/ui/     owned shadcn/Radix primitives
  components/workbench/ shared execution and document primitives
  api.ts             reads, project selection, SSE, action transport
  api/actions.ts     typed write-action descriptors
  types.ts           wire and projection types
  index.css          tokens, typography, responsive and motion policy
```

Execution surfaces and Company OS surfaces share shell, typography, identity,
status, relation, activity, and context primitives. They do not collapse their
objects: a MemberRun is still different from a Standing Agent; a Wave gate is
different from a Human Approval; an AgentTeamRun is different from an OrgUnit.

## Surface Ownership

| Surface | Owns | Must not claim |
| --- | --- | --- |
| Mission Canvas | durable Mission Markdown, linked Teams, ordered Host-plan Wave revisions, explicit judgment, closeout | dependency graph, runtime containment, or implicit acceptance |
| Agent Teams Home | independent and Mission-scoped AgentTeam/TeamRun discovery | pretending every run belongs to one Wave |
| Team War Room | stable Team identity, Mission relation, member presence, assignment lineage, unified activity, messages, ACK/start | claiming a selected Wave owns the TeamRun or provider child |
| MemberRun Focus | one run-scoped member's contract and evidence | Standing Agent identity |
| Workflows | WorkflowRun/WorkflowStep/result/artifacts | Agent Team semantics |
| Company OS | Documents, WorkItems, actors, approvals, finance, metrics, governance | unimplemented schema authority |
| Debug | raw snapshot and diagnostics | primary product navigation |

## Component Policy

| Primitive | Purpose |
| --- | --- |
| `WorkbenchShell` | product rail, source state, responsive workspace, debug boundary |
| execution portraits and `Avatar` | stable identity with generated asset and text fallback |
| status/tone primitives | text-backed semantic state, never color-only |
| timeline/activity rows | assignment, handoff, runtime, evidence, review, decision semantics |
| context modules | Wave, Gate, Attempt, Member, Resources, linked company records |
| document primitives | basic rich content, properties, relations, structured views |
| operator forms | typed API commands with pending/error state and truthful disable reasons |

Avoid generic metric-card grids for primary workflows. Use cards only for
bounded interactive objects; use continuous document or timeline composition
for the main story. Icons and generated art must carry identity or semantics,
not decorative noise.

## Responsive Contract

- Desktop uses product rail, primary work surface, and contextual rail.
- Tablet collapses the product rail and permits contextual sheets/inline
  modules without hiding the gate or current pressure.
- Mobile shows one clear work story, explicit disclosure for secondary members
  or context, and no horizontal overflow.
- Motion communicates progress, selection, and readiness; it respects
  `prefers-reduced-motion` and never implies nonexistent runtime activity.

## Technology Policy

| Area | Decision |
| --- | --- |
| Routing | URL-addressable selection handled by the app selection layer; add a router only when nested navigation needs it. |
| State | local React state plus pure selectors; canonical state stays server-side. |
| Styling | Tailwind v4 tokens plus owned CSS for high-fidelity execution compositions. |
| UI primitives | shadcn/Radix copy-in components, wrapped by product primitives. |
| Icons | lucide plus purpose-built generated identity assets. |
| Graph/canvas | no library unless a future Company OS view has a semantic graph requirement and a list fallback. |
| Dependencies | root `package.json`; no second full component framework without an ADR. |

## Visual Implementation Contract

Design images establish hierarchy, density, material, iconography, and motion
intent. Implementation must record expected, baseline, actual, comparison,
overlay, and intentional deviations in a versioned visual contract. A design
is not considered implemented because the same content exists at larger card
sizes; layout rhythm, continuous flow, semantic icons, pressure placement, and
responsive behavior are acceptance criteria.

The active contract is
[`docs/design/execution-workbench-v3/visual-contract.json`](../design/execution-workbench-v3/visual-contract.json).

## Validation

```bash
npx pnpm@9.15.4 check:dashboard
npx pnpm@9.15.4 acceptance:mission-wave
```

The first command proves types, selectors, operator controls, visual fixture
semantics, and production build. The second also proves native Mission/Wave,
MCP, TeamRun, Kimi, Codex, and mixed-provider execution contracts.
