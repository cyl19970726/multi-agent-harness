# Agent Team: Goal/Wave Layout (final)

```text
status: accepted
owner_role: product-design
canonical_for: Agent Team frontend information architecture (Goals / Agent Teams / member pages)
supersedes: docs/design/agent-team-layout-v1..final.md (removed; see "Why v1–v4 were discarded")
```

## Why v1–v4 were discarded

The v1–v4 loop designed a *run-centric* console (one TeamRun per page, wave
chain as stepper). Review against the real motivation showed it framed the
product wrong:

- The primary thing a user watches is **how the host agent decomposed the
  goal** — the waves themselves — not one run at a time.
- The existing `GoalPhase` model in the harness is too rigid for this; the
  Goal/Wave model replaces it (wave = a phase of a goal, boundary = an
  integration gate, not time).
- A member is a first-class page (direct conversation + real-time events),
  not a drawer.
- Hidden reasoning is **not** dropped (see Thought visibility).

This document is the layout contract for the Goal/Wave information
architecture.

## Product model

Four layers; Goal/Wave replaces the old GoalPhase concept:

| Layer | Meaning | Notes |
| --- | --- | --- |
| Goal | The objective itself ("what we want"). | Reuses the existing `Goal` entity (`goals.jsonl`). The Goals page makes the host's decomposition of the goal visible. |
| Wave | One phase of the goal. | Boundary = integration gate, not time. Executor: one `AgentTeamRun` (default) **or** the host's own subagents (no team). |
| Agent Team | The executor of a wave when a team is needed. | One wave → one `AgentTeamRun` by default. The Teams list is a first-class sidebar entry; each goal shows which team serves each wave. |
| Member | A `MemberRun` inside the team. | First-class page: direct conversation + real-time events. |

Re-plan between waves is a first-class narrative: after a wave completes,
the host may resolve conflicts and adjust the next wave's plan (objective,
roster, contracts). Those adjustments are displayed between wave cards,
not buried in history.

## Information architecture

Sidebar (left rail) gains two entries next to Agents/Workflows/Docs:

- **Goals** — goal list and the vertical wave view.
- **Agent Teams** — team list (operator inbox) and the team war room.

| Level | Address | Region | User watches | User does |
| --- | --- | --- | --- | --- |
| L0 Goals | `?surface=goals` | goal list | every goal: title, status, wave progress (x/y), which team(s), needs-you | open a goal; create a goal |
| L1 Goal | `?goal=<id>` | vertical wave flow | how the host split the goal; per-wave status, executor, members, gate; re-plan between waves | complete gate; adjust next wave; open a team; open a member |
| L1.5 Team | `?team=<runId>` | team war room | members at a glance; internal event flow; external message flow | message members; ack handoffs; decide approvals |
| L2 Member | `?team=<runId>&teamMember=<id>` | member page | one member's real-time events; conversation with that member; contract | talk to the member directly |

## Goal detail page (the core page)

Vertical wave flow, workflow-phase style: waves stack top→down, each a
card, click to expand. The current wave is expanded by default; completed
waves stay expanded with their outcome; planned waves collapsed.

```text
← Goals
┌ GOAL: Stage 6 migration end-state                       [active] ──┐
│ executors: waves 1–3 → Agent Team "delivery" · wave 4 → host direct │
│ ⛔ needs-you: 2 decisions (aggregated across active waves)          │
├──────────────────────────────────────────────────────────────────────┤
│ ▾ WAVE 1 · unblock development & acceptance           [completed ✓]  │
│   Entry: checkpoint consolidated · Exit: A/B/C merged, gate rerun    │
│   Executor: team delivery-run-1 (3 members) · gate note "verified…"  │
│   ▸ member contract table (member | task | done when | boundaries)   │
│   ▸ outcome: handoffs + evidence links · deviations 2                │
├─ re-plan band ───────────────────────────────────────────────────────┤
│ ⤷ after wave 1: host resolved 1 conflict · adjusted wave 3 plan      │
│   (member +1, tasks rewritten) — diff visible here                   │
├──────────────────────────────────────────────────────────────────────┤
│ ▾ WAVE 2 · data & non-device E2E                        [running]    │
│   Entry: w1 gate passed · Exit: D/E1/E2 evidence complete            │
│   Executor: team delivery-run-2 (3 members) [open team ↗]            │
│   ▸ member contract table (click member → member page)               │
│   ▸ [expanded] embedded team panel: cockpit + live events + messages │
├──────────────────────────────────────────────────────────────────────┤
│ ▸ WAVE 3 · real-device capabilities                     [planned]    │
│   Executor: host's own subagents (no agent team) · task pack: NFC…   │
│ ▸ WAVE 4 · final consolidation                          [planned]    │
│   Contains two operator decision points: deploy · remote delete      │
└──────────────────────────────────────────────────────────────────────┘
```

