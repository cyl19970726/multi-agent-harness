# Agent Dashboard Layout Variants

This document holds candidate Dashboard UI/UX directions before one direction is
accepted into [frontend-design.md](frontend-design.md). It is the place for
Designer variants, Questioner critique, scoring, and rejected alternatives.

## Product Context

Multi-Agent Harness is a coordination layer for persistent Agent Teams. The
Dashboard must show that this workflow really happened:

```text
Vision -> Goal collection -> GoalDesign -> AgentTeam -> TaskGraph
  -> Message -> AgentMember work -> Evidence -> Proposal/Review
  -> Decision -> GoalEvaluation -> distance-to-vision -> next Goal
```

The current frontend design goal is to reduce distance-to-vision by making that
chain visible and operable without raw JSON or hidden chat context. The
Dashboard should feel like a multi-agent collaboration control plane: agents are
durable teammates, goals and tasks are auditable work documents, and graph views
explain relationships without becoming the default mental model for everything.

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
accepted, which alternatives were rejected, and why the selected direction
serves the product Vision.

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

The Questioner must be independent and objective. It does not serve the
Designer and does not reward visual polish unless the design improves workflow
proof, operation speed, and acceptance.

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

The likely product direction is a hybrid:

```text
Team workspace shell
  + Goal/Task document surfaces
  + controlled graph/Kanban tabs
```

Use Candidate 1 as the overall shell because Multi-Agent Harness is a
persistent team product. Use Candidate 2 for Goal and Task detail because
goals/tasks need auditable document-like surfaces. Use Candidate 3 only for
relationship views: Vision goal graph, selected Goal task graph, blockers,
follow-ups, and distance-to-vision.

This synthesis is not final implementation approval. Before coding, the
Decision Agent must score the candidates, record the accepted/rejected tradeoffs
in this document or an ADR, and move the accepted layout into
[frontend-design.md](frontend-design.md).

## Next Module Option Loops

After selecting the top-level direction, run smaller three-option loops for:

- Team list and team detail workspace;
- AgentMember workbench, status header, inbox/outbox, activity stream, and
  prompt/skills/permissions panel;
- Goal document, Goal graph/Kanban switch, GoalEvaluation, and
  distance-to-vision area;
- Task document, Task graph/Kanban switch, assignment proof, and
  branch/worktree/PR area;
- Dashboard-mounted docs navigation and contextual docs panel;
- Evidence, Review, Decision, Warnings, and Debug drawer;
- desktop, tablet, and mobile placement.
