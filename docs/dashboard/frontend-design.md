# Agent Workbench Frontend Design

This document is the canonical implementation design for the Agent Workbench
frontend. It replaces the previous split between `ui-ux-layout.md` and
`frontend-rebuild-design.md`.

Split reason: this file intentionally exceeds the normal 500-line split target
because it is the single implementation handoff for the full Workbench redesign.
Splitting page cards, visual placement, safe actions, read-model needs, and
browser acceptance into separate canonical docs previously created conflicting
sources of truth. If this file grows beyond the current route/page/action
contract, split only stable child surfaces after the parent design remains
unambiguous.

Product purpose stays in [../dashboard.md](../dashboard.md). Rejected layout
candidates stay in [layout-variants.md](layout-variants.md). Accepted decisions
stay in [layout-decisions.md](layout-decisions.md). React boundaries stay in
[frontend-architecture.md](frontend-architecture.md). Read-model contracts stay
in [read-model.md](read-model.md). Browser and web-quality gates stay in
[acceptance.md](acceptance.md).

## Vision And Acceptance

Multi-Agent Harness is a goal-task-multi-agent development system. The Agent
Workbench must let an operator reconstruct and control this workflow without
raw JSON or hidden chat context:

```text
Vision -> Goal collection -> GoalDesign -> AgentTeam -> TaskGraph
  -> Message assignment -> AgentMember runtime -> Evidence -> Proposal
  -> Review -> Decision -> GoalEvaluation -> distance-to-vision
  -> next Goal / follow-up Task / GoalCase
```

The current frontend goal is to rebuild the surface as a multi-agent
collaboration workbench. The first accepted product
direction is:

```text
Team workspace shell
  + Goal and Task document surfaces
  + controlled Goal/Task Graph and Kanban views
  + AgentMember workbench
  + mounted docs, warnings, evidence, review, and decision context
  + collapsed debug drawer
```

Implementation cannot start from the old component stack. The old
summary/Kanban/detail/raw-view layout is deprecated as a product UI. Preserve
only stable TypeScript types, API helpers, and pure read-model logic that still
serve the design below.

## Hard Layout Gate

The accepted direction is not enough by itself to start coding. A renewed
implementation must first add hard layout implementation specs for every core
surface that will be built in the slice.

Each spec must live in `docs/dashboard/hard-layout-specs/<slice>.md` unless the
Reviewer records a different path. The implementation PR must cite the
`spec_id`, selected design refs, Reviewer `stop | continue | blocked` decision,
and screenshot matrix.

Each spec must include:

```text
spec_id:
route_or_surface:
selected_design_refs:
reviewer_decision_ref:
desktop_wireframe:
  viewport:
  columns_or_regions:
  fixed_dimensions:
  first_viewport_content:
  scroll_containers:
tablet_wireframe:
  breakpoint:
  collapsed_regions:
  first_viewport_content:
mobile_wireframe:
  breakpoint:
  tab_order:
  first_viewport_content:
  hidden_or_deferred_regions:
state_matrix:
  empty:
  loading:
  loaded:
  warning:
  error:
component_inventory:
data_density_limits:
text_wrapping_rules:
forbidden_primary_surfaces:
screenshot_acceptance:
reviewer_stop_conditions:
```

Implementation must stop and return to design when browser screenshots show a
stacked report, metrics dashboard, card dump, raw snapshot tool, incoherent
first viewport, or page-level overflow.

The first implementation attempt on `task/agent-workbench-implementation` is a
rejected example: checks passed, but the browser-visible layout did not meet the
Workbench bar. See [layout-decisions.md](layout-decisions.md) for the rejection
record. Future implementation should replace that attempt from the hard specs,
not continue styling it into shape.

## Documentation Cleanup Decision

Deleted or superseded:

