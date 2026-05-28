# Agent Workbench Shell Hard Layout Spec v1

```text
spec_id: agent-workbench-shell-v1
spec_path: docs/dashboard/hard-layout-specs/agent-workbench-shell-v1.md
route_or_surface:
  default / Team workspace shell plus route-ready Vision, Team, Member, Goal,
  Task, Graph/Kanban, Docs, Warnings, Decision, and Debug surfaces
selected_design_refs:
  - docs/dashboard/layout-decisions.md
  - docs/dashboard/frontend-design.md
  - Designer A: collaboration-first Team workspace candidate
  - Designer B: Goal/Task document-first candidate
  - Designer C: control-plane hybrid candidate
reviewer_decision_ref:
  docs/dashboard/layout-decisions.md#decision-2026-05-28-hard-layout-spec-shell-v1
reviewer_decision:
  continue for implementation planning; frontend code still requires browser
  screenshots, implementation Questioner/Critic comparison, and acceptance
```

## Reviewer Synthesis

Selected direction: control-plane hybrid with Team workspace first.

The main shell follows Designer C: Team workspace is the default operating
surface, Graph/Kanban is a constrained relationship layer, and Goal/Task
documents remain audit/proof surfaces. This is the closest fit for a
multi-agent collaboration workbench and avoids graph-first or metrics-first
drift.

Borrowed from Designer A:

- first viewport collaboration density: standing Team, active Vision/Goal,
  role groups, member status, queue counts, activity, decisions, warnings, and
  selected Member inspector;
- full AgentMember workbench: identity, current task/proposal, queue,
  inbox/outbox, runtime health, timeline, prompt/skills, and send-message
  controls;
- explicit read-model selectors before components.

Borrowed from Designer B:

- Goal/Task proof-order document structure;
- Vision overview as a real goal collection and distance-to-vision surface,
  not only a shell strip;
- Task document puts `Message(kind=task)` assignment proof before report,
  evidence, proposal, review, or decision.

Killed candidates:

- Goal/Task document-first as the default shell: too likely to become a static
  audit document manager and weaken realtime AgentTeam observability.
- Graph/control-plane-first as a full shell: too likely to hide assignment,
  evidence, review, and decision proof behind topology visuals.
- Team chat or activity-only surface: too likely to look collaborative while
  burying Goal/Task proof chain.

## Desktop Wireframe

Viewport: `1440x1000`.

```text
52px top bar
948px body grid:
  72px app rail
  264px team rail
  minmax(620px, 1fr) workspace, normally 744px at 1440 viewport
  360px inspector, collapsible to 48px

workspace rows:
  88px VisionGoalStrip
  44px SurfaceTabs
  1fr active surface
  160px DecisionWarningQueue on default Team surface

debug drawer:
  closed by default
  bottom overlay when opened
  never affects primary grid height
```

Fixed dimensions:

- top bar: `52px` height;
- app rail: `72px` width;
- team rail: `264px` width;
- inspector: `360px` width, collapse affordance keeps `48px`;
- Vision/Goal strip: `88px` height;
- surface tabs/action row: `44px` height;
- decision/warning queue: `160px` height on the default Team route only.

First viewport content on `/` and `/teams/:teamId`:

- top bar shows store/API source, live/offline state, selected Vision/Goal,
  search/command affordance, and debug toggle;
- app rail shows Vision, Team, Work, Members, Docs, Warnings, Decisions, Debug;
- team rail shows standing Team, role groups, role gaps, member status, queue
  count, current task, stale/retired markers, and last event age;
- workspace shows pinned Vision/Goal strip, Team activity mapped to canonical
  objects, work lanes, decision queue, and warning queue;
- inspector defaults to selected AgentMember and offers tabs for Member, Task,
  Docs, Evidence, Warnings, and Decision;
- Graph/Kanban is reachable from Work tabs but cannot exceed 35% of the default
  Team first-viewport area.

Scroll containers:

- `body` uses fixed shell height and no horizontal page scroll;
- team rail scrolls internally;
- workspace active surface scrolls below the sticky Vision/Goal strip and tabs;
- inspector tab body scrolls internally;
- graph focus owns pan/zoom inside its region only;
- debug drawer scrolls only when explicitly opened.

