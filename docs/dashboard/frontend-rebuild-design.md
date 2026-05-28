# Agent Dashboard Frontend Rebuild Design

This document is the implementation-ready design draft for rebuilding the
Agent Dashboard frontend. It must be reviewed before code changes continue.

## Vision And Goal

Multi-Agent Harness is a durable coordination system for persistent
AgentTeams. The Dashboard must let an operator reconstruct and control this
chain without reading raw JSON:

```text
Vision -> Goal collection -> GoalDesign -> AgentTeam -> TaskGraph
  -> Message assignment -> AgentMember runtime -> Evidence / Proposal
  -> Review / Decision -> GoalEvaluation -> distance-to-vision -> next Goal
```

Current implementation goal:

```text
Build a product-grade multi-agent collaboration observability/control console,
starting with a complete frontend redesign before implementation.
```

## Failed Attempt Record

The first implementation attempt is killed. It only reshaped the old component
tree and kept the product feeling like a debug page. It failed because:

- raw/debug concepts still shaped the app shell;
- the layout remained a stack of panels rather than a designed workspace;
- AgentMember still read as a compact detail card, not a durable teammate;
- Team state was still mostly inferred from task references;
- there was no complete page-level design record before code changes;
- browser screenshots showed a technically functional but visually weak product.

Do not continue by patching that attempt. The next implementation must start
from the page specs in this document.

## Product Shape

The Dashboard is a multi-agent workspace, not a metrics dashboard and not a
component gallery. It should feel closer to a focused collaboration console:

```text
Global app shell
  -> Vision / Goal context
  -> persistent Team workspace
  -> AgentMember workbench
  -> Goal and Task documents
  -> graph / Kanban relationship views
  -> docs context
  -> evidence / review / decision queue
  -> warnings and safe repair actions
  -> debug drawer
```

No shadcn dependency. Use React, TypeScript, custom CSS, and lucide icons only
where icons improve tool recognition.

## Route Map

| Route | Purpose |
| --- | --- |
| `/` | Default Team workspace for active Vision/Goal. |
| `/visions/:visionId` | Vision overview, goal collection, distance-to-vision, next goals. |
| `/teams/:teamId` | Persistent AgentTeam workspace, role groups, queues, decision queue. |
| `/members/:memberId` | Focused AgentMember workbench. |
| `/goals/:goalId` | Goal document with GoalDesign, team design, graph/Kanban, evidence, review, decision, evaluation. |
| `/tasks/:taskId` | Task document with assignment proof, messages, evidence, proposal, review, decision, branch/worktree/PR. |
| `/docs` | Mounted docs context with links back to active objects. |
| `/debug` | Raw snapshot, import/export, and low-level object views. |

The first implementation may be single-route internally, but it must be
structured as if these routes exist. Do not design a one-page stack that blocks
future route extraction.

## Implementation Boundary

The current Dashboard frontend implementation is deprecated as a product UI.
The rebuild must not be a refactor or reskin of the current component tree.

Implementation should delete or replace the old page stack:

```text
TopBar + SummaryGrid + ControlPlane + KanbanBoard + TaskDetail
  + MemberDetail + WarningsPanel + RawViews
```

The new implementation may preserve only stable contracts when they are still
useful:

- `types.ts` snapshot object interfaces, adjusted as needed;
- API helpers for `/v1/snapshot` and safe actions;
- small pure read-model utilities after review;
- Vite/React build setup.

All visual structure, component names, layout hierarchy, route composition, and
CSS should be rebuilt from this document. If old code is reused temporarily for
data extraction, it must not determine the new product shape.

## Core Page Specs

### Vision Overview

- Why it exists: show that Vision is a goal collection and learning loop.
- Primary question: are completed and active goals moving the product closer to
  the target state?
- Objects: Vision context, Goals, GoalEvaluation, NextRoundPlan, GoalCase.
- Layout: left goal collection, center vision progress and active path, right
  evaluator/next-round panel.
- Mobile: tabs for Goals, Progress, Next.
- Acceptance: completed, not-complete, blocked, proposed, and archived goals
  are visually distinct.

### Team Workspace

- Why it exists: the operator starts from a persistent AgentTeam, not raw tasks.
- Primary question: who is active, who is blocked, and what needs action now?
- Objects: AgentTeam, AgentMember, Message, Task, Evidence, Decision, Warning.
- Layout: Team rail, central activity/work queue, right contextual inspector.
- Primary actions: select member, send message, deliver queued work, request
  review, open Goal/Task documents.