| Old doc | Decision | Why |
| --- | --- | --- |
| `ui-ux-layout.md` | Superseded by this document | It mixed global shell, page details, read-model needs, and implementation notes while missing formal page cards. |
| `frontend-rebuild-design.md` | Superseded by this document | It was an implementation-ready sketch after a failed attempt, but overlapped with layout docs and was not the single source for page-level design. |

Kept:

| Doc | Role |
| --- | --- |
| `design-principles.md` | Durable doctrine and failure modes. |
| `layout-variants.md` | Three candidate layouts and Questioner critique. |
| `layout-decisions.md` | Accepted direction, killed alternatives, module decisions, and loop status. |
| `frontend-architecture.md` | React/Vite module boundaries. |
| `read-model.md` | Selector, projection, warning, and safe-action contracts. |
| `acceptance.md` | Browser, overflow, console, accessibility, performance, and web-quality gates. |
| `runbook.md` | Local run and build commands. |

## Design Workflow Provenance

This document is the final design draft, not the raw design loop. The workflow
record is split intentionally:

| Stage | Record |
| --- | --- |
| Product Vision and Workbench purpose | [../dashboard.md](../dashboard.md) |
| Frontend doctrine and failure modes | [design-principles.md](design-principles.md) |
| Top-level layout candidates and Questioner critique | [layout-variants.md](layout-variants.md) |
| Selected layout, rejected alternatives, page-level decisions, and loop status | [layout-decisions.md](layout-decisions.md) |
| Final implementation handoff | This document |

The design loop used two temporary chat-side subagents:

- Designer: proposed three top-level candidates and page-level options after
  restating the Vision, selected Goal, and final acceptance standard.
- Questioner: independently challenged workflow proof, mobile/accessibility,
  read-model/API feasibility, and raw-debug-first risks.

They do not count as canonical harness AgentMember execution. Their useful
outputs are preserved in `layout-variants.md`, `layout-decisions.md`, this
document, and task evidence. Implementation must treat those docs as the source
of truth, not hidden chat context.

## Route Map

The first implementation may remain a single React route internally, but the
component model must be route-ready.

| Route | Purpose |
| --- | --- |
| `/` | Team workspace for the selected Vision/Goal. |
| `/visions/:visionId` | Vision overview, goal collection, distance-to-vision, next goals. |
| `/teams/:teamId` | Persistent AgentTeam workspace, role groups, queues, decision queue. |
| `/members/:memberId` | Focused AgentMember workbench. |
| `/goals/:goalId` | Goal document with design, team, graph/Kanban, evidence, review, decision, evaluation. |
| `/tasks/:taskId` | Task document with assignment proof, evidence, proposal, review, decision, Git refs. |
| `/docs` | Mounted docs context linked to active objects. |
| `/debug` | Raw snapshot, import/export, and low-level object views. |

## Global Shell

Desktop layout:

```text
top bar
  app rail | team rail | team workspace | inspector
debug drawer collapsed
```

Top bar:

- active store and API URL;
- generated time and live/polling state;
- gateway health or last load error;
- search/command affordance when available;
- explicit debug drawer button.

App rail:

- Vision, Teams, Goals, Work, Members, Docs, Decisions, Warnings, Debug;
- icons may be used, but every icon needs a label or tooltip;
- Debug is secondary and never the default selected surface.

Team rail:

- persistent teams and role groups;
- member status, current task, queue count, last event age;
- role gaps and closed/retired members remain visible when relevant.

Workspace:

- active Vision/Goal strip;
- Team activity tied to canonical objects;
- Goal/Task document tabs;
- Graph/Kanban relationship tab;
- global decision and warning queue.

Inspector:

- selected Member, Task, Docs, Warnings, Evidence, or Decision;
- tabbed instead of stacked;
- internal scroll only.

Debug drawer:

- raw snapshot import/export, raw object lists, and copied CLI/API refs;
- hidden by default;
- never pushes the primary workspace below the first viewport.

## Core Page Cards

### Vision Overview

Route: `/visions/:visionId`.