Re-plan band: between consecutive wave cards, show what the host changed
after the previous wave's gate — conflicts resolved, deviations recorded,
and the diff of the next wave's plan (objective/roster/contract changes).
Planned waves are editable (adjusting the next wave's work is a first-class
operation, not a side effect of starting it).

## Agent Team war room (L1.5)

One page per AgentTeamRun; every region maps to a real mechanism.

```text
← Agent Teams
┌ TEAM: delivery-run-2 · "data & E2E"                    [running] ──┐
│ goal: Stage 6 · wave 2/4 · host: kimi-cli · created · budget limit │
│ ⛔ needs-you: 1 decision · ⚠ 2 unacked (page-level, capped)        │
├─ Members (cockpit) ────────────────────────────────────────────────┤
│ Member │ Role │ Provider/Model │ Status │ Current action │ Last    │
│ (row click → member page; row also filters the internal flow)      │
├─ External flow ✉ (messages) ──┬─ Internal flow ⟳ (events/actions) ─┤
│ ledger oldest-first, max-h    │ newest-first, max-h, filter member │
│ kind pill · from→to · ACK     │ seq · source · type · summary      │
│ evidence badges · reply links │ expand: summary + evidence         │
│ [composer: operator → members]│                                    │
├─ Delegations ──────────────────────────────────────────────────────┤
│ honest empty state until adapters capture native subagents         │
├─ Wave & gate context ──────────────────────────────────────────────┤
│ goal link · wave 2/4 · gate min-conditions · [Complete gate…]      │
│ deviations (non-empty expands)                                     │
└──────────────────────────────────────────────────────────────────────┘
```

The two flows are the two data streams: external = host↔member message
mechanism (kinds, deliveries, ACK/resolve, evidence, reply links);
internal = per-member real-time action/event stream. One subscription,
render-time projections only.

## Member page (L2)

```text
← Team: delivery-run-2
┌ ●E1 shop-journey · kimi/k3                            [testing] ───┐
│ session: kimi-4d21 · worktree · owned: src/shops/** · heartbeat 2s │
├─ Conversation (you ↔ this member) ──┬─ Real-time events ───────────┤
│ messages involving this member      │ member's actions, seq desc   │
│ kind pill · ACK · evidence          │ expand: command/file/test/   │
│ [composer → this member]            │ evidence/thinking entries    │
├─ Contract (## Task / Done when / Boundaries verbatim) ─────────────┤
├─ Delegations (honest empty) · Raw provider stream (collapsed) ─────┤
```

## Thought visibility (policy change)

v0 dropped `agent_thought_chunk` by design. That was wrong: the reasoning
stream is part of "what the member is doing", and member observability is
the product's core promise.

New policy:

- Thinking streams are captured as first-class `MemberAction`s
  (`action_type = "thinking"`), never silently discarded.
- They are marked as **derived reasoning** (muted badge), collapsed by
  default in the UI, and expandable on demand.
- Guardrails: thinking actions are **never** treated as execution evidence
  or acceptance proof (evidence = commands/files/tests/artifacts), and
  **never** forwarded into other members' contexts (no cross-member
  context pollution).

## Data model adjustments

- New first-class `Wave` entity: `{id, goal_id, index, title,
  entry_criteria?, exit_criteria?, status, executor_kind(agent_team|host_subagents),
  team_run_id?, plan_note?, created_at, updated_at}`. Waves with no team
  (host's own subagents) still appear in the flow.
- `AgentTeamRun.goal_id` (+ `wave_id`) links a run into the goal/wave flow;
  `previous_run_id` lineage remains as the re-plan/branch mechanism.
- `Goal` reuses the existing entity; the rigid `GoalPhase` concept is
  retired from the UI over time (execution modes `task_graph`/`workflow`
  remain as non-team executors of a wave).
- Member conversation reuses `TeamMessage` (operator ↔ member); no new
  channel.

## What stays from v0, what gets reshaped

Stays: the six ledger entities, kimi ACP driver, orchestrator, MCP server,
CLI/HTTP surface, plugin package, wave lineage + transitions.

Reshaped: the v0 Teams surface becomes the war room (keep cockpit,
messages, gate); new Goals surface (vertical waves); member drawer is
replaced by the member page; thought chunks are journaled as `thinking`
actions instead of dropped.

## Non-goals (this iteration)

- No automatic re-plan (the host proposes, the operator confirms).
- No per-member control actions beyond messaging (pause/inject/interrupt
  stay CLI/driver-level).
- No goal templates/definitions library yet.
- No cross-goal analytics.