- Mobile: Team, Work, Member, Warnings, Docs tabs.
- Acceptance: a Team with idle members remains visible even when no task points
  to those members.

### AgentMember Workbench

- Why it exists: each AgentMember is a durable teammate identity.
- Primary question: what is this member doing, what has it received, and what
  evidence supports its claims?
- Objects: AgentMember, inbox/outbox Messages, ProviderSession, AgentEvent,
  ProviderChildThread, Evidence, Proposal.
- Layout: identity/status header, current work, runtime health, activity
  timeline, inbox/outbox, sessions, prompt/skills/permissions, actions.
- Primary actions: send message, deliver, retry, reconcile, close.
- Acceptance: activity is chronological and canonical, not a chat-only stream.

### Goal Document

- Why it exists: Goal is a durable outcome, not a task list.
- Primary question: why does this goal exist and is it accepted?
- Objects: Goal, GoalDesign, AgentTeam, TaskGraph, Evidence, Proposal,
  Review, Decision, GoalEvaluation.
- Layout: objective/success criteria, design block, branch lane, team block,
  graph/Kanban, evidence/review/decision, evaluation, related docs.
- Acceptance: goal completion cannot be inferred from tasks alone.

### Task Document

- Why it exists: Task proves assignment, work, evidence, review, and decision.
- Primary question: did this task follow the harness protocol?
- Objects: Task, Message, AgentMember, ProviderSession, Evidence, Proposal,
  Decision, branch/worktree/PR refs.
- Layout: objective, acceptance, assignment proof, assignee/runtime, messages,
  evidence, proposal/review/decision, Git refs, warnings.
- Acceptance: missing `Message(kind=task)` is visibly incomplete.

### Graph / Kanban View

- Why it exists: graph explains relationships; Kanban explains execution.
- Objects: Goals, Tasks, blockers, dependencies, follow-ups, evaluations.
- Layout: controlled graph and lane board from the same read model, with node
  selection opening a document/inspector.
- Mobile: Kanban/document first; graph opens as focus mode.
- Acceptance: graph and Kanban cannot disagree about object status.

### Docs Context

- Why it exists: operators need canonical docs without leaving the control
  plane.
- Objects: docs registry, active Vision/Goal/Task/Decision links.
- Layout: right inspector tab and `/docs` route; selected inline doc links in
  Goal/Task documents.
- Acceptance: docs are mounted context, not copied product truth.

### Evidence / Review / Decision

- Why it exists: acceptance must be auditable.
- Objects: Evidence, Proposal, critic findings, Decision, check outputs.
- Layout: global decision queue plus object-local acceptance block.
- Acceptance: every accepted task links to check evidence and review output or
  an explicit waiver.

### Warnings / Repair Queue

- Why it exists: Dashboard should surface protocol gaps before they become
  hidden failures.
- Objects: warnings derived from canonical state.
- Layout: global queue, object-local warning callouts, safe repair actions.
- Acceptance: warning shows affected object, severity, why it matters, and
  navigation/repair when available.

### Debug Drawer

- Why it exists: raw snapshot tooling is useful but not the product.
- Objects: raw Messages, Sessions, Evidence, Decisions, JSON import/export.
- Layout: collapsed drawer or `/debug` route only.
- Acceptance: raw JSON is not visible in the default first viewport.

## High-Risk Module Options

### Team Workspace

- Option A: Feishu-like team shell with Team rail, activity/workspace, inspector.
  Selected because it best supports persistent AgentTeams.
- Option B: Goal document shell with Team as side panel. Rejected as primary
  because it makes agents feel secondary.
- Option C: graph-first control plane. Rejected as primary because teams are not
  dependency graphs.

Borrowed ideas: Goal document depth from B, controlled graph focus from C.

### AgentMember Workbench

- Option A: full teammate workbench with timeline, inbox/outbox, runtime, and
  actions. Selected.
- Option B: compact inspector card. Rejected because it hides live workflow.
- Option C: chat-only thread. Rejected because it loses Evidence/Decision
  semantics.

### Goal / Task Documents

- Option A: document pages with embedded graph/Kanban and acceptance blocks.
  Selected.