Why it exists: Vision is a long-lived target with a goal collection and
learning loop, not a single active goal.

Primary user question: are completed and active goals moving the product closer
to the target state?

Canonical objects: Vision context, Goals, GoalEvaluation, NextRoundPlan,
GoalCase, autonomous proposals, follow-up tasks.

Workflow proof:

- goals grouped as proposed, active, blocked, complete, archived/rejected;
- complete goals show Decision and GoalEvaluation state;
- next-round proposals link back to source evidence and prior evaluation;
- missing vision context is shown as a workflow gap.

Primary actions: select goal, open Goal document, open next-round proposal,
request evidence, create or accept follow-up goal when backend support exists.

Safe-action contracts: no frontend-only goal creation. Goal or follow-up
creation must route through harness CLI/API and return canonical Goal/Task or
Decision records.

Read-model needs: `visionOverview`, goal grouping, distance-to-vision summary,
next-round proposals, goal evaluation links, docs context.

Desktop: goal collection rail, central progress/path, right evaluator and next
round panel.

Tablet: progress and next-round panel stack under the goal collection.

Mobile: tabs for Goals, Progress, Next, Warnings.

Browser acceptance: completed and not-complete goals are visually distinct; no
goal appears complete without Decision and GoalEvaluation or an explicit
blocked/killed/replanned closeout.

### Team Workspace

Route: `/teams/:teamId`, also default `/`.

Why it exists: the operator should start from a persistent AgentTeam rather
than a task dump.

Primary user question: who is active, who is blocked, and what needs Lead
action now?

Canonical objects: AgentTeam, AgentMember, Goal, Task, Message, Evidence,
Proposal, Decision, ProviderSession, warnings.

Workflow proof:

- full team roster is visible even when no task currently references a member;
- every activity row maps to Message, Task, Evidence, Proposal, Decision,
  session, or advisory warning;
- active Vision/Goal strip stays above activity;
- decision queue is visible without opening raw objects.

Primary actions: select member, select task, send message, deliver queued work,
request review, open Goal/Task document, open docs context.

Safe-action contracts: send-message creates `Message`; deliver/retry/reconcile
routes through agent API; request review creates a canonical queued review
message or task action; close member must be explicit and destructive.

Read-model needs: `teamWorkspace`, full roster, role groups, role gaps, member
queue counts, goal-scoped activity, decision queue, warning queue.

Desktop: team rail, central workspace, inspector.

Tablet: team rail collapses; workspace and inspector become two columns.

Mobile: Team tab shows current team, active Goal, role groups, running/blocked
members, and critical warnings.

Browser acceptance: an idle member remains visible as a durable teammate; raw
snapshot controls are not in the primary viewport.

### AgentMember Workbench

Route: `/members/:memberId`, also inspector tab.

Why it exists: each AgentMember is a durable teammate with runtime continuity,
not a disposable provider turn.

Primary user question: what is this member doing, what did it receive, and what
evidence supports its claims?

Canonical objects: AgentMember, inbox/outbox Message, MessageDelivery,
ProviderSession, ProviderChildThread, AgentEvent, Evidence, Proposal, Task.

Workflow proof:

- chronological activity merges inbox, outbox, delivery, sessions, events,
  reports, evidence refs, and proposals;
- runtime health is split into process, endpoint, protocol, and delivery;
- current task/proposal, prompt refs, skill refs, and permissions are visible;
- provider child threads remain under the parent member unless promoted.

Primary actions: send message, deliver, retry delivery, reconcile session,
request report, close member.

Safe-action contracts: runtime and delivery actions must call safe API
endpoints; close requires confirmation and must not be the normal successful
end of a task; failed actions return updated snapshot and visible error state.

Read-model needs: `memberWorkbench`, `memberTimeline`, inbox/outbox,
sessions/child threads, runtime layers, current task/proposal, prompt/skill
refs, action-disabled reasons.

