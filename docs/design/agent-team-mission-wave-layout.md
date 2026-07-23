# Agent Team: Mission/Wave Layout

```text
status: implemented
owner_role: product-design
canonical_for: Mission / Host-plan Wave / Agent Team frontend information architecture
architecture: ADR 0034
```

## Product Model

| Object | Meaning | Product rule |
| --- | --- | --- |
| Mission | Durable intent, Markdown context, team relations, and closeout. | Contains ordered Waves; links zero or more independent teams. |
| Wave | Versioned Host plan and judgment. | Not a task graph, executor container, barrier, or session boundary. |
| Agent Team | Independent reusable collaborator definition. | May be standalone or linked to Missions. |
| AgentTeamRun | One use of a team. | Mission-scoped runs may span several Waves. |
| MemberRun | One run-scoped participant and native-session binding. | Assignment-message correlation owns work. |

Standing Agents and Docs remain separate Company OS surfaces. They may share
shell, avatar, activity, conversation, and compact-control primitives with
Agent Team pages, but never identity or lifecycle semantics.

## Information Architecture

- `Missions`: collection, Mission context, linked teams, ordered Wave history,
  advance, and closeout.
- `Agent Teams`: independent definitions and standalone/Mission-scoped runs.
- `Members`: run-scoped drill-in from Team controls.

| Level | Surface | User watches | User does |
| --- | --- | --- | --- |
| L0 | Missions | status, current judgment, linked teams, needs-you | create/open Mission |
| L1 | Mission Canvas | long Mission context, ordered Waves, responsibilities, carry-over | update/advance Wave, link/open Team, close |
| L1.5 | Team War Room | members, assignments, activity, pending interactions, evidence | message, ACK, add/steer/interrupt/resume member |
| L2 | Member Focus | one member's native work history and coordination | chat, inspect, control, open artifacts |

## Mission Canvas

Waves form a vertical ordered flow. The selected Wave renders full Markdown,
including an optional responsibility table. Mission-linked Team controls live
at Mission scope. Member/Team controls inside Wave context are navigational
projections only; the Wave does not own them.

Advancing a Wave records a Host outcome and may summarize active carry-over.
It does not require every member to finish. Creating Wave N+1 preserves the
same TeamRun, MemberRun, assignment correlation, and native session unless the
Host explicitly changes them.

## Team War Room

The Team page contains:

1. independent team identity and current TeamRun;
2. compact member controls with role, provider/model, action, pressure, and
   project-default portraits;
3. one source-aware Team Activity stream;
4. Team/@member composer and record-attached actions;
5. Mission/current-Wave orientation, selected member, runtime, and artifact
   modules.

Harness coordination and ephemeral provider-native projections render
together but remain source-labelled. Provider transcript, tool, command, file,
turn, and thinking streams are not copied into Harness ledgers.

## Member Focus

The standalone MemberRun page follows the Codex-like working layout:

- header and identity;
- central chronological work history and chat;
- semantic Markdown handoff, tool/activity groups, artifacts, and checks;
- right-rail Team, Mission/Wave orientation, runtime, native session, and
  artifacts;
- real chat, PendingInteraction, steer, interrupt, and resume controls only
  when the provider adapter supports them.

Unknown native activity uses a generic source-labelled renderer. A MemberRun is
not a Standing Agent.

## Member Lifecycle

1. Validate provider mode/version, permissions, paths/worktree, and budget.
2. Persist MemberRun and bind a provider-native session.
3. Assign through correlated TeamMessage.
4. Continue interaction and resume through the real native session.
5. Add, rename, deactivate, or stop explicitly; Wave advance changes none of
   these automatically.
6. Preserve Harness coordination and native-session locator after terminal
   state; never mirror private provider history.

Provider-native subagents remain implementation detail unless hooks expose
honest attribution. The Harness does not invent lifecycle control.

## Implemented Data Boundary

- `Mission.context`, `Mission.agent_team_ids[]`
- `Wave.context`, `Wave.revision`, `Wave.updated_by`, ordered append-only
  history, explicit outcome/artifacts/advance
- `AgentTeamRun.agent_team_id`, optional `mission_id`, legacy optional
  `wave_id`
- `MemberRun.native_session`
- `TeamMessage(kind=assignment)`, `correlation_id`, optional `origin_wave_id`

Legacy `executor_kind`, attempt-list, accepted-run, and gate fields remain
readable for direct-Wave-executor history only.

## UX And Visual Contract

Every design reference must pair:

1. expected image;
2. interaction/animation/state annotations;
3. actual browser capture from a deterministic fixture; and
4. comparison with classified defects or intentional deviations.

The current visual assets live in
[`execution-workbench-v3/`](execution-workbench-v3/README.md). Canonical
behavior is owned by the
[Mission/Wave Canvas](../dashboard/pages/mission-wave-canvas.md) and
[Agent Team War Room](../dashboard/pages/team-run-war-room.md) page specs.

## Non-goals

- dependency/task graph or universal executor object;
- Wave-owned TeamRun lifecycle;
- automatic member/team deletion at Wave or Mission closeout;
- durable private reasoning or provider-event mirror;
- collapse of MemberRun and Standing Agent identity.