## Tablet Wireframe

Viewport: `900x1180`.

```text
52px top bar
body:
  64px app rail
  536px workspace
  300px inspector

team rail:
  collapsed to a 320px drawer
  opened from Team rail control or Team tab

debug drawer:
  closed by default
  overlays from bottom
```

First viewport content:

- Vision/Goal strip remains visible at the top of the workspace;
- Team activity, Work tabs, and decision/warning summary remain in the primary
  workspace column;
- selected Member or Warning appears in the inspector column;
- Team roster is reachable through the drawer without permanently consuming
  width;
- Graph defaults to Kanban/list view and uses a segmented Graph/Kanban toggle.

Collapsed regions:

- team rail collapses to drawer;
- inspector may collapse below `820px` width or when route focus requires more
  document width;
- debug remains closed.

## Mobile Wireframe

Viewport: `390x844`.

```text
44px top bar
72px compact VisionGoalStrip
52px tabs:
  Team | Work | Member | Warnings | Docs | Debug
676px active tab scroll area
```

Default tab: `Team`.

Tab order:

1. Team
2. Work
3. Member
4. Warnings
5. Docs
6. Debug

First viewport content:

- top bar shows source/live state and debug affordance;
- compact Vision/Goal strip shows selected Vision, selected Goal, goal status,
  and critical warning count;
- Team tab shows current team, active Goal, critical warnings, running/blocked
  members, queue summary, and decision actions;
- Work tab shows selected Goal/Task proof chain and Kanban/list first;
- Member tab shows selected member identity, action row, current work, queue,
  timeline, runtime, inbox/outbox, prompt/skills;
- Warnings tab shows global queue and affected object navigation;
- Docs tab shows related docs links and owners;
- Debug tab is explicit and never the default.

Hidden or deferred regions:

- app rail becomes the tab bar;
- team rail becomes Team tab content;
- inspector becomes Member, Warnings, Docs, or Debug tabs;
- graph canvas is deferred to focus mode with list fallback.

## First Viewport By Route

`/` and `/teams/:teamId`:

- default operating surface;
- standing Team, role groups, member status, active Vision/Goal, Team activity,
  global decision queue, warning queue, and selected Member inspector;
- no raw JSON, snapshot textarea, metric wall, or graph-first hero.

`/visions/:visionId`:

- Vision title, final acceptance, selected scenario/pilot;
- goal collection grouped as proposed, active, blocked, complete, and
  archived/rejected;
- distance-to-vision and next-round proposals above the fold;
- evaluation gaps, related docs, and warnings in inspector.

`/goals/:goalId`:

- objective, success criteria, owner, completion state;
- GoalDesign gate, team design summary, role gaps, and branch/target refs;
- compact live team strip with Lead, Worker, Critic, Observer status, queue,
  and last event;
- Graph/Kanban block visible, Kanban selected by default;
- Evidence/Proposal/Review/Decision strip visible in first viewport.

`/tasks/:taskId`:

- objective and acceptance criteria;
- assignment `Message(kind=task)` proof before delivery, report, evidence,
  proposal, review, or decision;
- assignee/runtime strip, delivery state, report state;
- owned paths, worktree, branch, PR/check refs near proposal/review.

`/members/:memberId`:

- identity, role, team, prompt refs, skill refs, permissions;
- current task/proposal, queue, inbox/outbox;
- runtime health split by process, endpoint/socket, protocol, delivery;
- chronological timeline of messages, sessions, events, reports, evidence, and
  proposals;
- send message, deliver, retry, reconcile, and close actions with disabled
  reasons when unavailable.

`/docs`:

- source-linked docs index with owner, status, lifecycle, and related-object
  reason;
- active Vision/Goal/Task/Member docs context;
- no copied docs body as canonical product truth.

`/debug`:

- raw snapshot, import/export, copied CLI/API refs, and raw object lists;
- explicit debug route only;
- never default and never hidden inside primary Workbench content.

## Core Module Placement