Desktop: inspector summary plus optional full page; full page uses left current
work, center timeline, right runtime/actions.

Tablet: member workbench becomes a drawer or second column.

Mobile: Member tab shows identity, current work, action row, timeline, runtime,
and prompt/skills sections.

Browser acceptance: selecting a member changes content and shows status,
queue, runtime health, activity, inbox/outbox, sessions, prompt/skills, and
send-message UI.

### Goal Document

Route: `/goals/:goalId`.

Why it exists: Goal is a durable outcome with design, acceptance, and learning,
not a task list.

Primary user question: why does this goal exist, how was it designed, and is it
accepted or still incomplete?

Canonical objects: Goal, GoalDesign evidence, AgentTeam, Tasks, Messages,
Evidence, Proposals, Reviews, Decisions, GoalEvaluation, GoalCase,
NextRoundPlan, Git refs.

Workflow proof:

- objective and success criteria are above operational detail;
- GoalDesign and assignment gates are visible before implementation state;
- Goal branch and task PR/worktree refs are near proposal/review;
- completion requires Decision and GoalEvaluation or explicit blocked/killed
  closeout;
- distance-to-vision and next proposed goal are shown when available.

Primary actions: open task, open graph/Kanban, request review, record decision
when backend supports it, open related docs, create follow-up task/goal through
canonical API.

Safe-action contracts: no local goal status toggles. Decisions and follow-ups
must create canonical Decision, Task, Goal, Evidence, or Proposal records.

Read-model needs: `goalDocument`, goal learning status, GoalDesign evidence,
team design, tasks, decisions, evaluation evidence, Git/PR refs, related docs.

Desktop: document header, design block, graph/Kanban block, task section,
evidence/review/decision, evaluation/next-round side panel.

Tablet: document with sticky section tabs and inspector drawer.

Mobile: Work tab opens document sections; graph is a secondary focus view.

Browser acceptance: all-tasks-done never renders as goal complete unless
decision/evaluation proof exists.

### Task Document

Route: `/tasks/:taskId`.

Why it exists: Task proves assignment, work, evidence, review, and decision.

Primary user question: did this task follow the harness protocol?

Canonical objects: Task, assignment Message, report Message, AgentMember,
ProviderSession, Evidence, Proposal, Review evidence, Decision, branch/worktree
/ PR refs, warnings.

Workflow proof:

- assignment proof appears before report and evidence;
- missing `Message(kind=task)` is marked incomplete;
- reports and evidence are linked to messages/proposals/sessions;
- proposal, review, decision, owned paths, and PR refs appear together;
- object-local warnings sit next to the affected section.

Primary actions: send/assign task, deliver to assignee, request review, open PR
or evidence refs, open member, open related docs.

Safe-action contracts: assignment and review requests must create Messages;
deliver/retry/reconcile use safe agent endpoints; PR/status links are source
refs, not frontend truth.

Read-model needs: `taskDocument`, assignment proof, reports, sessions,
evidence refs, proposals, review evidence, decisions, owned-path warnings,
related docs.

Desktop: proof-order document in workspace, selected assignee in inspector.

Tablet: task document primary, inspector drawer for member/evidence.

Mobile: Work tab shows objective, proof chain, current state, warnings, and
actions before long evidence lists.

Browser acceptance: task in review/done without report or decision shows a
visible protocol gap.

### Graph And Kanban

Route: `/goals/:goalId/graph`, `/goals/:goalId/board`, future `/graph`.

Why it exists: graph explains relationships; Kanban explains execution state.

Primary user question: what depends on what, and what can move next?

Canonical objects: Goals, Tasks, dependencies, blockers, graph-change
proposals, follow-ups, evaluations, decisions.

Workflow proof:

- graph and Kanban are synchronized projections of the same read model;
- node selection opens the same object as selecting a card/lane item;
- blockers, split/killed tasks, follow-ups, and generated goals are semantic
  edges, not decorative lines;
