# Agent Team: Mission/Wave Layout

```text
status: stable
owner_role: product-design
canonical_for: Mission / Wave / Agent Team frontend information architecture
compatibility_note: file path and some current routes still say goal/team-run
  until the runtime and dashboard migration lands
```

## Why Earlier Layouts Were Replaced

Earlier iterations treated one Team Run as the main page. That framed the
product incorrectly.

- The operator's primary question is how the Host split a Mission into Waves.
- A Wave may be executed by `agent_team`, `dynamic_workflow`, or `host`.
- Agent Team is one executor kind, not the whole product hierarchy.
- A Wave is lightweight. It does not require a Task Graph.
- Thinking is not durable history; at most it is a transient live signal.

This document is the layout contract for the accepted Mission/Wave information
architecture.

## Product Model

Four layers:

| Layer | Meaning | Notes |
| --- | --- | --- |
| Mission | The durable objective. | Canonical product term. Current store/runtime still uses `Goal` as a compatibility surface. |
| Wave | One ordered unit inside a Mission. | Boundary = integration gate, not time. Executor kinds: `agent_team`, `dynamic_workflow`, `host`. |
| Agent Team | The `agent_team` executor for a Wave. | One Wave may instantiate multiple `AgentTeamRun` attempts; its gate identifies the accepted attempt. |
| Member | A `MemberRun` inside the Agent Team. | First-class page with contract, explicit actions, messages, and artifacts. |

`Standing Agents + Docs` is future and does not appear as a first-class current
navigation surface in this iteration.

## Information Architecture

The canonical navigation is:

- `Missions`: collection and Mission detail.
- `Agent Teams`: wave-scoped collaborative runs.
- `Members`: drill-in from a Team.

Compatibility note: current routes and components may still expose `goal`,
`goals`, or `team-run` names until migration lands. The product copy and doc
contracts should use Mission/Wave now.

| Level | Region | User watches | User does |
| --- | --- | --- | --- |
| L0 Missions | Mission list | mission status, wave progress, executor mix, needs-you | open a Mission, create a Mission |
| L1 Mission | vertical Wave flow | per-Wave objective, executor, gate, outcome, re-plan bands | complete a gate, adjust a later Wave, open a Team or member |
| L1.5 Team | Agent Team war room | member state, external message flow, internal action/event flow, Wave gate context | message members, ack handoffs, decide approvals |
| L2 Member | member page | one member's contract, actions, messages, artifacts, delegations | talk to the member directly |

## Mission Detail Page

Mission detail is the core page. Waves stack vertically. The current Wave is
expanded by default; completed Waves remain readable history; future Waves are
collapsed but editable at plan level.

Each Wave card shows:

- Wave title and status;
- objective and exit criteria;
- executor kind: `agent_team`, `dynamic_workflow`, or `host`;
- lightweight gate state;
- outcome summary and artifact links;
- deviations and re-plan deltas for the next Wave.

The page must make the Host's replanning visible between Waves instead of
burying it in logs.

## Team Page

The Team page is the L1.5 war room for a Wave whose executor is `agent_team`.

It has four regions:

1. Header: Mission/Wave identity, host surface, budget, run status.
2. Member cockpit: each member's role, provider/model, status, current action,
   unread/blocked pressure.
3. External flow: host/operator <-> member message ledger with delivery state.
4. Internal flow: newest-first action/event stream for the run.

The Team page does not make Task Graph the center of the experience. Ownership
is explained through assignment-message correlation and explicit Wave context.

## Member Page

The Member page is the durable drill-in for one `MemberRun`.

It shows:

- role, provider/model, worktree, owned paths, current contract;
- explicit action timeline: commands, tests, file changes, reviews, delegation,
  waiting, blocked, completed;
- conversation/messages with the member;
- artifacts and evidence links;
- delegation observations when available.

Unknown future action types should fall back to a generic renderer. New member
behaviors should extend vocabulary, not force new page types.

## Member Lifecycle

Member lifecycle is run-scoped and resource-disciplined:

1. Add member: validate provider/model, owned paths, permissions, and budget;
   persist `MemberRun(status=starting)` before acquiring runtime resources.
2. Start lazily: acquire worktree when needed, then provider session, then move
   to `idle`/ready. A failed acquisition releases resources in reverse order
   and records an explicit failure action.
3. Stop gracefully: refuse new assignments, expire queued deliveries, request
   provider cancellation, then terminate the process tree after a bounded grace
   window if needed.
4. Release and preserve history: reap process/thread handles, release the
   worktree, archive sanitized wire/session artifacts, and persist
   `status=stopped` plus `ended_at`.
5. Guard destructive controls: stopping a working member needs confirmation;
   the lead member cannot be removed; finished runs and their members are
   read-only.

One runtime child and one orchestrator owner must have a clear shutdown path.
The resident service needs a member registry so run end, explicit stop, and
service shutdown cannot leave orphan provider processes.

## Thinking Visibility

Thinking is transient live-only state.

- It may appear in host-local live UI when a provider exposes it.
- It is never persisted as canonical page history.
- It is never replayable after refresh.
- It is never evidence.
- It is never forwarded into another member's context.

The stored page history should show explicit actions, summaries, blockers,
artifacts, and outcomes instead.

## Data Model And Compatibility Notes

- Planned first-class `Wave` product shape:
  `{id, mission_id, index, title, objective, exit_criteria?, status,
  executor_kind(agent_team|dynamic_workflow|host), executor_run_ids[],
  accepted_run_id?, outcome_summary?, artifact_refs[], gate_status,
  gate_note?, created_at, updated_at}`.
- `AgentTeamRun` attempts link to the Wave executed by
  `executor_kind=agent_team`.
- Current store/runtime may still use `Goal`, `GoalPhase`, `task_id`, and older
  route names during migration.
- Host-native subagents remain host/provider implementation detail unless
  optional hooks expose observable delegation facts.

## Non-goals

- No standing-team directory or cross-Mission inbox in this iteration.
- No automatic replanning without Host/operator confirmation.
- No requirement that every Wave expose a Task Graph.
- No durable storage of private reasoning.
