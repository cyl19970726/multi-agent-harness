# Product Model To UI Rules

Use these rules when translating harness objects into frontend pages.

## Vision

Vision is the long-lived target state and final acceptance standard. It contains
or references a collection of goals.

UI:

- show a Vision page;
- show Goal collection grouped by complete, active, blocked, proposed, and
  archived/rejected;
- show distance-to-vision after each completed goal;
- show next-round proposals generated from GoalEvaluation.

## Goal

Goal is a durable outcome inside a Vision. It is complete only after decision
and evaluation, not merely because tasks are done.

UI:

- show GoalDesign;
- show goal branch and production target;
- show designed AgentTeam and role gaps;
- show goal-level graph for generated goals, blockers, dependencies,
  follow-ups, and distance-to-vision causality;
- show goal-level Kanban/lane view for proposed, active, blocked, review,
  complete, archived, and rejected goals;
- show dynamic TaskGraph and task execution lanes;
- show evidence, review, decision, and GoalEvaluation state.

## Task

Task is the assignable and reviewable unit inside a Goal.

UI:

- show task graph dependencies, blockers, splits, killed paths, and follow-ups;
- show task Kanban/lane state across backlog, ready, running, review, blocked,
  and closed;
- show assignment `Message(kind=task)`;
- show owner, assignee, reviewer, workspace, branch, PR, owned paths, and
  acceptance criteria;
- show messages, provider sessions, evidence, proposal, review, and decision;
- show graph-change proposals and follow-up tasks.

## AgentTeam

AgentTeam is persistent organization.

UI:

- do not treat the team as disposable per task;
- do not default to graph; use role groups, roster, queues, runtime state,
  current task, prompt refs, skill refs, and permissions first;
- show standing team continuity across goals;
- show role groups, status, queue, current task, and health;
- show specialists or temporary members as explicit design decisions.

## AgentMember

AgentMember is a durable teammate identity behind provider runtime.

UI:

- show prompt refs, skill refs, permission profile, runtime status, and current
  task;
- merge inbox, outbox, sessions, events, report, evidence, and proposals into a
  chronological activity stream;
- provide direct send-message and safe delivery/retry/reconcile actions.