- graph-change proposals appear in the decision queue.

Primary actions: select node/card, open object document, collapse layer, search
node, focus graph, accept/reject graph-change proposal when API supports it.

Safe-action contracts: graph changes are proposals/decisions; the frontend does
not mutate topology locally.

Read-model needs: `graphKanbanModel`, node types, edge types, lane groups,
sync key, selected object id, empty/large graph state, graph proposal links.

Desktop: compact graph next to or above Kanban lanes, with focus mode for large
graphs.

Tablet: segmented Graph/Kanban tabs.

Mobile: Kanban/document first; graph opens as secondary focus mode with list
fallback.

Browser acceptance: graph and Kanban both reachable; selecting one updates the
same object context.

### Docs Context

Route: `/docs`, also inspector tab.

Why it exists: operators need canonical docs beside active objects without
turning the Dashboard into a copied documentation source.

Primary user question: which product or workflow doc explains this object or
decision?

Canonical objects: docs registry entries, active Vision/Goal/Task/Team/Member,
Evidence, Decision, ADR links.

Workflow proof:

- docs are source-linked context;
- object pages show related docs and reasons, not pasted doc truth;
- missing docs context appears as a knowledge-routing gap.

Primary actions: open doc link, filter docs by active object, copy path,
request doc follow-up task when a canonical doc is missing.

Safe-action contracts: doc follow-ups create Task/Proposal through harness API
when supported; doc links never mutate product state.

Read-model needs: `docsContext`, registry lookup, related docs by object type,
doc owner/status/lifecycle, broken/missing link warnings.

Desktop: inspector Docs tab and `/docs` route.

Tablet: docs drawer.

Mobile: Docs tab.

Browser acceptance: active Goal/Task/Member can show related docs without
promoting docs to primary raw state.

### Evidence, Review, And Decision

Route: workspace queue plus object-local sections.

Why it exists: acceptance must be auditable.

Primary user question: what claim is being accepted, rejected, waived, split,
or blocked, and what evidence supports it?

Canonical objects: Evidence, Proposal, Review findings, Decision, check output,
screenshots, PR refs, GoalEvaluation.

Workflow proof:

- proposal shows changed paths, evidence refs, checks, and PR/worktree refs;
- review evidence is distinct from Leader Decision;
- decision without evidence refs is visibly incomplete unless it is an
  explicit waiver with owner and follow-up;
- task-local acceptance contributes to goal close but does not replace it.

Primary actions: open evidence, open proposal, request review, record or
inspect decision, create follow-up task.

Safe-action contracts: decisions and follow-ups must create canonical records;
review request must send a message or task action; evidence is source-linked.

Read-model needs: `decisionQueue`, proposal/evidence joins, review evidence,
decision completeness, waiver/follow-up links.

Desktop: global decision queue in Team workspace; object-local lanes in Goal
and Task documents.

Tablet: decision queue tab/drawer.

Mobile: Evidence tab combines proposal, review, decision, and evaluation.

Browser acceptance: accepted task/goal shows evidence and review/decision, or
an explicit waiver warning.

### Warnings And Repair Queue

Route: `/warnings`, also inspector tab and local callouts.

Why it exists: broken workflow links should become visible and repairable
before they turn into hidden failures.

Primary user question: which object is risky, why does it matter, and what can
I safely do next?

Canonical objects: advisory warnings, affected Goal/Task/Member/Message/
Session/Proposal/Decision/Evidence, safe action metadata.

Workflow proof:

- warning includes affected object, severity, cause, consequence, and action;
- warnings appear both globally and near the affected object;
- UI-only warnings remain advisory until promoted to schema/CLI/review/CI.

Primary actions: navigate to affected object, send message, deliver/retry,
request review, create follow-up, copy command, open docs.

Safe-action contracts: each repair action names its endpoint/CLI path,
preconditions, disabled reasons, success state, and failure state.

