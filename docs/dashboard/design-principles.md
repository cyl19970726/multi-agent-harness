# Agent Dashboard Design Principles

This document owns the core UI/UX principles for the Agent Dashboard. Concrete
screen layout and page cards belong in
[frontend-design.md](frontend-design.md). Frontend module boundaries belong in
[frontend-architecture.md](frontend-architecture.md).

## Doctrine

The Agent Dashboard is an operator workbench for proving and repairing the
harness workflow:

```text
Vision -> Goal collection -> GoalDesign
  -> Goal graph / Goal Kanban
  -> Task graph / Task Kanban
  -> Message assignment -> AgentMember runtime
  -> Report/Evidence -> Proposal/Review -> Decision/Evaluation
  -> distance-to-vision -> NextRoundPlan
```

The UI should make this chain visible without requiring raw JSON, provider
transcripts, or hidden chat context. The first screen should answer:

- what vision and goal are active;
- which goals are complete, not complete, blocked, proposed, or archived;
- which goal or task work can move now;
- which AgentMembers are idle, running, blocked, or stale;
- whether assignment and reporting happened through messages;
- which evidence, review, decision, or warning needs operator action.

The Dashboard is not a metric wall. Summary counts can help orientation, but
they must not push the workflow proof chain below the fold.

The default product feel should be closer to a collaboration workspace than a
BI dashboard. AgentMembers are durable teammates with workbenches, queues,
messages, and status; Goals and Tasks are auditable work documents; graphs are
relationship views that can be opened when they clarify dependencies or
distance-to-vision.

## Vision, Goal, And Task

The layout must preserve this hierarchy:

```text
Vision
  -> Goal collection
  -> selected / active Goal
  -> GoalDesign
  -> Goal graph / Goal Kanban
  -> Task graph / Task Kanban
  -> Messages / AgentMember work
  -> Evidence / Proposal / Review / Decision
  -> GoalEvaluation
  -> distance-to-vision assessment
  -> NextRoundPlan
  -> next proposed Goal / follow-up Task / GoalCase
```

| Concept | Meaning | Dashboard responsibility |
| --- | --- | --- |
| Vision | Long-lived product or project target state plus final acceptance standard. | Show active vision, goal collection, progress signals, remaining gaps, and whether completed goals moved closer to the vision. |
| Proposed Goal | Candidate work from Observer, Critic, warning, prior evaluation, adapter evidence, or user request. | Show source evidence, Lead disposition, and whether it became an accepted goal or was rejected/deferred. |
| Goal | Durable outcome inside a vision. Goals are either complete or not complete; not-complete goals may be active, blocked, or archived. | Show completion state, design completeness, task graph health, evidence/review/decision state, evaluation readiness, and contribution to the vision. |
| GoalDesign | Lead plan for scenario, non-goals, infra, team, initial task graph, evidence, and gates. | Show whether implementation may proceed, which team shape was chosen, and what assumptions govern the task graph. |
| Task | Assignable and reviewable unit inside a goal. | Show assignment message, owner/assignee/reviewer, workspace, evidence, proposal, and decision chain. |
| GoalEvaluation | Evaluator interpretation after acceptance, blockage, kill, or replanning. | Show what worked, what failed, remaining distance to vision, missing infra, follow-ups, and reusable GoalCase candidates. |
| NextRoundPlan | Plan generated after comparing GoalEvaluation with the active vision. | Show why the next proposed goal exists and which vision gap it addresses. |

A goal is not complete because its tasks are done. A goal is complete only when
a Leader decision and GoalEvaluation show that the goal's success criteria were
met or that the goal was explicitly blocked, killed, or replanned.

## Graph And Kanban

Goals and tasks both need two synchronized projections:

- graph/canvas view for dependencies, causality, blocked edges, split/killed
  paths, follow-up links, generated goals, and distance-to-vision relationships;
- Kanban/lane view for operational progress, review queues, blocked work, Lead
  disposition, and what the operator can move next.

These projections must be derived from the same harness read model. The graph
cannot become decorative topology, and the Kanban cannot become a separate
frontend state machine.

Use a controlled canvas, not a freeform whiteboard:

- automatic layout;
- semantic node types;
- collapse/expand by layer;
- minimap and search for large graphs;
- side inspector for selected node;
- no default expansion of every TaskGraph under every Goal.

## AgentTeam And AgentMember

AgentTeams are standing organizations. A goal may reuse the standing team,
adjust it, or add a specialist, but team and member identities should not be
treated as disposable execution surfaces.

Do not make AgentTeam a graph by default. Role groups, queues, status, prompt
refs, permissions, runtime state, current task, and continuity are the primary
team affordances. Reserve graph views for optional message-flow diagnostics.

AgentMember detail must feel live:

- chronological activity stream across inbox, outbox, delivery, provider
  sessions, AgentEvents, reports, evidence, and proposals;
- visible process, endpoint/socket, protocol, and delivery health layers;
- queue, current task, current proposal, prompt refs, skills, and permissions;
- direct send-message and safe deliver/retry/reconcile actions.

Provider-native child threads stay under the parent member unless the store
promotes them to durable AgentMember identity.

## Collaboration Workspace Model

The Dashboard may use a Feishu/Slack-like shell when that helps operators think
in terms of standing teams:

- left side: teams, active vision, goals, docs, warnings, and debug access;
- team workspace: current goal strip, team messages, queues, decision queue,
  and linked goal/task surfaces;
- member workspace: profile, role, current task, inbox, outbox, activity stream,
  runtime health, prompt refs, skills, permissions, and send-message controls;
- goal/task workspace: document-like surface with graph and Kanban tabs;
- docs workspace: mounted project docs linked to Vision, Goal, Task, Evidence,
  and Decision objects.

This shell must not become a chat app. Every visible message, action, warning,
or status should map back to canonical harness objects or explicitly say that it
is advisory/debug information.

## Git And PR Workflow

Git integration follows the product hierarchy. A file-changing goal should own
a goal branch. File-changing tasks may use task worktrees and task branches
that PR into the goal branch. Only after the goal is accepted and evaluated
should the goal branch merge into the production branch.

The Dashboard should place branch, PR, owned-path, and conflict state near
proposal and review surfaces because that is where path violations become
actionable.

## Visual System

Build visual impact from product state:

- live pulses for running members and active sessions;
- explicit state colors for complete, active, blocked, queued, failed, review,
  decision, and warning;
- dense but readable workbench layout;
- dark or hybrid technical theme only if contrast and readability remain strong;
- no decorative elements that do not map to harness state.

Realtime UI should show generated time, polling or streaming state, event age,
active provider sessions, queued messages, direct message affordance, and safe
repair actions.

## Current Failure Modes

The current implementation proves useful data surfaces, but its layout has been
hard to operate:

- snapshot paste/debug controls occupy primary space during live operation;
- too many task statuses can force wide Kanban columns and overflow;
- member details and warnings compete instead of forming one inspector;
- raw object views make the screen feel like a JSON report rather than an
  operating surface;
- member detail splits inbox, outbox, provider sessions, and child threads
  instead of merging them into one chronological activity surface;
- there is no dedicated AgentMember route or focused member page for live
  runtime inspection;
- repeated object versions can leak into React lists and create duplicate key
  warnings.

The redesign should preserve state visibility while changing where that state
appears and how operators move through it.
