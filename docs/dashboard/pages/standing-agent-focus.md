# Standing Agent Focus Page Spec

```text
status: partial implementation — explicit Agent Team participation projection is implemented
owner_role: product-design
canonical_for: one durable AgentMember across assignments and provider sessions
route_or_surface: Agents -> AgentMember
```

## Product question

The operator opens a long-lived teammate to answer: is this Agent available,
which explicit contexts is it serving, what has it done across those contexts,
and can I safely message it now?

This is not a `MemberRun` page. A standing `AgentMember` retains identity across
provider restarts and may participate in multiple Missions or Workflows. A
`MemberRun` remains one participation in one `AgentTeamRun` attempt.

The first implemented slice uses the same explicit stable identifier across the
Organization `StandingAgent`, the independent Agent Team `AgentMember`
definition, and `MemberRun.agent_member_id`. Creating a TeamRun from an
AgentTeam definition preserves that identifier on each MemberRun. Ad-hoc
members may supply the link explicitly only when the referenced AgentMember
exists. The Company OS snapshot then derives `standing_assignments` from latest
native rows; it never creates another assignment ledger.

## Object boundary

```mermaid
flowchart LR
  A["AgentMember\ndurable identity"] --> R["AgentRuntime"]
  A --> S["NativeSessionRef"]
  A --> X["StandingAssignment\nread-only projection"]
  X --> M["Mission / Wave"]
  X --> W["WorkflowRun / Step"]
  X --> D["Direct message"]
  M --> MR["MemberRun\none TeamRun participation"]
  MR -. "explicit agent_member_id only" .-> A
```

Shared UI does not imply shared lifecycle. Never join an `AgentMember` to a
`MemberRun` by name, role, provider, model, or temporal proximity.

## Required read model

### AgentMember availability

The snapshot needs explicit optional fields rather than deriving availability
from `idle` or a healthy process:

```text
availability = available | busy | paused | offline | unknown
assignment_capacity: integer | null
exclusive_assignment_ref: string | null
```

Missing values render as `Availability not reported`. `Available` means the
Agent can accept work under its capacity and permission policy; it does not
mean the Agent has no history or no active non-exclusive assignments.

### StandingAssignment projection

`StandingAssignment` is a read-only cross-executor projection, not a legacy dependency graph
and not a new universal executor:

```text
id
agent_member_id
source_kind = mission_wave | workflow_participation | direct_assignment
source_ref
mission_id? / wave_id?
team_run_id? / member_run_id?
workflow_run_id? / workflow_step_id?
title
role
status
assigned_at
last_activity_at?
navigation_target
```

Projection rules:

- Mission/Wave participation requires an explicit `MemberRun.agent_member_id`
  or equivalent stable source link.
- The current implementation projects Agent Team assignments only. It includes
  a Mission reference when the TeamRun is Mission-scoped; a Wave reference is
  shown only when an existing compatibility record supplies one. A Wave does
  not own the MemberRun.
- Workflow participation uses an explicit step owner and the step's
  `NativeSessionRef` when provider-native activity is available.
- A direct assignment must be an explicit assignment/task message addressed to
  the durable AgentMember; ordinary conversation is activity, not assignment.
- Missing links remain missing. The UI must not fall back to legacy
  `current_task_id` to invent a cross-executor assignment.
- Retries are lineage of one source assignment, not duplicate active work.

## Layout contract

Use the shared `FocusShell`: continuous activity/conversation in the center,
sticky composer, and a composed Context Rail. The active execution-workbench
visual contract is `docs/design/execution-workbench-v3/visual-contract.json`;
the retired `workbench-layout-v2` concept package is no longer an active
design source.

Center reading order:

1. durable identity header: name, availability, `Standing Agent`, provider and
   model;
2. availability banner, including exclusive-assignment truth;
3. chronological cross-context activity: direct messages, explicit assignment
   entries, workflow participation, delivered artifacts, provider-native
   activity projections, and agent replies;
4. composer addressed to the AgentMember, with queue/busy behavior stated
   honestly.

Context Rail order:

1. Agent Profile;
2. Availability and capacity;
3. Active Assignments;
4. Capabilities and skills;
5. Runtime and permission/workspace boundaries;
6. Provider Sessions and observed child threads.

At tablet width the product navigation uses the shared compact rail and context
moves behind `Context & controls`. Mobile preserves one central stream and a
fixed composer; context remains a disclosure rather than disappearing.

## Explicit non-goals

- no Mission > Wave > Team breadcrumb as page ownership;
- no Wave gate or TeamRun retry lifecycle at Agent level;
- no legacy dependency graph requirement;
- no inference that a provider-native child thread is another AgentMember;
- no persisted thinking in the activity stream;
- no fake capacity, capability, assignment, interrupt, or wake state;
- no reuse of legacy `Tasks` as the primary cross-executor model.

## Implemented state

The Store-live Organization chart routes Human and Standing Agent cards to
their profile. A Standing Agent profile now shows:

- WorkItems linked through Company OS Actor references;
- Agent Team assignments linked through `MemberRun.agent_member_id`;
- the exact TeamRun, MemberRun, assignment correlation, status, and optional
  provider-native session locator;
- a deep link to the Team/Member execution surface; and
- an explicit empty state when no MemberRun is linked.

The profile continues to keep organizational status, business authority, and
runtime participation separate. A running member does not grant authority, and
an offline runtime does not retire a Standing Agent.

## Remaining representative state

`available-multi-assignment` proves:

- one healthy durable AgentMember with explicit `available` state;
- multiple non-exclusive assignments from at least two source kinds;
- capacity remains available;
- provider-native session history reached through explicit native-session refs;
- a delivered artifact and durable message history;
- no single Wave owns the page and no thinking is persisted.

## Remaining gates

This page is not complete until:

1. availability and capacity ownership are implemented rather than inferred;
2. Workflow participation and direct durable-agent messaging gain equally
   explicit source links;
3. the candidate expected design is marked approved in the visual contract;
4. deterministic Dashboard interaction coverage proves chart-to-profile and
   profile-to-MemberRun navigation at supported viewports; and
5. a durable Team Supervisor can report cross-client member runtime health.