Read-model needs: `warningsByObject`, severity groups, affected refs, repair
action metadata, promotion status.

Desktop: global queue plus inspector details.

Tablet: Warnings drawer.

Mobile: Warnings tab.

Browser acceptance: warning selection navigates to or opens the affected
object, and unavailable repairs explain why they are disabled.

### Debug Drawer

Route: `/debug`, also top-bar drawer.

Why it exists: raw snapshot tooling is useful for audit/debug but is not the
operator experience.

Primary user question: what raw state was loaded, and can I import/export it?

Canonical objects: raw DashboardSnapshot sections, pasted/file snapshot,
live snapshot source, copied CLI/API refs.

Workflow proof:

- raw state is secondary;
- import/export does not replace live canonical state;
- debug state never hides missing workflow proof in primary pages.

Primary actions: paste snapshot, upload snapshot, export snapshot, view raw
sections, copy command refs, clear local snapshot.

Safe-action contracts: paste/file snapshots stop live polling and are labeled
offline; debug actions do not mutate harness state.

Read-model needs: snapshot metadata, source mode, raw section counts, parse
errors.

Desktop: collapsed drawer opened from top bar.

Tablet: drawer.

Mobile: Debug tab behind explicit selection.

Browser acceptance: debug is closed by default and raw JSON is not visible in
the first viewport.

## Visual System

The Workbench should feel like a dense collaboration control plane, closer to
Feishu/Slack for teams and Notion/Linear for work documents, but with harness
object proof as the source of truth.

Rules:

- no shadcn dependency;
- no generic marketing hero, decorative gradient/orb background, or card-grid
  landing page;
- use custom React/TypeScript/CSS and lucide icons only when icons clarify
  actions;
- stable columns, fixed toolbars, bounded scroll areas, and responsive tabs;
- text wraps technical ids and refs with `overflow-wrap: anywhere`;
- status is expressed with text plus color, never color alone;
- all buttons keep readable labels without viewport-scaled typography.

State language:

| State | UI treatment |
| --- | --- |
| complete / accepted | strong success text, evidence/decision link required |
| active / running | live status pulse plus last event age |
| queued / assigned | pending tone plus delivery state |
| blocked / failed | high-contrast warning, object-local repair path |
| review | review tone, proposal/evidence refs visible |
| waived | warning tone, rationale, owner, follow-up |
| missing read-model | neutral warning, follow-up task path |

## Safe Action Contracts

| Action | UI source | Canonical effect | Preconditions | Failure state |
| --- | --- | --- | --- | --- |
| Send message | Member, Team, Task | Creates `Message` | sender/recipient available, member accepts delivery | show error and keep draft |
| Deliver | Member, Message queue | Updates delivery/session via agent delivery path | queued message, runtime accepts delivery | warning with session/message refs |
| Retry delivery | Member, Warning | Attempts safe retry/reconciliation | failed or retryable delivery | repair remains disabled with reason |
| Reconcile session | Runtime, Warning | Updates provider session/message delivery state | unresolved provider session | unresolved warning remains |
| Close member | Member actions | Closes AgentMember runtime/member state | explicit confirmation, no normal task success path | destructive failure warning |
| Request review | Task, Proposal | Sends review request or creates task action | reviewer exists, proposal/evidence ready | task-local warning |
| Record decision | Decision queue | Creates Decision | evidence refs or waiver rationale/follow-up | incomplete decision warning |
| Create follow-up | Goal/Warning/Evaluation | Creates Task or Goal | owner, scope, acceptance known | proposal remains pending |

The frontend may hide or disable an action, but it must not simulate canonical
success. Success requires an updated snapshot containing the resulting object.

## Read-Model Contract

The first implementation should define selectors before wiring components:

