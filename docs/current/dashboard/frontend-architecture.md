# Agent Workbench Frontend Architecture

This document owns the implemented frontend stack, module boundaries, data
flow, and component policy. Product semantics live in
the Workbench product contract; page behavior lives in
[page specs](pages/README.md); the approved visual baseline lives in
`execution-workbench-v3`.

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
canonical AgentTeam, AgentTeamRun, Work, Message, session, or runtime state.
The retired Mission/Wave and Company OS surfaces are gone from navigation
(DEV-38, DOC-108).

## Data Flow

```text
Harness store / provider adapters
  -> authenticated Rust snapshot, RoleView and action APIs
  -> project-scoped SSE deltas
  -> pure read-model selectors
  -> Nodes, Agent Teams, Team Workspace, Agent Conversation, Global Work, and
     Workflow surfaces
  -> screenshot and behavior acceptance
```

- A full snapshot establishes authority.
- SSE merges newer durable events and transient expiring member activity.
- Reconnect fetches a fresh snapshot; stale overlapping reads cannot overwrite
  newer action responses or live deltas.
- Project selection is explicit. URL selection state never substitutes for a
  canonical object relation.
- Thinking is sanitized transient state and is absent after expiry/reload.
- Agent Workspace reads one authenticated, server-built RoleView. The browser
  never joins Team snapshots to MemberRuns or native provider activity, and a
  Host's provider-private Session projection is never populated from another
  Agent's native events.

## Module Boundary

```text
apps/agent-dashboard/src/
  app/               shell, selection, snapshot/SSE lifecycle
  surfaces/          OperatorView, AgentTeamsHome, TeamWorkspace,
                     AgentConversationWorkspace, GlobalWorkIndex, HostConsole,
                     Workflows
  model/             pure selectors and projection helpers
  components/ui/     owned shadcn/Radix primitives
  components/workbench/ shared execution primitives
  api.ts             reads, project selection, SSE, action transport
  api/actions.ts     typed write-action descriptors
  types.ts           wire and projection types
  index.css          tokens, typography, responsive and motion policy
```

Surfaces share shell, typography, identity, status, relation, activity, and
context primitives. They do not collapse their objects: a MemberRun is still
different from an AgentMember, and a TeamRun is different from a durable
AgentTeam.

## Surface Ownership

| Surface | Owns | Must not claim |
| --- | --- | --- |
| Agent Teams Home | durable Node-placed AgentTeam discovery and Team routes | implying Teams belong to a retired Mission or inherit runtime state |
| Team Workspace | stable Team identity, shared Works, current Supervisor, authenticated Message actors, WorkDelivery claim/receipt/failure, per-recipient CanonicalMessageDelivery state, member presence, Work-linked conversation, unified activity, and controls | impersonating a Member, consulting Legacy TeamMessage state, or fabricating provider control |
| Agent Conversation Workspace | one shared Host/Member shell with Team roster, exact Session activity, authored Messages, Work responsibility, selected context, profile/configuration and server-authorized actions | browser-authored authority, a second Work/Message model, copied provider transcript, or cross-Agent provider-private events |
| Global Work Index | the read-only Global Work aggregate over authoritative TeamWork | a second task ledger or a Work mutation path |
| Operator View / Nodes | ExecutionNode and machine-scoped NodeDaemon state | per-Team daemon claims |
| Legacy archive | Historical Dynamic Workflow export/verify/restore-read evidence | current execution, Agent Team semantics, or mutation actions |
| Debug | raw snapshot and diagnostics | primary product navigation |

## Component Policy

| Primitive | Purpose |
| --- | --- |
| `WorkbenchShell` | product rail, source state, responsive workspace, debug boundary |
| execution portraits and `Avatar` | stable identity with generated asset and text fallback |
| status/tone primitives | text-backed semantic state, never color-only |
| timeline/activity rows | WorkEvent, conversation, runtime, evidence, review, and decision semantics |
| context modules | Gate, Attempt, Member, Resources, linked legacy context |
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
| Graph/canvas | no library unless a future view has a semantic graph requirement and a list fallback (the retired Company OS views are not such a case). |
| Dependencies | root `package.json`; no second full component framework without an ADR. |

## Visual Implementation Contract

Design images establish hierarchy, density, material, iconography, and motion
intent. Implementation must record expected, baseline, actual, comparison,
overlay, and intentional deviations in a versioned visual contract. A design
is not considered implemented because the same content exists at larger card
sizes; layout rhythm, continuous flow, semantic icons, pressure placement, and
responsive behavior are acceptance criteria.

The legacy execution-family contract is preserved in git history
(`design/execution-workbench-v3`). Agent Workspace visual references are
approved and governed in AgentFirm DOC-72; generated reference images are not
copied into this repository.

## Validation

```bash
npx pnpm@9.15.4 check:dashboard
npx pnpm@9.15.4 acceptance:legacy-retirement
```

The first command proves types, selectors, operator controls, visual fixture
semantics, and production build. The second proves the deterministic Agent
Team, MCP, Kimi, Codex, and mixed-provider execution contracts plus the
retired Mission/Wave legacy reads.
