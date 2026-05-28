# Agent Workbench Layout Decisions

This document records Workbench UI/UX layout alternatives, rejected variants,
and selected design direction. Core principles stay in
[design-principles.md](design-principles.md). Concrete route composition,
page cards, visual placement, safe actions, and responsive behavior stay in
[frontend-design.md](frontend-design.md).

## Decision 2026-05-28: Team Workspace Shell

Selected direction:

```text
Team workspace shell
  + Goal/Task document surfaces
  + controlled graph/Kanban relationship layer
```

Rationale:

- Multi-Agent Harness needs to feel like a persistent team control plane, not a
  one-shot job runner or JSON report.
- AgentMembers should read as durable teammates with status, queue, inbox,
  outbox, runtime health, current task, and activity history.
- Goals and Tasks are durable work records, closer to collaborative documents
  than simple cards.
- Graph is valuable for Vision, Goal, TaskGraph, blockers, follow-ups, and
  distance-to-vision, but it should not become the default AgentTeam UI.

## Top-Level Alternatives

| Variant | Score | Decision | Why |
| --- | ---: | --- | --- |
| Team Workspace First | 84/100 | Selected as shell | Best supports persistent AgentTeam and AgentMember-as-person mental model. |
| Goal/Task Document First | 78/100 | Rejected as shell, absorbed into details | Strong audit model, but too likely to feel like a document manager instead of a live team control plane. |
| Control Plane + Graph Hybrid | 66/100 | Rejected as shell, absorbed as relationship layer | Useful for dependencies and distance-to-vision, but too risky as graph-first default and expensive on mobile/accessibility. |

Useful parts kept:

- from Team Workspace First: left Team spaces, member roster, collaboration
  workbench, Member inspector/workbench;
- from Goal/Task Document First: Goal and Task document surfaces, object
  mentions, evidence/decision blocks, mounted docs;
- from Control Plane + Graph Hybrid: controlled graph/Kanban tabs, focus mode,
  graph node selection synchronized with cards and document sections.

Killed directions:

- AgentTeam graph as default UI: killed because it confuses team identity with
  task dependency topology and provider child threads.
- Pure document shell: killed because it weakens realtime team observability.
- Graph-first control plane: killed because it can hide assignment, evidence,
  review, and decision proof behind topology visuals.
- Task-card-only model: killed because it cannot prove assignment, report,
  evidence, proposal, review, and decision order.

## Decision 2026-05-28: Frontend Design Workflow Gate

This decision records the frontend skill workflow that produced the current
design baseline.

Independent review record:

```text
designer_prompt:
  propose top-level and page-level Workbench UI/UX options after restating
  Vision, selected Goal, and final acceptance
designer_output_ref:
  layout-variants.md Page-Level Option Loop; chat-side subagent Lagrange
questioner_prompt:
  independently challenge workflow proof, mobile/accessibility, read-model/API
  feasibility, and no-raw-debug-first behavior
questioner_input_ref:
  current Workbench docs and raw design artifacts, not a Lead-preferred answer
questioner_output_ref:
  this decision record; chat-side subagent Locke
decision_record_ref:
  layout-decisions.md and frontend-design.md
unresolved_questions:
  backend/read-model fields for Vision, GoalEvaluation, graph-change
  proposals, docs links, and safe repair actions remain implementation tasks
next_loop_request:
  no new top-level shell loop; continue only if implementation exposes missing
  read-model/API blockers or browser evidence contradicts the design
```

The subagents were temporary design inputs, not canonical harness execution.
Their outputs are durable only through this documentation and harness evidence.

Loop status: stop for top-level design. The selected shell has enough signal:
Team workspace first, Goal/Task document surfaces, controlled graph/Kanban
relationship views, mounted docs, warnings/repair, and debug secondary.

## Decision 2026-05-28: Reject First Implementation Attempt

The first implementation attempt in branch
`task/agent-workbench-implementation` is rejected as a layout-quality failure.
It is not a PR candidate for the goal branch.

What happened:

- build and TypeScript checks could pass while the browser-visible surface still
  read as a long stacked report rather than a coherent Agent Workbench;
- the implementation moved from accepted direction directly into component/CSS
  work without a hard desktop/tablet/mobile layout implementation spec;