| Module | Placement |
| --- | --- |
| Vision overview | Compact strip in shell; full Vision route owns goal collection, distance-to-vision, next-round proposals, and GoalEvaluation links. |
| Team workspace | Default route and primary shell surface: role groups, member status, queues, activity, decisions, warnings. |
| AgentMember workbench | Inspector default plus `/members/:memberId`; full identity, current work, timeline, inbox/outbox, runtime health, prompt/skills, safe actions. |
| Goal document | Work route/tab: objective, GoalDesign, team design, branch refs, Graph/Kanban, Evidence/Review/Decision, GoalEvaluation, next-round plan. |
| Task document | Proof-order route/tab: assignment, delivery, report, evidence, proposal, review, decision, Git refs, owned paths, warnings. |
| Graph/Kanban | Kanban/list default; graph focus mode synchronized to the same selected object. |
| Docs context | Inspector Docs tab and `/docs` route; source links and related-object reasons only. |
| Evidence/Review/Decision | Global queue plus object-local four-lane strip; review evidence visually distinct from Leader Decision. |
| Warnings/repair | Global queue plus local callouts; each warning has affected object, consequence, navigation, disabled reason, and repair metadata. |
| Debug | Top-bar drawer and `/debug` route; raw state secondary and closed by default. |

## Graph/Kanban Contract

- Kanban/list is the default operational view for Goal and Task execution.
- Graph explains dependencies, blockers, follow-ups, graph-change proposals,
  distance-to-vision, and generated next goals.
- Graph and Kanban share one selected-object key. Selecting a graph node and
  selecting a card/lane item open the same document/inspector context.
- On default Team route, graph cannot dominate the first viewport and cannot
  exceed 35% of workspace first-viewport area.
- Tablet uses segmented Graph/Kanban tabs.
- Mobile opens graph as a secondary focus mode with list fallback.
- Topology changes are proposals/decisions; frontend never mutates graph state
  locally.

## Docs Mounting Contract

- Docs are mounted context, not copied source of truth.
- Object pages show related docs, owner, lifecycle, status, path, and reason.
- Missing docs context is a knowledge-routing warning, not silent absence.
- Doc follow-up actions create canonical Task/Proposal only through harness
  API/CLI support.

## Debug Secondary Contract

- Raw JSON, snapshot paste, file import, export, and raw object lists are
  available only in collapsed debug drawer or `/debug`.
- Debug is closed by default in every browser acceptance screenshot except the
  explicit Debug screenshot.
- Paste/file snapshots stop live polling and show offline-source labeling.
- Debug state cannot be used as proof that the Workbench primary workflow is
  understandable.

## State Matrix

| State | Required UI | Stop condition |
| --- | --- | --- |
| Empty | Explain missing canonical object and next safe action. | Blank panel or fake placeholder data. |
| Loading | Skeletons preserve final region widths and tab height. | Layout shift that changes shell geometry. |
| Loaded | Canonical object refs visible in Team, Member, Goal/Task, evidence, and decisions. | Status-only cards or metrics-only summaries. |
| Warning | Local callout near affected proof gap plus global queue entry. | Toast-only warning or warning detached from object. |
| Error | Failed API/source shown with retry path and last good snapshot label when available. | Silent fallback or pretending stale state is live. |
| Offline snapshot | Source label visible and live polling disabled. | Offline data looks live. |
| Missing read model | Explicit gap, affected component, and follow-up path. | Fake Vision, GoalEvaluation, Decision, or runtime state. |
| Large graph | Collapse/search/list fallback; graph focus isolated. | Unreadable canvas or page scroll trap. |

## Component Inventory

Shell components:

- `WorkbenchShell`
- `TopBar`
- `AppRail`
- `TeamRail`
- `WorkspaceFrame`
- `VisionGoalStrip`
- `SurfaceTabs`
- `InspectorTabs`
- `DebugDrawer`
- `MobileTabs`

Core components:

- `RoleGroupRoster`
- `MemberStatusRow`
- `TeamActivityStream`
- `DecisionQueue`
- `WarningQueue`
- `AgentMemberWorkbench`
- `MemberTimeline`
- `RuntimeHealthStack`
- `SendMessageComposer`
- `GoalDocument`
- `TaskDocument`
- `ProofChain`
- `GraphKanbanSwitcher`
- `KanbanLanes`
- `GraphFocusCanvas`
- `AcceptanceStrip`
- `DocsContextPanel`
- `SafeActionBar`
- `StateBanner`