| Selector | Purpose |
| --- | --- |
| `activeVisionContext(snapshot, selectedGoalId)` | Vision summary, final acceptance, missing-context warnings. |
| `goalCollection(snapshot)` | Proposed, active, blocked, complete, archived/rejected goals. |
| `goalDocument(snapshot, goalId)` | Goal, GoalDesign evidence, tasks, team, learning status, decisions, evaluation, docs. |
| `teamWorkspace(snapshot, teamId, goalId?)` | Full roster, role groups, queues, activity, warnings, decision queue. |
| `memberWorkbench(snapshot, memberId)` | Member identity, current work, runtime, prompt/skills, actions. |
| `memberTimeline(snapshot, memberId)` | Chronological messages, delivery, sessions, events, reports, evidence, proposals. |
| `taskDocument(snapshot, taskId)` | Assignment proof, reports, evidence, sessions, proposal, review, decision, Git refs. |
| `graphKanbanModel(snapshot, scope)` | Synchronized nodes, edges, lanes, selected object, graph-change proposals. |
| `decisionQueue(snapshot, scope)` | Proposals, missing reviews, warnings, waivers, pending decisions. |
| `docsContext(snapshot, objectRef)` | Related docs, registry metadata, missing-doc warnings. |
| `warningsByObject(snapshot)` | Advisory warnings grouped by affected canonical object. |

Missing fields should render explicit missing-read-model callouts and create
follow-up tasks when they block the design. Do not fake Vision, GoalEvaluation,
or Decision state in the frontend.

## Mobile And Accessibility

Mobile shell:

```text
top bar
vision + goal strip
tabs: Team | Work | Member | Warnings | Docs | Debug
```

Rules:

- no page-level horizontal overflow;
- graph falls back to Kanban/list plus optional focus mode;
- selected member and warning details are reachable through tabs, not stacked
  after a long board;
- destructive actions require confirmation;
- focus order follows top bar, tabs, primary content, inspector/drawer;
- panels use landmarks or labels so screen readers can identify Team,
  Workspace, Inspector, and Debug regions;
- status changes have text labels and do not rely on animation;
- touch targets use stable dimensions.

## Implementation Sequence

1. Read-model foundation: latest-row projection, team roster, goal collection,
   member timeline, task proof chain, warnings by object.
2. App shell: top bar, app rail, team rail, workspace, inspector, collapsed
   debug drawer.
3. Team workspace: active Vision/Goal strip, role groups, activity, decision
   and warning queues.
4. AgentMember workbench: identity, current work, timeline, runtime layers,
   inbox/outbox, sessions, prompt/skills, safe actions.
5. Goal and Task documents: proof-order sections, Git/PR refs, evidence,
   review, decision, evaluation, related docs.
6. Graph/Kanban layer: synchronized scope, lanes, selected object, focus mode.
7. Docs context and warnings repair: registry-backed docs, object-local and
   global warnings, safe repair metadata.
8. Browser and web-quality acceptance: screenshots, console, overflow,
   accessibility, performance, best practices, PR evidence.

Each implementation task should own disjoint files or a separate worktree and
must attach browser evidence before acceptance.

## Browser Acceptance

Implementation acceptance requires:

- `npx pnpm@9.15.4 check` passes;
- desktop screenshot at `1440x1000`;
- tablet screenshot at `900x1180`;
- mobile screenshot at `390x844`;
- clean browser console with no runtime errors or React key/layout warnings;
- `document.documentElement.scrollWidth <= document.documentElement.clientWidth`
  for tablet and mobile;
- debug drawer closed by default;
- selecting Team, Member, Goal, Task, Docs, Warnings, and Debug changes
  meaningful content;
- live mode loads `/v1/snapshot`;
- Goal/Task views show protocol proof instead of status-only cards;
- AgentMember workbench shows realtime state, activity, inbox/outbox, runtime,
  sessions, prompt/skills, and send-message UI;
- graph and Kanban/lane projections are both reachable where claimed;
- web-quality audit records accessibility, performance, best practices, SEO
  waiver if needed, and Core Web Vitals.