- the Team workspace, activity stream, graph/Kanban, inspector, and debug
  surfaces were technically present but not constrained enough by first-viewport
  placement, region dimensions, or scroll boundaries;
- browser review showed that "Team workspace first" was still too vague as an
  implementation instruction.

Rejected because:

- it did not meet the product bar for a Feishu-like multi-agent collaboration
  workbench;
- it risked repeating the old dashboard failure mode: lots of state visible,
  but weak hierarchy and poor operational usability;
- it proved that passing build checks is not sufficient frontend acceptance;
- it exposed a missing workflow gate in the frontend skill.

Decision:

```text
branch_or_worktree:
  /Users/hhh0x/multi-agent-harness-agent-workbench-implementation
branch:
  task/agent-workbench-implementation
commit_or_diff:
  base 11fae5e761c734763c1a8292a42101c00c3a8148 plus uncommitted dirty
  implementation diff; no implementation commit was accepted
screenshot_refs:
  local rejected review screenshots captured before rejection:
  dashboard-shell-desktop-1440.png, dashboard-shell-tablet-900.png
failed_acceptance:
  no hard layout implementation spec; weak first-viewport hierarchy; Team,
  activity, graph/Kanban, inspector, and debug placement not constrained by
  region dimensions or scroll ownership
spec_gap_that_allowed_it:
  accepted direction said "Team workspace first" without implementation-ready
  desktop/tablet/mobile wireframes, module-level option decisions, or
  screenshot stop conditions
code_disposition:
  keep isolated in task/agent-workbench-implementation until replaced or
  explicitly discarded; do not merge it into the goal branch
required_next_loop:
  update frontend skill and docs, then create hard layout implementation specs
  before renewed UI coding
reviewer_stop_condition:
  no new implementation PR may proceed without desktop/tablet/mobile wireframes,
  region dimensions, first-viewport content, scroll boundaries, rejected module
  options, and browser screenshot acceptance criteria
```

Skill changes required by this failure:

- core modules require multiple Designer candidates, not a single option;
- Questioner/Critic must challenge both design artifacts and browser screenshots;
- Reviewer chooses, synthesizes, kills, or requests another round for each core
  module;
- failed implementation attempts must be recorded as rejected layouts when they
  reveal that the design spec was too vague.

## Page-Level Decisions

These decisions close the required page-level option loop. The rejected options
are recorded in [layout-variants.md](layout-variants.md); the selected page
cards are implemented in [frontend-design.md](frontend-design.md).

| Surface | Selected option | Rejected options | Borrowed ideas | Loop status |
| --- | --- | --- | --- | --- |
| Vision overview | Vision ladder | scorecard as primary, timeline as primary | pilot acceptance chips, evaluation-to-next-goal links | stop |
| Team workspace | Collaboration workspace | static operations console, goal-scoped team panel | dense queues, pinned goal health strip | stop |
| AgentMember workbench | Chronological activity stream | tab-only inspector, runtime-first console | timeline filters, health summary | stop |
| Goal document | Audit document | cockpit as primary, learning-only document | health strip, distance-to-vision closeout | stop |
| Task document | Proof-order document | execution workbench as primary, PR/evidence-only audit | runtime strip, owned-path/check block | stop |
| Graph/Kanban | Kanban default plus graph focus | split view as first implementation, graph canvas default | graph/card sync, later minimap/search/collapse | stop for design, stage implementation |
| Docs context | Inspector docs panel plus docs route | external links only, full docs embedded inline | compact related-doc blocks | stop |
| Evidence/Review/Decision | Four-lane acceptance strip | timeline-only chain, packet-only summary | chronological lane ordering, packet summary | stop |
| Warnings/repair | Global queue plus local callouts | inspector-only warnings, passive checklist | checklist header summary | stop |
| Debug | Collapsed drawer plus `/debug` route | raw route primary, modal-only console | shareable debug route | stop |
| Mobile/accessibility | Tabbed mobile workbench | stacked desktop document, drawer-only detail | drawer for secondary context | stop |

Remaining weaknesses accepted into implementation:

- graph visualization should start with Kanban/list-first behavior and add
  canvas focus only after node/edge/read-model contracts are clear;
- Evidence/Review/Decision needs object-local and global queue wiring so
  acceptance proof is not buried;
- warning repair actions depend on backend support and must show disabled
  reasons instead of simulating success;