Read-model selectors:

- `activeVisionContext(snapshot, selectedGoalId)`
- `goalCollection(snapshot)`
- `teamWorkspace(snapshot, teamId, goalId?)`
- `memberWorkbench(snapshot, memberId)`
- `memberTimeline(snapshot, memberId)`
- `goalDocument(snapshot, goalId)`
- `taskDocument(snapshot, taskId)`
- `graphKanbanModel(snapshot, scope)`
- `decisionQueue(snapshot, scope)`
- `docsContext(snapshot, objectRef)`
- `warningsByObject(snapshot)`

## Data Density And Text Wrapping

- body text: `13px` to `14px`;
- compact labels: `11px` to `12px`;
- section headers inside work surfaces: `16px` to `18px`;
- no viewport-scaled typography;
- member rows: fixed `56px`;
- activity rows: `72px` to `96px`;
- warning rows: `72px`;
- Kanban cards: fixed minimum width with maximum three visible body lines;
- evidence/proposal lists: show top 3 to 5 items with in-region expand control;
- ids, paths, branch names, PR refs, evidence refs use `overflow-wrap:
  anywhere`;
- status always uses text plus color/icon, never color alone.

## Forbidden Primary Surfaces

Implementation fails review when any of these drive the first viewport:

- raw JSON;
- snapshot textarea;
- `SummaryGrid` as primary shell;
- `RawViews` as primary shell;
- old summary/Kanban/detail/raw-view composition;
- metrics dashboard;
- card dump of every object;
- graph-first Team route;
- chat-only member/team view;
- document-only shell with no live AgentMember state;
- frontend-only toggles that simulate canonical decisions or goal completion.

## Screenshot Acceptance

Required viewports:

- desktop: `1440x1000`;
- tablet: `900x1180`;
- mobile: `390x844`.

Required screenshots:

- default Team workspace;
- selected Member detail/workbench;
- Goal document;
- Task document;
- Graph/Kanban surface;
- Docs context;
- Warnings surface;
- Debug closed default state;
- Debug route or opened drawer only as a separate explicit debug screenshot.

Every viewport must prove:

- first viewport is a Workbench, not a stacked report, card dump, metrics wall,
  or raw debug surface;
- debug is closed by default;
- no page-level horizontal overflow:
  `document.documentElement.scrollWidth <= document.documentElement.clientWidth`;
- selecting Team, Member, Goal/Task, Graph/Kanban, Docs, Warnings, and Debug
  changes meaningful content;
- selected member shows runtime health, queue, current task, activity,
  inbox/outbox, sessions, prompt/skills, and send-message control;
- Goal and Task show assignment, evidence, review, and decision proof instead
  of status-only cards;
- graph and Kanban selection synchronize to the same object context;
- console has no runtime errors or React key/layout warnings;
- live mode loads `/v1/snapshot`.

## Implementation Questioner Checklist

The implementation Questioner/Critic must compare browser screenshots and DOM
behavior against this spec, not against developer intent.

Required checks:

- default first viewport matches Team workspace first;
- Graph/Kanban does not dominate the default Team route;
- Goal/Task documents preserve proof order;
- AgentMember workbench shows realtime and runtime surfaces;
- Docs are source-linked context;
- Warnings are local and global;
- Debug is secondary;
- scroll ownership matches this spec;
- tablet and mobile do not collapse into stacked desktop panels;
- no forbidden primary surface appears;
- screenshot matrix covers all required routes/surfaces.

## Reviewer Stop Conditions

Stop implementation and return to design when:

- a changed route or core module lacks hard spec coverage;
- the browser first viewport reads as dashboard, report, card dump, or debug
  tool;
- Goal/Task proof chain is hidden behind Team activity;
- Team/Member realtime state is hidden behind static documents;
- Graph becomes the shell instead of a constrained relationship layer;
- mobile uses stacked desktop panels instead of tabs/focus modes;
- scroll ownership causes page-level horizontal overflow;
- raw snapshot input or raw JSON appears in the primary viewport;
- missing read-model fields are faked instead of surfaced as gaps.
