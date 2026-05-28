# Layout Principles

## Graph And Kanban

Use graph for relationships and causality:

- Vision to Goal collection;
- Goal to generated/follow-up goals;
- Goal dependencies, blockers, distance-to-vision links, and next-round plans;
- Task dependencies and blockers;
- evidence/review/decision links when inspecting provenance.

Use Kanban or lane views for execution:

- Goal status across proposed, active, blocked, complete;
- Goal review/evaluation readiness and Lead disposition queues;
- Task status across backlog, ready, running, review, blocked, closed;
- review and decision queues.

Do not force AgentTeam into graph as the default view. Use an operations layout:
roster, role groups, queues, runtime health, current task, and recent activity.

## Canvas

Use a controlled canvas, not a freeform whiteboard:

- automatic layout;
- semantic node types;
- collapse/expand by layer;
- minimap and search for large graphs;
- side inspector for selected node;
- no default expansion of every TaskGraph under every Goal.

## Visual System

Build visual impact from product state:

- live pulses for running members and active sessions;
- explicit state colors for complete, active, blocked, queued, failed, review,
  decision, and warning;
- dense but readable workbench layout;
- dark or hybrid technical theme only if contrast and readability remain strong;
- no decorative elements that do not map to harness state.

## Realtime Feel

Realtime UI should show:

- generated time and polling/streaming state;
- event age and last activity;
- in-flight provider sessions;
- queued messages;
- direct message affordance;
- visible safe actions for deliver, retry, reconcile, request review, and close.