- mobile graph and timeline views need browser proof for no overflow, focus
  order, labels, and text-based status.

Future module decision records must include:

```text
selected:
killed_because:
borrowed:
remaining_weakness:
hard_spec_implication:
screenshot_acceptance:
reviewer_stop_continue_blocked:
```

At minimum, Team workspace, AgentMember workbench, Goal/Task documents,
Graph/Kanban, and mobile/responsive placement need this record before renewed
implementation.

## Decision 2026-05-28: Hard Layout Spec Shell v1

Reviewer decision: `continue` for implementation planning, not automatic UI
acceptance. Formal spec:
[hard-layout-specs/agent-workbench-shell-v1.md](hard-layout-specs/agent-workbench-shell-v1.md).

Selected main candidate: Designer C control-plane hybrid. Borrowed from
Designer A: collaboration-first first viewport, persistent Team roster,
selected Member inspector, complete AgentMember runtime/timeline/inbox/outbox,
and read-model selector list. Borrowed from Designer B: Goal/Task proof-order
documents, real Vision goal collection and distance-to-vision, and assignment
`Message(kind=task)` proof before report/evidence/decision.

Killed candidates: Goal/Task document-first default shell, graph-first shell,
and Team chat/activity-only shell.

Remaining weaknesses carried into implementation acceptance: inspector density
at `1440x1000` and `900x1180`, mobile six-tab overflow/readability, Graph focus
scroll traps, and missing read-model fields being faked instead of surfaced as
explicit gaps.

## Module Decisions

### Team Rail And Team Detail

Selected: Feishu/Slack-like three-layer collaboration space.

```text
global icon rail | Team list | Team workspace | inspector
```

Desktop placement:

- left: Team spaces and team list;
- center: selected Team workspace with active Vision/Goal, current work, member
  activity, and decision queue;
- right: selected Member/Task/Docs/Warn inspector.

Tablet placement:

- Team list collapses into a drawer;
- center workspace remains primary;
- inspector becomes a drawer or tabbed panel.

Mobile placement:

- `Team` tab first shows current Team, active Goal, running/blocked members, and
  critical warnings.

Rejected variants:

- top Team switcher: killed because persistent team presence is too weak;
- Team card grid: killed because it feels like a project list, not a
  collaboration space.

Constraints:

- Team detail must show active Vision, selected Goal, goal health, role groups,
  role gaps, stale/retired members, queue, current task, and last event.
- Team workspace cannot become chat-only; every message-like row must map back
  to `Message`, `Task`, `Evidence`, `Proposal`, `Decision`, or warning state.

### AgentMember Workbench

Selected: Member workbench in right inspector plus optional `/members/:id` full
page.

Required content:

- identity, role, team, prompt refs, skill refs, permissions;
- status, queue, current task, current proposal;
- chronological activity stream merging inbox, outbox, delivery updates,
  provider sessions, AgentEvents, reports, evidence, and proposals;
- runtime health split by process, endpoint/socket, protocol, and delivery;
- send message, deliver, retry, reconcile, close actions.

Rejected variants:

- member row expansion only: killed because realtime state is not visible
  enough;
- chat-only member page: killed because it weakens canonical
  Message/Evidence/Decision semantics.

### Goal Document

Selected: Goal collaborative document as the Goal detail model.

Required sections:

```text
objective / success criteria
GoalDesign state
AgentTeam design and role gaps
goal branch and production target
Goal graph/Kanban block
Task section
Evidence / Review / Decision
GoalEvaluation
distance-to-vision
NextRoundPlan
related docs
```

Rejected variants:

- Goal control console only: killed because Goal becomes a task board;
- Goal graph first: killed because graph is analysis, not the default work
  surface.

Constraints:

- Goal complete cannot be inferred from all tasks being done.
- Goal complete requires Leader Decision and GoalEvaluation, or explicit
  blocked/killed/replanned closeout.

### Task Document

Selected: Task audit document.

Required order:

```text
objective
acceptance criteria
assignment proof
assignee / runtime
messages and reports
evidence
proposal / review
decision
workspace / branch / PR / owned paths
warnings
```

Rejected variants:

- task drawer only: killed for complex tasks because it is not audit-friendly;
- task card only: killed because it cannot prove harness execution.

Constraints:

