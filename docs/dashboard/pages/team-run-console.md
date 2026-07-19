# Agent Team Page Spec

```text
status: implemented for native Agent Team Waves; compatibility reader retained
owner_role: product-design
canonical_for: Agent Team war-room page for a Mission/Wave executed by
  executor_kind=agent_team
route_or_surface: Missions -> Mission detail -> Wave -> ?team=<runId>; historical
  Team Run routes remain a compatibility reader for unlinked runs
supersedes: run-centric Team Run Console layout; compatibility path retained for
  historical/manual runs without Mission/Wave linkage
```

## Purpose

Primary user question: what is every member of this Wave's Agent Team doing
right now, what is blocked or waiting on approval, and what should the operator
do next?

Why it exists:

- Agent Team is the collaborative executor for one Wave.
- The operator needs live member state, explicit assignment/handoff/review
  messages, and Wave-gate context without opening raw provider transcripts.
- The page must show ownership through assignment-message correlation, not
  through a first-class Task Graph concept.

Non-goals:

- not a standing-team directory or long-lived employee workspace;
- not a replacement for the top-level Mission page, which creates Waves and
  selects/retries attempts;
- not a metrics wall;
- not a transcript dump;
- not a place that stores private reasoning as durable history.

## Canonical Objects

- native Mission and native Wave (Goal/GoalPhase are labeled compatibility)
- AgentTeamRun
- MemberRun
- TeamMessage
- MemberAction
- DelegationRun
- TeamRunEvent
- Evidence / artifacts

## Workflow Proof

The page must expose this proof chain:

```text
Wave context
  -> TeamMessage(kind=assignment)
  -> correlation_id
  -> member actions / blockers / handoffs / reviews / delegations
  -> artifacts and summaries
  -> Wave gate
```

Rules:

- member state is derived from runtime, explicit actions, queue pressure, and
  durable events, never from provider self-report alone;
- assignment, handoff, blocker, and review messages show delivery state;
- capability degradation is honest;
- a completed Team page becomes read-only history;
- thinking is never persisted as page history or evidence.

## Selected Information Architecture

The selected page is a war room with four regions:

1. Header: Mission/Wave identity, run status, host surface, budget.
2. Member cockpit: each member's provider/model, status, current action,
   pressure, last update.
3. External flow: host/operator <-> member message ledger with delivery state.
4. Internal flow: newest-first action/event stream for the run.

The current Wave gate context stays visible in the same page. Operators should
not need to leave the Team page to understand whether the team is close to the
gate or blocked before it. Completing a TeamRun is not the same as accepting
the Wave: completion makes an attempt eligible; the Host records the separate
Wave gate from the parent Mission/Wave surface.

## Primary Actions

- message one or more members;
- acknowledge or re-deliver key messages;
- review blocked or waiting-for-approval states;
- open a member page;
- open the parent Mission/Wave detail;
- complete this attempt, or cancel it while no provider execution is active;
  gate the completed attempt from the parent Mission/Wave surface.

## Responsive Requirements

- Desktop: header + cockpit + two-column flow view.
- Tablet: cockpit and flows stack, member filters remain visible.
- Mobile: cockpit-first, then alerts, then message/action streams.
- No horizontal overflow.

## Layout Contract

### Desktop

Target viewport: about `1440x960`.

```text
+----------------------------------------------------------------------------+
| Agent Team: Wave 2 "data verification"        running   budget   host kind  |
+----------------------------------------------------------------------------+
| Cockpit                                                                    |
| Member | Role | Provider/Model | Status | Current action | Pressure | Last |
+--------------------------------------+-------------------------------------+
| External flow (messages)             | Internal flow (actions/events)       |
| oldest-first ledger                  | newest-first stream                  |
| kind, from->to, delivery state       | action type, summary, artifacts      |
| composer                             | filter by member                     |
+--------------------------------------+-------------------------------------+
| Wave context: objective, exit criteria, gate summary, deviations           |
+----------------------------------------------------------------------------+
```

### Tablet

Target viewport: about `900x1180`.

```text
+--------------------------------------------------------------+
| Agent Team: Wave 2                          running          |
+--------------------------------------------------------------+
| Cockpit                                                      |
+--------------------------------------------------------------+
| External flow                                                |
+--------------------------------------------------------------+
| Internal flow                                                |
+--------------------------------------------------------------+
| Wave context                                                 |
+--------------------------------------------------------------+
```

### Mobile

Target viewport: about `390x844`.

```text
+--------------------------------------+
| Agent Team: Wave 2   running         |
+--------------------------------------+
| Cockpit rows                         |
+--------------------------------------+
| Alerts / blocked / approvals         |
+--------------------------------------+
| Messages                             |
+--------------------------------------+
| Actions                              |
+--------------------------------------+
| Wave context                         |
+--------------------------------------+
```

## Thinking Policy

Thinking is transient live-only state.

- If a provider exposes it, the host may render a sanitized preview from a
  project-scoped SSE `member_activity` frame.
- Previews carry an expiry/TTL and are removed by expiry or refresh; a reconnect
  does not receive old activity.
- The activity channel is direct-only: it is not JSONL, snapshot, replay,
  evidence, message context, or a substitute for explicit status/actions.
- Stored history shows explicit actions, blockers, summaries, artifacts, and
  outcomes only.

## Member Lifecycle Surface

The Team page must make lifecycle transitions honest without becoming a
process manager UI:

- `starting`, `idle`, `queued`, `running`, `waiting`, `reviewing`, `blocked`,
  `completed`, `failed`, `stopping`, and `stopped` are visibly distinct;
- add/start failures show what resource acquisition failed and what was
  released;
- stopping a working member requires confirmation and prevents new
  assignments;
- the lead cannot be removed;
- finished runs are read-only, while preserved actions, messages, handoffs,
  reviews, and artifacts remain inspectable.

## Failure Modes Prevented

- hiding ownership behind a task-graph-only explanation;
- implying a Wave must be a Task Graph;
- losing message delivery state;
- presenting host-native delegation as fully controlled when it is only
  observed;
- storing private reasoning as audit history;
- forcing operators into raw provider transcripts to answer basic Wave questions.

## Current Boundary

- Native, Mission-linked TeamRuns are created and retried from the parent Wave;
  `previous_run_id` is same-Wave retry lineage.
- Start reserves the attempt synchronously, then provider execution proceeds in
  the background; durable run/member/message/event changes stream to the
  selected project's SSE read model.
- Running-provider interruption is not yet a Console control. Until cooperative
  provider cancellation exists, the UI does not present status-only cancellation
  as if it stopped active work.
- The standalone Team Run list is compatibility-only for unlinked historical or
  manual runs. It is not an alternative Mission planning surface.
- Dynamic Workflow and Host remain executor seams. This page does not claim
  that those executor kinds can already be launched or controlled here.