- Option B: drawers from a board. Rejected for complex audit trails.
- Option C: cards only. Rejected because cards cannot prove protocol order.

### Graph / Kanban

- Option A: synchronized tabs with focus mode. Selected.
- Option B: full infinite canvas first. Rejected for mobile and operator speed.
- Option C: Kanban only. Rejected because dependencies and follow-ups disappear.

### Docs Context

- Option A: inspector tab plus `/docs` route. Selected.
- Option B: external links only. Rejected because context is too weak.
- Option C: copied docs inside Goal/Task pages. Rejected because source of truth
  becomes ambiguous.

### Warnings / Decision Queue

- Option A: global queue plus object-local warning callouts. Selected because it
  keeps Lead decisions visible and makes object-local causes actionable.
- Option B: inspector-only warnings. Rejected because the operator can miss the
  affected section.
- Option C: toast or notification feed. Rejected because it is not auditable.

## Reviewer Decision

Primary selected design:

```text
Team workspace shell
  + route-ready Goal and Task documents
  + AgentMember workbench as durable teammate surface
  + controlled graph/Kanban focus layer
  + docs/evidence/warnings/decision context
```

Remaining weaknesses:

- current snapshot types do not yet expose every Vision field or docs link
  needed by the final UI;
- the first implementation may need missing-read-model callouts while backend
  fields catch up;
- graph/Kanban rendering should be staged after the shell and document
  surfaces, or it may dominate the UI too early;
- mobile must be designed as tabs/focus modes, not as stacked desktop panels.

Borrowed ideas:

- from document-first layout: Goal and Task pages must remain audit documents;
- from graph/control-plane layout: controlled graph focus is valuable for
  blockers, dependencies, and distance-to-vision;
- from operations-console layout: queues, warnings, and decisions need dense
  scanning and safe actions.

Killed alternatives:

- old Dashboard stack of summary metrics, Kanban, detail cards, and RawViews;
- any shadcn or generic card-grid dependency that dictates the product shape;
- chat-first Member UI without Evidence/Decision semantics;
- graph-first AgentTeam UI;
- implementation before this document, page specs, and acceptance plan exist.

Loop status: stop for design planning, continue into implementation only after
this document is registered and validated. If implementation reveals missing
read-model fields, create follow-up tasks instead of faking state.

## Visual System

- Base: quiet operational UI, dense but readable.
- Palette: neutral surfaces, strong status colors, restrained accent color.
- Do not use gradient/orb decoration, oversized hero treatment, or marketing
  cards.
- Use stable panel dimensions, clear tab rails, and scroll containment.
- Text must wrap inside technical IDs/refs without resizing panels.
- Status must use text plus color, not color alone.
- Visual impact comes from live state, timelines, queues, and topology, not
  decorative illustration.

## Read-Model Needs

The first rebuild should add selectors for:

- active Vision/Goal context and goal collection grouping;
- standing Team roster independent of task references;
- role groups and role gaps;
- member chronological activity timeline;
- goal-scoped activity stream;
- task protocol completeness;
- evidence/review/decision readiness;
- graph and Kanban projections from the same object set;
- docs links by active object;
- warnings with affected object and repair action metadata.

If the snapshot lacks a required field, the UI must show a missing-read-model
gap and the task must create a follow-up instead of faking the state.

## Implementation Phases

1. New app shell and route-ready layout; debug drawer only.
2. Team workspace and AgentMember workbench.
3. Goal and Task document pages.
4. Graph/Kanban synchronized relationship layer.
5. Docs context, evidence/review/decision queue, warnings/repair queue.
6. Browser/web-quality acceptance and critic review.

Do not merge a code PR that only reskins the old Dashboard. Each phase must
add a functional workflow surface and screenshot evidence.

## Acceptance

Before implementation acceptance:

- `npx pnpm@9.15.4 check` passes.
- Desktop, tablet, and mobile screenshots are attached.
- Console has no runtime errors or React warnings.
- `document.documentElement.scrollWidth <= document.documentElement.clientWidth`
  passes for mobile and tablet.
- Debug drawer is closed by default.
- Selecting a Team, Member, Goal, and Task changes meaningful content.
- Member workbench shows status, queue, runtime health, activity, inbox/outbox,
  sessions, prompt/skills, and actions.
- Goal/Task documents show protocol proof, not only status cards.