- Missing `Message(kind=task)` before report/decision must be visibly
  incomplete.
- Branch, PR, worktree, and owned paths must be visually near proposal/review.

### Goal/Task Graph And Kanban

Selected: desktop split with focus mode.

Placement:

- desktop: compact controlled graph plus Kanban/work lanes in the workbench;
- tablet: segmented Graph/Kanban tabs;
- mobile: Work defaults to document/Kanban; Graph opens as a secondary focus
  view.

Rejected variants:

- graph focus as default: killed because it hides operational lanes;
- pure Kanban: killed because dependencies, blockers, follow-ups, and
  distance-to-vision are lost.

Constraints:

- Graph and Kanban must be synchronized projections of the same read model.
- AgentTeam does not use graph as default.
- Clicking a graph node should synchronize selected card and document section.

### Dashboard-Mounted Docs

Selected: Docs context panel plus selected inline blocks.

Placement:

- desktop: Docs tab in inspector;
- tablet: drawer;
- mobile: Docs tab;
- Goal/Task docs: inline links or compact context blocks for key docs only.

Rejected variants:

- docs-only route as primary: killed because context is too weak;
- full docs embedded in Goal/Task: killed because pages become long and source
  of truth becomes ambiguous.

Constraints:

- Docs panel mounts canonical docs; it does not copy facts into a new source of
  truth.
- Related docs should link back to Goal, Task, Evidence, Decision, or ADR where
  possible.

### Warnings And Decision Queue

Selected: global queue plus local warnings.

Placement:

- desktop: Team workspace decision queue plus object-local warnings;
- tablet/mobile: Warnings tab;
- object pages: local warning callouts near affected section.

Rejected variants:

- right-panel-only warnings: killed because users miss object-local causes;
- toast-first warnings: killed because toasts are not audit surfaces.

Constraints:

- each warning needs affected object, severity, why it matters, navigation, and
  safe repair action when available;
- UI warnings remain advisory until promoted to schema, CLI/API, review gate, or
  CI.

### Mobile Shell

Selected:

```text
Team | Work | Member | Warnings | Docs
```

Constraints:

- compact Vision/Goal strip stays visible;
- Work defaults to document/Kanban, not graph;
- Member tab preserves current selected member activity;
- Docs tab provides context, not replacement for operations;
- no horizontal overflow.

## Implementation Guidance

Do not implement the whole Workbench rewrite in one task. Split into page-level
or module-level work:

1. shell and Team workspace;
2. Member workbench and activity timeline read model;
3. Goal document and Task document surfaces;
4. graph/Kanban relationship layer;
5. mounted docs context;
6. warnings/decision queue;
7. mobile/tabbed responsive shell;
8. browser and web-quality acceptance.

## Decision 2026-05-28: Frontend Skill Audit Hardening

Reviewer: independent skill-quality reviewer using the `skill-creator` guidance.

Findings accepted:

- `SKILL.md` duplicated reference material and weakened progressive disclosure.
- Required source docs were too broad and encouraged loading every doc by
  default.
- The Designer/Questioner loop did not state a concrete execution contract for
  harness dogfooding, independence, evidence, and waiver cases.
- Acceptance gates listed outcomes but did not define viewport targets,
  artifact names, overflow proof, or non-waivable failures.
- The skill metadata default prompt did not mention multi-candidate review loops
  or browser/web-quality evidence.

Fixes applied:

- narrowed `SKILL.md` into an entry workflow, doctrine, failure modes, artifact
  placement, and acceptance pointers;
- moved detailed loop mechanics to
  `.agents/skills/harness-frontend-product-design/references/subagent-design-loop.md`;
- moved page-level option and decision mechanics to
  `.agents/skills/harness-frontend-product-design/references/page-design-workflow.md`;
- expanded browser and web-quality gates in
  `.agents/skills/harness-frontend-product-design/references/acceptance-gates.md`;
- regenerated `agents/openai.yaml` with a default prompt that names
  multi-candidate review loops and browser/web-quality validation.

Loop decision:

- status: stop for this skill-hardening round;
- reason: the accepted reviewer findings were directly addressed, and remaining
  improvements are implementation of the Workbench itself rather than skill
  structure uncertainty;
- next Designer request: none for the skill; use the skill to drive the next
  frontend implementation design/verification loop.
