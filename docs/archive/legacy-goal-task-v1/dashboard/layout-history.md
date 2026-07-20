# Agent Workbench Layout History

<!-- doc-size-exempt -->
<!--
Size-exempt reason: this file intentionally consolidates the former
layout-variants.md (candidate critique, scoring rubric, page-option loops) and
layout-decisions.md (chronological decision ledger) into one routing target for
layout history and rejected-implementation records. Keeping the candidate
critique next to the decisions it produced is the whole point of the merge, so
the combined length is expected and is preserved deliberately rather than
re-split.
-->

This is the consolidated layout history for the Agent Workbench. It merges the
former `layout-variants.md` (the three-candidate Designer/Questioner critique,
scoring rubric, and page-option loops) with the former `layout-decisions.md`
(the chronological selected/killed/deprecated decision ledger), plus the one-line
outcome of the PR #6 rejected implementation.

This file is the routing target for future rejected-implementation records:
when a browser-visible attempt is rejected, append a dated decision entry to the
[decision ledger](#decision-ledger) instead of creating a separate
rejected-implementations directory. Durable design principles stay in
[design-principles.md](design-principles.md); concrete route composition, page
cards, and page-local `## Layout Contract` sections live under [pages/](pages/);
accepted stack and module boundaries live in
[frontend-architecture.md](../../../dashboard/frontend-architecture.md) and ADR
[0016](../../../decisions/0016-tailwind-shadcn-adoption.md).

## Product Context

Star Harness is a coordination layer for persistent Agent Teams. The
Workbench must show that this workflow really happened:

```text
Vision -> Goal collection -> GoalDesign -> AgentTeam -> TaskGraph
  -> Message -> AgentMember work -> Evidence -> Proposal/Review
  -> Decision -> GoalEvaluation -> distance-to-vision -> next Goal
```

The frontend design goal is to reduce distance-to-vision by making that chain
visible and operable without raw JSON or hidden chat context. The Workbench
should feel like a multi-agent collaboration control plane: agents are durable
teammates, goals and tasks are auditable work documents, and graph views explain
relationships without becoming the default mental model for everything.

## Workflow Record

The design round used two temporary chat-side subagents as design inputs. Their
outputs are recorded here because they are not canonical harness execution by
themselves.

```text
designer: Lagrange
designer_task: propose top-level and page-level Workbench UI/UX options
questioner: Locke
questioner_task: independently challenge workflow proof, mobile/accessibility,
  read-model feasibility, and raw-debug-first risks
decision_owner: Lead
durable_record: this file, frontend-design.md, and task evidence in the harness
  store
```

Both subagents first restated the product Vision, the selected Workbench Goal,
and the final acceptance standard. The Questioner agreed that no further
top-level shell loop was useful, but required explicit page-level option records
before implementation.

## Decision Rubric

The Questioner and Decision Agent use this rubric:

| Criterion | Weight |
| --- | ---: |
| Workflow proof | 25% |
| Team/Member collaboration model | 20% |
| Goal/Task document model | 15% |
| Graph/Kanban balance | 15% |
| Realtime control and observability | 10% |
| Implementation complexity | 10% |
| Mobile/accessibility quality | 5% |

The Decision Agent may synthesize a hybrid, but it must record which parts were
accepted, which alternatives were rejected, and why the selected direction serves
the product Vision.

## Candidate 1: Team Workspace First

This direction is closest to Feishu or Slack. The main mental model is a
persistent team workspace where each AgentMember behaves like a teammate with a
workbench, inbox, outbox, queue, current task, runtime status, and message
history.

```text
+--------------------------------------------------------------------+
| top bar: store, active vision, live status, command/search/docs     |
+-------------+--------------------+--------------------+------------+
| Team rail   | Team members       | Team workspace     | Inspector  |
|             |                    |                    |            |
| teams       | Lead               | active goal strip  | Member     |
| goals       | Observer           | team message flow  | Task       |
| docs        | Workers            | current queues     | Evidence   |
| warnings    | Critics            | goal/task panels   | Warnings   |
+-------------+--------------------+--------------------+------------+
```

Primary routes:

- `/teams`: team list, standing team health, role gaps, stale members.
- `/teams/:teamId`: team workspace with members, messages, queues, active goal
  strip, and decision queue.
- `/members/:memberId`: one member workbench with inbox, outbox, runtime,
  prompt refs, skill refs, current task, provider sessions, and safe actions.
- `/goals/:goalId`: goal document and graph/Kanban tabs reached from the team.
- `/tasks/:taskId`: task document reached from messages, queues, or goal views.
- `/docs`: mounted project docs with links back to active Vision/Goal/Task.

Strengths:

- best expresses persistent AgentTeams and AgentMembers as durable teammates;
- supports direct send-message, inbox/outbox, idle/busy/blocked state, and
  realtime activity naturally;
- makes team continuity visible across multiple goals instead of turning agents
  into disposable job runners;
- good fit for daily operation because the first question is often "who is
  working and what needs action?"

Risks:

- can degrade into chat UI and hide GoalDesign, TaskGraph, Evidence, and
  Decision proof;
- a team-first shell may underplay Vision and distance-to-vision unless the
  active goal strip is always visible;
- docs can become a side link instead of first-class context;
- implementation needs a good member activity stream projection to avoid
  showing static lists as fake realtime state.

Required safeguards:

- active Vision and selected Goal stay above the team message flow;
- each team message links to canonical Message, Task, Evidence, Proposal, or
  Decision objects;
- task and goal graph/Kanban tabs are one click away from the team workspace;
- member workbench merges inbox, outbox, delivery, provider session, event,
  report, evidence, and proposal refs into one chronological stream;
- raw debug is hidden in a drawer.

## Candidate 2: Goal/Task Document Workspace First

This direction treats Goals and Tasks as collaborative work documents. The
operator starts from a goal document that contains the objective, success
criteria, GoalDesign, team plan, branch policy, TaskGraph, task lanes,
evidence, review, decision, evaluation, and linked docs.

```text
+--------------------------------------------------------------------+
| top bar: store, vision, goal status, docs/search/actions            |
+-------------+---------------------------------------+--------------+
| Goal rail   | Goal / Task document                  | Inspector    |
|             |                                       |              |
| visions     | objective and success criteria         | Team roster  |
| goals       | GoalDesign / Task brief                | Member live  |
| task docs   | graph + Kanban tabs                    | Evidence     |
| docs        | messages, evidence, decisions          | Warnings     |
+-------------+---------------------------------------+--------------+
```

Primary routes:

- `/visions/:visionId`: goal collection grouped by proposed, active, blocked,
  complete, archived/rejected, and next-round proposals.
- `/goals/:goalId`: goal document with GoalDesign, team design, graph/Kanban,
  evidence/review/decision, Git/PR lane, and GoalEvaluation.
- `/tasks/:taskId`: task document with assignment proof, acceptance criteria,
  messages, evidence, proposal, review, decision, branch/worktree/PR refs.
- `/docs`: project docs mounted as context and backlinked to active objects.

Strengths:

- strongest audit trail for why work exists and whether it was accepted;
- naturally supports Dashboard-mounted docs and goal/task narrative context;
- keeps "Goal is not task list" visible because GoalDesign, decisions, and
  GoalEvaluation live in the same surface;
- easier to implement incrementally from the current goal/task panels.

Risks:

- may feel like a static documentation app instead of a realtime control plane;
- AgentTeam can become a side panel instead of the primary collaboration model;
- member inbox/outbox and runtime health may be underexposed;
- operators may have to jump between documents to answer "who needs action now?"

Required safeguards:

- every goal document includes a persistent team strip and member status;
- every task document shows the assigned member, delivery state, and report
  message before evidence and decisions;
- a global team/member workspace remains available from any document;
- document comments or activity are canonical messages, not frontend-only notes;
- mobile navigation must keep Work, Member, Warnings, Evidence, and Docs within
  one tap.

## Candidate 3: Control Plane + Graph Hybrid

This direction starts from the system topology. The main workbench is a
controlled canvas for Vision, Goals, TaskGraphs, blockers, evidence, proposals,
and decisions, with Kanban lanes and inspectors attached to selected nodes.

```text
+--------------------------------------------------------------------+
| top bar: store, live status, graph scope, filters, safe actions     |
+-------------+---------------------------------------+--------------+
| Scope rail  | Controlled graph/canvas               | Inspector    |
|             |                                       |              |
| vision      | Vision -> Goals                       | selected     |
| goals       | selected Goal -> TaskGraph             | node doc     |
| tasks       | blockers / follow-ups / evidence       | member/team  |
| teams       | collapse/expand layers                 | warnings     |
+-------------+---------------------------------------+--------------+
| bottom lane: synchronized Goal/Task Kanban and decision queues       |
+--------------------------------------------------------------------+
```

Primary routes:

- `/graph`: global controlled canvas with scope filters and collapsed layers.
- `/visions/:visionId/graph`: vision goal graph with next-round links.
- `/goals/:goalId/graph`: selected goal graph and task graph.
- `/goals/:goalId/board`: goal and task Kanban projections from the same read
  model.
- `/members/:memberId`: focused member workbench outside the canvas.

Strengths:

- best for dependency reasoning, blockers, follow-up chains, GoalEvaluation to
  next-goal causality, and distance-to-vision analysis;
- makes graph-change proposals and split/killed/follow-up tasks visible;
- useful for large goals where a list or document hides dependency structure.

Risks:

- graph can become decorative topology instead of operational proof;
- poor default for AgentTeam because team identity is not a dependency graph;
- high implementation complexity and mobile risk;
- could push the Feishu-like collaboration model and docs context too far from
  the first viewport.

Required safeguards:

- graph is a controlled canvas with semantic node types, automatic layout,
  collapse/expand, minimap, search, and side inspector;
- AgentTeam remains a roster/workspace surface, not a graph by default;
- Kanban/lane projection stays synchronized with graph state;
- selected node inspector shows document-like details, evidence, messages, and
  decisions;
- mobile defaults to Work, Member, Warnings, Evidence, and Debug tabs rather
  than a large canvas.

## Questioner Critique Framework

The Questioner must be independent and objective. It does not serve the Designer
and does not reward visual polish unless the design improves workflow proof,
operation speed, and acceptance.

For every candidate, the Questioner must ask:

- Did the design preserve Vision as a goal collection with distance-to-vision,
  or did it collapse Vision into the active Goal?
- Did the design preserve Goal as an auditable outcome with GoalDesign,
  AgentTeam, branch policy, TaskGraph, Evidence, Decision, and GoalEvaluation,
  or did it turn Goal into a task board?
- Did Task show assignment proof through `Message(kind=task)`, delivery state,
  member report, evidence refs, proposal/review, and Leader decision?
- Did AgentTeam remain a standing organization with role gaps and member
  continuity, or did it become a one-off worker list?
- Did AgentMember feel live through inbox/outbox, queue, runtime health,
  provider session, activity stream, and send-message controls?
- Did graph and Kanban remain synchronized projections from the same read
  model?
- Did project docs become first-class context rather than an external link?
- Did safe actions update canonical harness objects instead of frontend-only
  state?
- Can the layout be verified with desktop, tablet, and mobile browser
  screenshots, clean console, no horizontal overflow, and web-quality checks?

### Candidate 1 Risks

P0:

- chat/team activity could hide the canonical workflow proof chain;
- Vision and GoalEvaluation may be underweighted unless pinned in the shell;
- member realtime claims could be fake if the read model cannot merge activity.

P1:

- goal/task documents might become secondary routes instead of primary objects;
- docs context may be too far from the team workspace;
- right inspector can become overloaded with member, task, warning, and evidence
  details.

P2:

- familiar collaboration layout may tempt decorative avatars and messages that
  do not map to harness objects.

Decision gate:

- pass only if active Vision/Goal, task graph/Kanban, evidence/decision, and
  member activity stream are visible without leaving the workspace.

### Candidate 2 Risks

P0:

- can become a static docs UI and fail to show persistent AgentMember liveness;
- team/member collaboration may be reduced to metadata panels;
- operator efficiency may suffer when action queues are spread across docs.

P1:

- task and goal graph/Kanban may feel bolted on if the document is too linear;
- member send-message actions may be hidden behind inspectors;
- cross-goal team continuity may be weak.

P2:

- document layout can grow too long on mobile if evidence, decisions, messages,
  and warnings are stacked.

Decision gate:

- pass only if every document has live team/member controls, action queues, and
  synchronized graph/Kanban views in the first operational viewport.

### Candidate 3 Risks

P0:

- graph may obscure the human/team collaboration model;
- implementation cost can delay basic operator workflows;
- mobile and accessibility risk is highest.

P1:

- AgentTeam graph can confuse team identity with task dependency;
- docs and Goal/Task document details may be hidden behind node selection;
- graph filtering can create inconsistent mental state if Kanban diverges.

P2:

- canvas motion and visual density can look impressive while hiding missing
  evidence, failed delivery, or unreviewed decisions.

Decision gate:

- pass only if graph remains scoped, collapsible, synchronized with Kanban, and
  secondary to Team/Member and Goal/Task operational proof.

## Provisional Synthesis

The product direction is a hybrid:

```text
Team workspace shell
  + Goal/Task document surfaces
  + controlled graph/Kanban tabs
```

Use Candidate 1 as the overall shell because Star Harness is a persistent
team product. Use Candidate 2 for Goal and Task detail because goals/tasks need
auditable document-like surfaces. Use Candidate 3 only for relationship views:
Vision goal graph, selected Goal task graph, blockers, follow-ups, and
distance-to-vision.

## Page-Level Option Loop

The following option loop records the concrete alternatives used to produce the
implementation design. The selected options are recorded in the
[decision ledger](#decision-ledger) below.

### Vision Overview

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Vision ladder | Goal groups by proposed, active, blocked, complete, archived/rejected, plus distance-to-vision panel | Can underplay scenario/pilot acceptance | Clear complete/not-complete goal collection |
| Pilot scorecard | Self-hosting and adapter pilots as columns with acceptance gaps | Can become metrics-first | Compact acceptance chips |
| Learning timeline | Completed goals, evaluations, next-round plans, follow-up goals | Can hide current work | Evaluation-to-next-goal causality |

Recommended: Vision ladder with scorecard chips and learning links.

### Team Workspace

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Collaboration workspace | App rail, team rail, central activity/work queue, right inspector | Can drift into chat | Best persistent AgentTeam mental model |
| Operations console | Dense roster, queues, warnings, decisions in tables | Can feel static | Fast queue scanning |
| Goal-scoped team panel | Team context embedded inside selected goal | Hides standing-team continuity | Goal health strip |

Recommended: Collaboration workspace with dense queue affordances and a pinned
goal health strip.

### AgentMember Workbench

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Chronological activity stream | Merge messages, delivery, sessions, events, reports, evidence, proposals | Requires normalized timeline projection | Best canonical workflow proof |
| Tabbed inspector | Inbox, outbox, sessions, events, evidence, actions | Fragments causality | Useful filters |
| Runtime-first console | Health, sessions, queue, linked work | Can make provider state look canonical | Health summary |

Recommended: Chronological activity stream with filters and a compact runtime
health summary.

### Goal Document

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Audit document | Objective, GoalDesign, team design, graph/Kanban, evidence/review/decision, evaluation | Can become long | Strongest acceptance trail |
| Goal cockpit | Header, health strip, queues, graph/Kanban, decision queue | Can reduce Goal to operations | Action/health strip |
| Learning document | GoalEvaluation, distance-to-vision, next-round plan emphasized | Can underplay execution | Strong vision loop |

Recommended: Audit document with cockpit health strip and learning closeout.

### Task Document

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Proof-order document | Assignment, report, evidence, proposal, review, decision | Can underplay live runtime | Best protocol proof |
| Execution workbench | Status, assignee/runtime, messages, sessions, actions, warnings | Can hide acceptance order | Runtime/action strip |
| PR/evidence audit | Changed paths, owned paths, checks, review, decision | Too narrow for non-code tasks | Owned-path/check block |

Recommended: Proof-order document with runtime strip and PR/evidence block.

### Graph And Kanban

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Split synchronized view | Compact graph beside operational lanes | Crowded desktop, poor on mobile if copied directly | Strong graph/card selection sync |
| Kanban default plus graph focus | Work lanes first, graph opens as focus mode | Dependencies can be missed if focus is hidden | Best mobile/accessibility default |
| Graph canvas default | Controlled canvas with bottom Kanban | Highest mobile/accessibility and decorative-topology risk | Minimap/search/collapse for later scale |

Recommended: Kanban default plus graph focus for the first implementation;
mature desktop may add split synchronized mode. The graph must define semantic
node/edge types, collapse/search behavior, and list fallback before visual canvas
work begins.

### Docs Context

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Inspector docs panel | Object-linked docs and anchors near active work | Panel can be narrow | Strong object context |
| Dedicated docs route | Search and object backlinks | Loses workflow context if primary | Full browsing and search |
| Inline context blocks | Compact related docs in Goal/Task documents | Can duplicate source truth | Good local cues |

Recommended: Inspector docs panel plus dedicated route, with compact inline
related-doc links only.

### Evidence, Review, And Decision

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Four-lane acceptance strip | Evidence, Proposal, Review, Decision lanes near object | Can fragment chronology | Makes missing acceptance links visible |
| Timeline acceptance chain | Event-time chain across evidence, review, decision | Can hide missing categories | Shows order and causality |
| Decision packet card | Summary of evidence, review, rationale, follow-ups | Can hide underlying proof | Fast Lead scanning |

Recommended: Four-lane acceptance strip with chronological ordering inside lanes
and a decision packet summary. The first implementation must keep review evidence
distinct from Leader Decision.

### Warnings And Repair

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Global queue plus local callouts | Warning queue plus object-local affected sections | Global queue can feel detached | Best visibility and repair path |
| Inspector warning browser | Right-panel warning list and detail | Easy to miss object-local causes | Useful detail browsing |
| Workflow health checklist | Per Goal/Task checklist of protocol links | Can become passive | Good header summary |

Recommended: Global queue plus local callouts, borrowing checklist summaries in
Goal/Task headers. Every warning needs severity, affected object, cause,
consequence, repair action, disabled reason, and promotion status.

### Debug Drawer

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Collapsed drawer | Raw snapshot, import/export, raw messages/sessions outside main viewport | Can still crowd layout if oversized | Best no-raw-debug-first default |
| Dedicated `/debug` route | Deep-linkable diagnostics | Separates debug from active context | Shareable raw review |
| Modal console | Temporary debug overlay | Blocks comparison with workbench | Safe for rare actions |

Recommended: Collapsed drawer plus dedicated `/debug` route. Debug must be closed
by default and labeled offline when using pasted/file snapshots.

### Mobile And Accessibility

| Option | Shape | Risk | Useful Part |
| --- | --- | --- | --- |
| Tabbed mobile workbench | Team, Work, Member, Warnings, Docs, Debug tabs | Tab state must be stable | Best no-overflow structure |
| Stacked document | Single scroll with anchored sections | Hides member/warning details far below | Simple document reading |
| Drawer inspector | Work first, details in drawers | Can hide acceptance proof | Good secondary detail pattern |

Recommended: Tabbed mobile workbench with drawer details only for secondary
context. Graph uses Kanban/list fallback; timelines and warning queues require
keyboard focus order, text status, and labeled controls.

## Decision Ledger

This is the chronological ledger of selected, killed, deprecated, and restarted
directions. Newer entries are appended at the bottom.

### Decision 2026-05-28: Team Workspace Shell

Selected direction:

```text
Team workspace shell
  + Goal/Task document surfaces
  + controlled graph/Kanban relationship layer
```

Rationale:

- Star Harness needs to feel like a persistent team control plane, not a
  one-shot job runner or JSON report.
- AgentMembers should read as durable teammates with status, queue, inbox,
  outbox, runtime health, current task, and activity history.
- Goals and Tasks are durable work records, closer to collaborative documents
  than simple cards.
- Graph is valuable for Vision, Goal, TaskGraph, blockers, follow-ups, and
  distance-to-vision, but it should not become the default AgentTeam UI.

Top-level alternatives:

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

### Decision 2026-05-28: Frontend Design Workflow Gate

This decision records the frontend skill workflow that produced the design
baseline.

Independent review record:

```text
designer_prompt:
  propose top-level and page-level Workbench UI/UX options after restating
  Vision, selected Goal, and final acceptance
designer_output_ref:
  Page-Level Option Loop in this file; chat-side subagent Lagrange
questioner_prompt:
  independently challenge workflow proof, mobile/accessibility, read-model/API
  feasibility, and no-raw-debug-first behavior
questioner_input_ref:
  current Workbench docs and raw design artifacts, not a Lead-preferred answer
questioner_output_ref:
  this decision record; chat-side subagent Locke
decision_record_ref:
  this file and frontend-design.md
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

### Decision 2026-05-28: Reject First Implementation Attempt

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

Skill changes required by this failure:

- core modules require multiple Designer candidates, not a single option;
- Questioner/Critic must challenge both design artifacts and browser screenshots;
- Reviewer chooses, synthesizes, kills, or requests another round for each core
  module;
- failed implementation attempts must be recorded as rejected layouts when they
  reveal that the design spec was too vague.

### Page-Level Decisions

These decisions close the required page-level option loop. The rejected options
are recorded in the [Page-Level Option Loop](#page-level-option-loop); the
selected page cards and detailed layout contracts are implemented in
[pages/](pages/).

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

Future module decision records must include:

```text
selected:
killed_because:
borrowed:
remaining_weakness:
layout_contract_implication:
screenshot_acceptance:
reviewer_stop_continue_blocked:
```

### Decision 2026-05-28: Hard Layout Spec Shell v1

Status: deprecated after PR #6. The v1 spec was useful as a first hard-layout
attempt, but it remained too broad and allowed a dashboard/card-dump
implementation to pass mechanical checks.

Selected main candidate: Designer C control-plane hybrid. Borrowed from
Designer A: collaboration-first first viewport, persistent Team roster, selected
Member inspector, complete AgentMember runtime/timeline/inbox/outbox, and
read-model selector list. Borrowed from Designer B: Goal/Task proof-order
documents, real Vision goal collection and distance-to-vision, and assignment
`Message(kind=task)` proof before report/evidence/decision.

Killed candidates: Goal/Task document-first default shell, graph-first shell,
and Team chat/activity-only shell.

Final disposition: do not implement from shell v1. Restart from page specs with
page-local `## Layout Contract` sections and the architecture reset in
[frontend-architecture.md](../../../dashboard/frontend-architecture.md).

### Decision 2026-05-29: Shell v2 Restart Spec

Reviewer decision: `deprecated` as an implementation source. The draft shell v2
spec was retained as historical context only; its useful constraints were moved
into the relevant page-local layout contracts under [pages/](pages/).

The shell v2 restart made the first viewport stricter:

- Team must read as a collaboration workspace, not roster/cards;
- AgentMember must read as a teammate workbench, not an inspector card;
- Goal/Task/Docs/Evidence/Decision must appear connected to workflow context,
  not as disconnected tabs;
- screenshots must be reviewed by first impression before console, data, or
  overflow checks can support acceptance.

### Decision 2026-05-29: Page-Local Layout Contracts

Reviewer decision: `continue` for documentation structure. Current layout
contracts live inside each `docs/dashboard/pages/<page>.md` file under
`## Layout Contract`.

Rationale:

- page meaning and page geometry must stay together so implementers cannot miss
  the workflow proof behind a layout;
- each core page now has detailed desktop, tablet, and mobile ASCII diagrams,
  concrete dimensions, first-viewport content, scroll ownership, and screenshot
  acceptance;
- hard-layout shell specs were too easy to treat as a generic shell and too far
  from the page-specific user questions.

Implementation implication:

- every changed route must link to its page spec and `## Layout Contract`;
- screenshot review compares the browser output to that page document;
- changes to dimensions, breakpoints, scroll ownership, or first-viewport
  placement update the same page document, not a separate hard-layout file.

### Module Decisions

#### Team Rail And Team Detail

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

#### AgentMember Workbench

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

#### Goal Document

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

#### Task Document

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

#### Goal/Task Graph And Kanban

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

#### Dashboard-Mounted Docs

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

#### Warnings And Decision Queue

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

#### Mobile Shell

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

### Implementation Guidance

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

### Decision 2026-05-28: Frontend Skill Audit Hardening

Reviewer: independent skill-quality reviewer using the `skill-creator` guidance.

Findings accepted:

- `SKILL.md` duplicated reference material and weakened progressive disclosure.
- Required source docs were too broad and encouraged loading every doc by
  default.
- The Designer/Questioner loop did not state a concrete execution contract for
  harness dogfooding, independence, evidence, and waiver cases.
- Acceptance gates listed outcomes but did not define viewport targets, artifact
  names, overflow proof, or non-waivable failures.
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

### Decision 2026-05-30: PR #6 Rejected Implementation Outcome

PR #6 ("Agent Workbench shell", branch
`task/agent-workbench-shell-implementation` / GitHub PR #6) was rejected: the
first viewport read as a dense dashboard/card dump rather than a Feishu-like
collaboration workspace, AgentMember appeared as an inspector card instead of a
durable teammate workbench, and the failure was structural (information
architecture), not spacing or color polish; reviewer decision was do-not-merge
and restart from page specs, the architecture decision, and page-local layout
contracts.

### Decision 2026-05-30: Tailwind v4 + shadcn Rebuild Shipped

The Agent Workbench frontend was rebuilt and merged in PR #7 on the
Tailwind CSS v4 + shadcn/ui (Radix) + lucide-react + Geist stack, replacing the
rejected hand-rolled-CSS shell. The accepted stack and module boundary are
recorded in [frontend-architecture.md](../../../dashboard/frontend-architecture.md) and ADR
[0016](../../../decisions/0016-tailwind-shadcn-adoption.md). This consolidated history
is now the routing target for any future rejected-implementation records: append
a dated entry to this ledger rather than creating a separate directory.

### Decision 2026-07-03: Goal Workbench v1 Phase Spine

Workflow evidence: `wfrun-1783013150649-0`; harness evidence:
`evidence-1783013226770-p11384-0`.

Selected direction:

```text
Goal detail as primary workbench
  + phase-first vertical spine
  + inline task/evidence/review/decision inspector
  + unphased/follow-up task separation
  + demoted derived lifecycle summary
```

Why this serves the Vision:

- A selected Goal must prove the harness workflow happened; a filtered task board
  cannot explain GoalDesign, phase gates, assignment messages, evidence,
  proposal/review, Leader decision, and GoalEvaluation in one place.
- `surface=tasks&goal=<id>` is useful as a projection, but it cannot be the
  primary explanation surface for "which Goal is this and what spec is being
  executed?"
- Phase is now the sequential execution and gate model. The UI must center
  `Goal.phases[]` and `Task.phase_id`, not the legacy stage strip.

Rejected or demoted directions:

- Stage-bar-first Goal: killed because `draft -> ... -> verified` is only a
  derived lifecycle projection for phase-driven goals.
- Task-board-first Goal: killed because it hides the Goal spec and closeout
  proof chain.
- Graph-first Goal: deferred because the current failure is orientation and
  proof, not topology; graph remains a phase-local projection.

Borrowed ideas:

- From the Work board design, keep Goal collection as the Work index and keep
  Task detail as an inspector/drawer rather than forcing a full route jump.
- From evidence/review/decision page contracts, keep proof chain gaps visible as
  object-local acceptance state.

Screenshot gate:

- Implementation is not accepted from code checks alone.
- Browser evidence must include actual desktop, tablet, and mobile screenshots,
  console checks, horizontal overflow checks, and at least one phase task detail
  interaction while retaining Goal context.

Current page contract:

- [the archived Goal Workbench spec](pages/goal.md)

### Iteration 2026-07-03: Goal Workbench Polish Pass

Browser evidence:

- before: `.harness/screenshots/goal-workbench-polish/before-tasks-goal-content-desktop.png`
- before: `.harness/screenshots/goal-workbench-polish/before-goal-content-desktop.png`
- before: `.harness/screenshots/goal-workbench-polish/before-goal-content-mobile.png`
- after: `.harness/screenshots/goal-workbench-polish/after-tasks-goal-content-desktop.png`
- after: `.harness/screenshots/goal-workbench-polish/after-goal-content-desktop.png`
- after: `.harness/screenshots/goal-workbench-polish/after-goal-content-mobile.png`

Problems found from screenshots:

- `surface=tasks&goal=<id>` still read as the generic Work board. It did not
  expose the Goal spec or make the path back to the Goal Workbench visually
  primary enough.
- Goal detail opened with a table-like property block. On mobile this made the
  first viewport spend too much space on metadata instead of the selected
  Goal's workbench state.
- The selected Goal page still labeled itself only as `Goal`, weakening the
  intended product model: the Goal detail is the workbench, not just a document.

Fixes:

- Goal-scoped task projection now titles itself as `<Goal> tasks`, shows a
  compact Goal context panel with status, priority, phase count, task count,
  short spec, and a single primary `Open Goal Workbench` action.
- Goal detail now uses `Goal Workbench` as the surface label and replaces the
  document property table with compact metadata chips.
- Desktop and mobile screenshots show no horizontal overflow and no console
  errors after the polish pass.

### Iteration 2026-07-03: Phase Inline Task Relationship View

Browser evidence:

- desktop: `.harness/screenshots/goal-phase-inline/desktop-phase-inline.png`
- mobile phase top: `.harness/screenshots/goal-phase-inline/mobile-phase-inline.png`
- mobile expanded task: `.harness/screenshots/goal-phase-inline/mobile-task-expanded.png`

Problem found:

- Phase-level tasks were rendered as small chips plus a Graph/Kanban toggle.
  This forced the operator to choose between a status board and a route jump,
  while the actual question inside a phase is relationship, detail, acceptance,
  and proof.
- Mobile verification showed that even after replacing the chip view, long
  relationship rows could visually push beyond their card. The final fix hides
  task ids on narrow screens and truncates long relation titles inside the row.

Decision:

- Keep Kanban as a Work-page / goal-scoped status projection only.
- Inside a Goal phase, render a single relationship path with expandable task
  nodes.
- Each expanded task node shows objective, acceptance, ownership, owned paths,
  depends-on, blocks, and proof counts inline. The full Task document remains a
  secondary explicit icon action.

Verification:

- `npx pnpm@9.15.4 check:dashboard` passed.
- Desktop `1440x900`: `kanbanButtonCount=0`, `graphButtonCount=0`,
  `hasInlineDetail=true`, `hasHorizontalOverflow=false`, console errors/warnings
  `0`.
- Mobile `390x844`: `kanbanButtonCount=0`, `graphButtonCount=0`,
  `hasInlineDetail=true`, `hasHorizontalOverflow=false`, overflowing relation
  rows `0`, console errors/warnings `0`.

Follow-up polish from mobile screenshot:

- The old fixed left rail consumed too much mobile width and made phase task
  details feel cramped even when the relationship rows technically fit.
- On small screens the Workbench rail now becomes a bottom navigation bar, and
  document padding drops from desktop spacing to mobile spacing.
- Verification screenshot:
  `.harness/screenshots/goal-phase-inline/mobile-bottom-rail-expanded.png`.
  Mobile `390x844`: `navAtBottom=true`, `hasInlineDetail=true`,
  `hasHorizontalOverflow=false`, console errors/warnings `0`.

### Iteration 2026-07-03: Phase Plan To Workflow View

Product correction:

- Do not expose `Task Graph` as a Goal phase product concept, including as an
  advanced/debug tab. The compiler may still use dependencies and owned-path
  grouping internally, but the operator-facing model is phase plan -> compiled
  workflow -> live workflow run -> gate.
- A phase should show the plan steps and the Starlark workflow shape generated
  from that plan. The latest `WorkflowRun` for the phase should be visible in
  the same phase card and openable directly for realtime execution data.

Decision:

- Replace phase-internal graph language with `Phase plan`.
- Add a `Compiled workflow` panel to every phase, showing the persisted run
  script when available or a generated Starlark preview from the current phase
  plan when no run has been recorded.
- Add a `Live execution` panel beside it, showing the latest phase-linked
  workflow run, step counts, attempts, verdict step, and a direct `Open
  workflow run` action.
- Keep `Task` as the internal assignment/evidence object and as the secondary
  document opened from a step; do not make `Task Graph` a visible navigation or
  debugging surface on the Goal page.

Verification target:

- Browser screenshots must show `Compiled workflow`, `Live execution`, and
  `Phase plan` inside `?surface=goal&goal=goal-content-model-v1`.
- Screenshots must show no visible `Task Graph`, no phase-local Graph/Kanban
  toggle, no horizontal overflow, and no console errors or warnings.

Implementation follow-up:

- Workflow run selection and rollup logic now lives in shared workflow selectors
  instead of `Surfaces.tsx`: phase-linked run lookup, step status counts,
  progress, verdict-step lookup, persisted Starlark script extraction, and phase
  preview compilation.
- Goal phase and Workflow detail share `WorkflowRunSummary` for run status,
  step counts, progress, verdict, attempts, and optional `Open workflow run`.
- Goal phase and Workflow detail share `WorkflowDefinitionPreview` for the
  Starlark source preview. Workflow detail keeps the full timeline, verdict,
  patch/artifact/session drill-in, and Rust definition as the deep surface.
