# Agent Team: Mission/Wave Layout

```text
status: implemented
owner_role: product-design
canonical_for: Mission / Wave / Agent Team frontend information architecture
```

## Product Model

The active execution hierarchy has four distinct layers:

| Layer | Meaning | Product rule |
| --- | --- | --- |
| Mission | Durable intent and closeout. | Contains ordered Waves; no task graph. |
| Wave | Lightweight execution and gate boundary. | Chooses `agent_team`, `dynamic_workflow`, or `host`. |
| Agent Team | One collaborative executor kind. | A Wave may have multiple immutable attempts. |
| MemberRun | One run-scoped member instance. | Ownership comes from assignment-message correlation. |

The former coordination stack is retired under
[ADR 0028](../decisions/0028-retire-goal-phase-task-graph.md). It is neither an
active authoring path nor a compatibility UI. Internal residual fields are
removal debt and do not alter this product model.

This workbench sits beneath the Company OS. Standing Agents and Docs are the
primary company surfaces defined by
[ADR 0027](../decisions/0027-company-os-primary-model.md). Mission/Wave,
Agent Team, and MemberRun pages link back to their WorkItem, source document,
accountable actors, and approval when those relations exist.

## Information Architecture

- `Missions`: collection, Mission detail, ordered Waves, gate, retry, closeout.
- `Agent Teams`: only Mission/Wave-linked AgentTeamRun attempts.
- `Members`: run-scoped drill-in from an Agent Team.

| Level | Surface | User watches | User does |
| --- | --- | --- | --- |
| L0 | Missions | status, Wave progress, executor mix, needs-you | create/open Mission |
| L1 | Mission Canvas | ordered Waves, current attempt, gate, outcome, re-plan note | create/select Wave, open executor, gate, retry, close |
| L1.5 | Team War Room | member presence, assignments, activity, pressure, evidence | message, ACK, start pending, inspect lineage |
| L2 | MemberRun Focus | one member's contract, activity, messages, artifacts | talk to or inspect member |

## Mission Canvas

Waves form one vertical ordered flow. The current Wave expands; accepted Waves
are concise history; planned Waves remain compact until selected. Each Wave
shows objective, exit criteria, executor kind, attempt lineage, gate state,
outcome, artifacts, and any creation-time re-plan note.

The implemented action contract is create/select, open executor, gate, retry,
and Mission closeout. Editing or reordering existing Waves and structured
post-creation re-plan mutation remain follow-up capabilities, not current UI
claims.

## Team War Room

The Team page is one AgentTeamRun attempt for an `agent_team` Wave. It contains:

1. Mission/Wave/attempt header and state.
2. Compact member presence with role, provider/model, action, and pressure.
3. One unified durable Team Activity stream.
4. Message composer and attached operator actions.
5. Wave, Gate, Attempt, Selected Member, and Resources context modules.

The default `All` activity projection preserves attempt creation, assignment
ownership, and the latest pressure-bearing record. `Full record` reveals the
complete durable timeline; category filters operate on that full set. ACK,
review, and start-pending actions attach to the relevant record rather than
creating a duplicate alert band.

Attempt completion remains distinct from the parent Wave gate. Only the Host
can accept, revise, or block a Wave and name an accepted completed attempt.

## MemberRun Focus

The standalone MemberRun page shows role, provider/model, worktree and owned
paths, assignment contract, explicit actions, direct messages, artifacts,
evidence, and observable delegations. Unknown action types use a generic
renderer. A MemberRun is not a Standing Agent, even when both surfaces reuse
shell, conversation, activity, runtime, or identity components.

## Member Lifecycle

1. Validate provider/model, permissions, paths, and budget; persist `starting`.
2. Acquire worktree/runtime lazily; release in reverse order on failure.
3. Stop gracefully: stop new assignments, expire queued deliveries, request
   cancellation, then terminate only where the adapter has real control.
4. Release runtime resources and preserve sanitized durable history.
5. Confirm destructive controls; completed attempts and members are read-only.

Provider-native subagents remain an implementation detail unless hooks expose
honest delegation facts. The harness does not invent lifecycle control.

## Thinking Visibility

Thinking may appear only as a sanitized, expiring, project-scoped live preview.
It is never persisted, replayed, used as evidence, or forwarded to peers.
Durable history contains explicit actions, summaries, blockers, artifacts, and
outcomes.

## Implemented Data Boundary

- `Wave`: `id`, `mission_id`, `index`, `title`, `objective`, `exit_criteria?`,
  `status`, `executor_kind`, `executor_run_ids[]`, `accepted_run_id?`,
  `outcome_summary?`, `artifact_refs[]`, `gate_status`, `gate_note?`,
  `accepted_by?`, `accepted_at?`, `plan_note?`, timestamps.
- `AgentTeamRun`: one immutable attempt linked to its Mission and Wave in the
  active product path.
- `TeamMessage(kind=assignment)` plus `correlation_id`: member lane ownership.
- `MemberAction`, `TeamRunEvent`, artifacts, and outcomes: durable execution
  facts, never private reasoning.

## Visual Contract

The approved expected images, implemented captures, overlays, and intentional
deviations live in
[`execution-workbench-v3/`](execution-workbench-v3/README.md). The canonical
page specs are the
[Mission/Wave Canvas](../dashboard/pages/mission-wave-canvas.md) and
[Agent Team War Room](../dashboard/pages/team-run-war-room.md).

## Non-goals

- No dependency graph, task-centric member ownership, or universal executor
  object.
- No standalone unlinked TeamRun product surface.
- No durable private reasoning.
- No claim that provider-native children are harness-controlled.
- No collapse of MemberRun and Standing Agent identity.
